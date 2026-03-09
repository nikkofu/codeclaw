use crate::{
    app_server::{AppServerClient, Notification},
    config::{Config, CoordinationPaths, GroupConfig},
    orchestration::{
        parse_master_response, visible_stream_text, MasterAction, ParsedMasterResponse,
    },
    session::{SessionEventKind, SessionSnapshot, SessionView},
    state::{now_unix_ts, AppState, SessionStatus, WorkerRecord, WorkerStatus},
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{broadcast, oneshot};

const MASTER_SESSION_ID: &str = "master";

#[derive(Clone)]
pub struct Controller {
    workspace_root: PathBuf,
    pub config: Config,
    pub paths: CoordinationPaths,
    state: Arc<Mutex<AppState>>,
    sessions: Arc<Mutex<BTreeMap<String, SessionView>>>,
    pending_turns: Arc<Mutex<BTreeMap<String, VecDeque<QueuedTurn>>>>,
    active_turn_batches: Arc<Mutex<BTreeMap<String, u64>>>,
    next_batch_id: Arc<Mutex<u64>>,
    client: Arc<AppServerClient>,
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub config_source: String,
    pub coordination_root: PathBuf,
    pub codex_app_server_ok: bool,
    pub thread_start_ok: bool,
}

#[derive(Debug, Deserialize)]
struct ThreadStartResponse {
    thread: ThreadSummary,
}

#[derive(Debug, Deserialize)]
struct TurnStartResponse {
    turn: TurnSummary,
}

#[derive(Debug, Deserialize)]
struct ThreadSummary {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TurnSummary {
    id: String,
    status: String,
    error: Option<TurnError>,
}

#[derive(Debug, Deserialize)]
struct TurnCompletedNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    turn: TurnSummary,
}

#[derive(Debug, Deserialize)]
struct TurnStartedNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    turn: TurnSummary,
}

#[derive(Debug, Deserialize)]
struct ThreadStatusChangedNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    status: Value,
}

#[derive(Debug, Deserialize)]
struct AgentMessageDeltaNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    delta: String,
}

#[derive(Debug, Deserialize)]
struct ErrorNotification {
    error: TurnError,
    #[serde(rename = "willRetry")]
    will_retry: bool,
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
}

#[derive(Debug, Deserialize)]
struct ItemLifecycleNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    item: ThreadItem,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ThreadItem {
    #[serde(rename = "agentMessage")]
    AgentMessage { text: String },
    #[serde(rename = "commandExecution")]
    CommandExecution {
        command: String,
        status: String,
        #[serde(rename = "exitCode")]
        exit_code: Option<i64>,
        #[serde(rename = "aggregatedOutput")]
        aggregated_output: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TurnError {
    pub message: String,
    #[serde(rename = "additionalDetails")]
    pub additional_details: Option<String>,
}

#[derive(Debug, Clone)]
pub enum PromptTarget {
    Master,
    Worker(String),
}

#[derive(Debug, Clone)]
enum SessionRole {
    Master,
    Worker(String),
}

#[derive(Debug, Clone)]
enum TurnSource {
    User,
    Bootstrap,
    Orchestrator,
    Runtime,
}

impl TurnSource {
    fn log_prefix(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Bootstrap => "system",
            Self::Orchestrator => "orchestrator",
            Self::Runtime => "runtime",
        }
    }

    fn format_prompt(&self, prompt: &str) -> String {
        match self {
            Self::User => prompt.to_owned(),
            Self::Bootstrap | Self::Orchestrator | Self::Runtime => compact_message(prompt),
        }
    }

    fn event_kind(&self) -> SessionEventKind {
        match self {
            Self::User => SessionEventKind::User,
            Self::Bootstrap => SessionEventKind::Bootstrap,
            Self::Orchestrator => SessionEventKind::Orchestrator,
            Self::Runtime => SessionEventKind::Runtime,
        }
    }

    fn timeline_text(&self, prompt: &str, queued: bool, pending_count: usize) -> String {
        let verb = if queued { "queued" } else { "started" };
        match self {
            Self::User => {
                if queued {
                    format!(
                        "{verb} prompt ({pending_count} waiting): {}",
                        compact_message(prompt)
                    )
                } else {
                    format!("{verb} prompt: {}", compact_message(prompt))
                }
            }
            Self::Bootstrap => {
                if queued {
                    format!("{verb} bootstrap task ({pending_count} waiting)")
                } else {
                    "started bootstrap task".to_owned()
                }
            }
            Self::Orchestrator => {
                if queued {
                    format!(
                        "{verb} follow-up ({pending_count} waiting): {}",
                        compact_message(prompt)
                    )
                } else {
                    format!("{verb} follow-up: {}", compact_message(prompt))
                }
            }
            Self::Runtime => {
                if queued {
                    format!("{verb} internal runtime update ({pending_count} waiting)")
                } else {
                    "started internal runtime update".to_owned()
                }
            }
        }
    }
}

struct QueuedTurn {
    batch_id: u64,
    session_id: String,
    thread_id: String,
    log_label: String,
    role: SessionRole,
    prompt: String,
    wait_for_follow_on: bool,
    source: TurnSource,
    completion: oneshot::Sender<Result<()>>,
}

impl Controller {
    pub async fn start(workspace_root: PathBuf) -> Result<Self> {
        let config = Config::load(&workspace_root)?;
        let paths = config.coordination_paths(&workspace_root);
        let state = AppState::load(&paths.state_file)?;
        let sessions = build_sessions(&state, &workspace_root);
        let client = Arc::new(
            AppServerClient::spawn(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                &config.master.reasoning_effort,
            )
            .await?,
        );

        Ok(Self {
            workspace_root,
            config,
            paths,
            state: Arc::new(Mutex::new(state)),
            sessions: Arc::new(Mutex::new(sessions)),
            pending_turns: Arc::new(Mutex::new(BTreeMap::new())),
            active_turn_batches: Arc::new(Mutex::new(BTreeMap::new())),
            next_batch_id: Arc::new(Mutex::new(1)),
            client,
        })
    }

    pub fn init_workspace(&self) -> Result<Option<PathBuf>> {
        let config_path = Config::write_default_config_if_missing(&self.workspace_root)?;
        self.paths.ensure_layout()?;
        self.save_state()?;
        self.write_master_status(
            "idle",
            self.master_last_turn_id(),
            self.master_last_message(),
        )?;
        Ok(config_path)
    }

    pub async fn doctor(&self) -> Result<DoctorReport> {
        let client = AppServerClient::spawn(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            &self.config.master.reasoning_effort,
        )
        .await?;
        let response: ThreadStartResponse = client
            .request(
                "thread/start",
                json!({
                    "cwd": self.workspace_root,
                    "sandbox": self.config.master.sandbox,
                    "approvalPolicy": self.config.master.approval,
                    "ephemeral": true,
                    "personality": "pragmatic",
                }),
            )
            .await
            .context("app-server thread/start probe failed")?;

        Ok(DoctorReport {
            config_source: self
                .workspace_root
                .join("codeclaw.toml")
                .display()
                .to_string(),
            coordination_root: self.paths.root.clone(),
            codex_app_server_ok: client.is_running().await.unwrap_or(false),
            thread_start_ok: !response.thread.id.is_empty(),
        })
    }

    pub fn groups(&self) -> Vec<GroupConfig> {
        self.config.groups.clone()
    }

    pub fn sessions_snapshot(&self) -> Vec<SessionSnapshot> {
        let sessions = self.sessions.lock().expect("sessions lock poisoned");
        let mut snapshots = sessions
            .values()
            .map(SessionView::snapshot)
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| {
            session_sort_key(&left.id)
                .cmp(&session_sort_key(&right.id))
                .then_with(|| left.title.cmp(&right.title))
        });
        snapshots
    }

    pub fn list_workers(&self) -> Vec<WorkerRecord> {
        let state = self.state.lock().expect("state lock poisoned");
        state.workers.values().cloned().collect()
    }

    pub async fn ensure_master_thread(&self) -> Result<String> {
        self.paths.ensure_layout()?;

        if let Some(thread_id) = self.master_thread_id() {
            if self
                .resume_thread(&thread_id, Some(self.master_instructions()))
                .await
                .is_ok()
            {
                self.ensure_master_session(thread_id.clone());
                self.write_master_status(
                    "idle",
                    self.master_last_turn_id(),
                    self.master_last_message(),
                )?;
                return Ok(thread_id);
            }
        }

        let response = self
            .start_thread(
                "master",
                Some(self.master_instructions()),
                Some("master"),
                false,
            )
            .await?;
        let thread_id = response.thread.id;

        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state.master_thread_id = Some(thread_id.clone());
            state.master_last_turn_id = None;
            state.master_last_message = None;
        }
        self.save_state()?;
        self.ensure_master_session(thread_id.clone());
        self.write_master_status("idle", None, None)?;
        Ok(thread_id)
    }

    pub async fn submit_prompt(&self, target: PromptTarget, prompt: &str) -> Result<()> {
        let (session_id, thread_id, log_label, role) = self.resolve_prompt_target(target).await?;
        let batch_id = self.allocate_batch_id();
        self.enqueue_turn(
            batch_id,
            session_id,
            thread_id,
            log_label,
            role,
            prompt,
            false,
            TurnSource::User,
        )
    }

    pub async fn submit_prompt_and_wait(&self, target: PromptTarget, prompt: &str) -> Result<()> {
        let (session_id, thread_id, log_label, role) = self.resolve_prompt_target(target).await?;
        let batch_id = self.allocate_batch_id();
        self.enqueue_turn_and_wait(
            batch_id,
            session_id,
            thread_id,
            log_label,
            role,
            prompt,
            true,
            TurnSource::User,
        )
        .await?;
        self.wait_for_batch_quiescence(batch_id).await
    }

    fn allocate_batch_id(&self) -> u64 {
        let mut next = self.next_batch_id.lock().expect("batch id lock poisoned");
        let batch_id = *next;
        *next += 1;
        batch_id
    }

    fn enqueue_turn(
        &self,
        batch_id: u64,
        session_id: String,
        thread_id: String,
        log_label: String,
        role: SessionRole,
        prompt: &str,
        wait_for_follow_on: bool,
        source: TurnSource,
    ) -> Result<()> {
        self.enqueue_turn_with_completion(
            batch_id,
            session_id,
            thread_id,
            log_label,
            role,
            prompt,
            wait_for_follow_on,
            source,
        )?;
        Ok(())
    }

    async fn enqueue_turn_and_wait(
        &self,
        batch_id: u64,
        session_id: String,
        thread_id: String,
        log_label: String,
        role: SessionRole,
        prompt: &str,
        wait_for_follow_on: bool,
        source: TurnSource,
    ) -> Result<()> {
        let done = self.enqueue_turn_with_completion(
            batch_id,
            session_id,
            thread_id,
            log_label,
            role,
            prompt,
            wait_for_follow_on,
            source,
        )?;
        done.await.context("turn task dropped before completion")?
    }

    fn enqueue_turn_with_completion(
        &self,
        batch_id: u64,
        session_id: String,
        thread_id: String,
        log_label: String,
        role: SessionRole,
        prompt: &str,
        wait_for_follow_on: bool,
        source: TurnSource,
    ) -> Result<oneshot::Receiver<Result<()>>> {
        let (tx, rx) = oneshot::channel();
        let turn = QueuedTurn {
            batch_id,
            session_id: session_id.clone(),
            thread_id,
            log_label,
            role,
            prompt: prompt.to_owned(),
            wait_for_follow_on,
            source: source.clone(),
            completion: tx,
        };

        if self.session_is_busy(&session_id) {
            let pending_count = {
                let mut pending = self.pending_turns.lock().expect("pending lock poisoned");
                let queue = pending.entry(session_id.clone()).or_default();
                queue.push_back(turn);
                queue.len()
            };
            self.set_session_pending_turns(&session_id, pending_count)?;
            self.append_log_line(
                &session_id,
                format!(
                    "queue> {} queued ({pending_count} waiting): {}",
                    source.log_prefix(),
                    source.format_prompt(prompt)
                ),
            )?;
            self.append_session_event(
                &session_id,
                source.event_kind(),
                source.timeline_text(prompt, true, pending_count),
            )?;
            return Ok(rx);
        }

        self.start_queued_turn(turn)?;

        Ok(rx)
    }

    pub async fn spawn_worker(&self, group: &str, task: &str) -> Result<WorkerRecord> {
        self.spawn_worker_with_options(group, task, None, None, false, self.allocate_batch_id())
            .await
    }

    pub async fn spawn_worker_and_wait(&self, group: &str, task: &str) -> Result<WorkerRecord> {
        self.spawn_worker_with_options(group, task, None, None, true, self.allocate_batch_id())
            .await
    }

    pub async fn update_worker_summary(&self, worker_id: &str, summary: &str) -> Result<()> {
        let worker = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let worker = state
                .workers
                .get_mut(worker_id)
                .with_context(|| format!("unknown worker `{worker_id}`"))?;
            worker.summary = Some(summary.to_owned());
            worker.updated_at = now_unix_ts();
            worker.clone()
        };
        self.save_state()?;
        self.set_session_summary(worker_id, Some(summary.to_owned()))?;
        self.write_worker_status(&worker)?;
        self.append_log_line(worker_id, format!("system> summary updated: {summary}"))?;
        self.append_session_event(
            worker_id,
            SessionEventKind::System,
            format!("summary updated: {summary}"),
        )?;
        Ok(())
    }

    async fn spawn_worker_with_options(
        &self,
        group: &str,
        task: &str,
        summary: Option<String>,
        prompt: Option<String>,
        wait_for_bootstrap: bool,
        batch_id: u64,
    ) -> Result<WorkerRecord> {
        let group_config = self
            .config
            .group(group)
            .with_context(|| format!("unknown group `{group}`"))?
            .clone();
        self.paths.ensure_layout()?;

        let task_number = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let current = state.next_task_number;
            state.next_task_number += 1;
            current
        };
        self.save_state()?;

        let task_file_name = format!("TASK-{task_number:03}.md");
        let task_file = self.paths.task_dir.join(task_file_name);
        let worker_id = format!("{group}-{task_number:03}-{}", slug(task));
        let thread_name = format!("[{group}] {task}");

        fs::write(
            &task_file,
            render_task_file(task_number, task, &group_config.lease_paths),
        )
        .with_context(|| format!("failed to write {}", task_file.display()))?;

        let response = self
            .start_thread(
                &thread_name,
                Some(worker_instructions(group, task, &task_file)),
                Some(&thread_name),
                false,
            )
            .await?;

        let now = now_unix_ts();
        let record = WorkerRecord {
            id: worker_id.clone(),
            group: group.to_owned(),
            task: task.to_owned(),
            summary: summary.clone(),
            task_file: task_file.display().to_string(),
            thread_id: response.thread.id,
            status: WorkerStatus::Idle,
            created_at: now,
            updated_at: now,
            last_turn_id: None,
            last_message: None,
        };

        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state.workers.insert(worker_id.clone(), record.clone());
        }
        self.save_state()?;
        self.write_worker_status(&record)?;
        self.upsert_worker_session(&record);
        self.append_log_line(
            &record.id,
            format!("system> worker created from {}", record.task_file),
        )?;
        self.append_session_event(
            &record.id,
            SessionEventKind::System,
            format!("worker registered from {}", record.task_file),
        )?;
        if let Some(summary) = summary {
            self.set_session_summary(&record.id, Some(summary.clone()))?;
            self.append_log_line(&record.id, format!("system> summary: {summary}"))?;
            self.append_session_event(
                &record.id,
                SessionEventKind::System,
                format!("initial summary: {summary}"),
            )?;
        }

        let bootstrap_prompt = prompt.unwrap_or_else(|| worker_bootstrap_prompt(&record));
        if wait_for_bootstrap {
            self.enqueue_turn_and_wait(
                batch_id,
                record.id.clone(),
                record.thread_id.clone(),
                record.id.clone(),
                SessionRole::Worker(record.id.clone()),
                &bootstrap_prompt,
                false,
                TurnSource::Bootstrap,
            )
            .await?;
        } else {
            self.enqueue_turn(
                batch_id,
                record.id.clone(),
                record.thread_id.clone(),
                record.id.clone(),
                SessionRole::Worker(record.id.clone()),
                &bootstrap_prompt,
                false,
                TurnSource::Bootstrap,
            )?;
        }

        Ok(record)
    }

    async fn resolve_prompt_target(
        &self,
        target: PromptTarget,
    ) -> Result<(String, String, String, SessionRole)> {
        match target {
            PromptTarget::Master => {
                let thread_id = self.ensure_master_thread().await?;
                Ok((
                    MASTER_SESSION_ID.to_owned(),
                    thread_id,
                    MASTER_SESSION_ID.to_owned(),
                    SessionRole::Master,
                ))
            }
            PromptTarget::Worker(worker_id) => {
                let worker = self.ensure_worker_thread(&worker_id).await?;
                Ok((
                    worker.id.clone(),
                    worker.thread_id.clone(),
                    worker.id.clone(),
                    SessionRole::Worker(worker.id),
                ))
            }
        }
    }

    async fn resolve_worker_target(
        &self,
        worker_id: &str,
    ) -> Result<(String, String, String, SessionRole)> {
        let worker = self.ensure_worker_thread(worker_id).await?;
        Ok((
            worker.id.clone(),
            worker.thread_id.clone(),
            worker.id.clone(),
            SessionRole::Worker(worker.id),
        ))
    }

    async fn process_turn(
        &self,
        batch_id: u64,
        session_id: String,
        thread_id: String,
        log_label: String,
        prompt: String,
        role: SessionRole,
        wait_for_follow_on: bool,
    ) -> Result<()> {
        let mut receiver = self.client.subscribe();
        self.write_role_status(&role, "running", None, None)?;
        let model_prompt = self.prepare_prompt_for_role(&prompt, &role);

        let response: TurnStartResponse = self
            .client
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [
                        {
                            "type": "text",
                            "text": model_prompt,
                            "text_elements": [],
                        }
                    ],
                    "effort": self.config.master.reasoning_effort,
                }),
            )
            .await?;

        let turn_id = response.turn.id.clone();
        self.set_session_last_turn_id(&session_id, Some(turn_id.clone()))?;
        self.write_role_status(&role, "running", Some(turn_id.clone()), None)?;

        let mut streamed_delta = false;
        let mut assistant_text = String::new();
        let mut final_error: Option<TurnError> = None;

        loop {
            let notification = receiver.recv().await.map_err(map_broadcast_error)?;
            self.log_notification(&log_label, &notification)?;

            match notification.method.as_str() {
                "item/agentMessage/delta" => {
                    let event: AgentMessageDeltaNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn_id == turn_id {
                        assistant_text.push_str(&event.delta);
                        if matches!(role, SessionRole::Master) {
                            self.set_live_buffer(
                                &session_id,
                                visible_stream_text(&assistant_text),
                            )?;
                        } else {
                            self.append_live_chunk(&session_id, &event.delta)?;
                        }
                        streamed_delta = true;
                    }
                }
                "item/completed" => {
                    let event: ItemLifecycleNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn_id == turn_id {
                        match event.item {
                            ThreadItem::AgentMessage { text } if !streamed_delta => {
                                assistant_text.push_str(&text);
                                if matches!(role, SessionRole::Master) {
                                    let visible = visible_stream_text(&assistant_text).trim();
                                    if !visible.is_empty() {
                                        self.append_log_line(
                                            &session_id,
                                            format!("assistant> {visible}"),
                                        )?;
                                    }
                                } else {
                                    self.append_log_line(
                                        &session_id,
                                        format!("assistant> {text}"),
                                    )?;
                                }
                            }
                            ThreadItem::CommandExecution {
                                command,
                                status,
                                exit_code,
                                aggregated_output,
                            } => {
                                self.append_log_line(
                                    &session_id,
                                    format!("command> [{status}] {:?} :: {}", exit_code, command),
                                )?;
                                self.append_session_event(
                                    &session_id,
                                    SessionEventKind::Command,
                                    format!("[{status}] {command}"),
                                )?;
                                if let Some(output) = aggregated_output {
                                    let trimmed = output.trim();
                                    if !trimmed.is_empty() {
                                        self.append_log_line(
                                            &session_id,
                                            format!("output> {trimmed}"),
                                        )?;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "thread/status/changed" => {
                    let event: ThreadStatusChangedNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id {
                        let status = thread_state_text(&event.status);
                        self.set_session_status(&session_id, &status)?;
                    }
                }
                "turn/started" => {
                    let event: TurnStartedNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn.id == turn_id {
                        self.set_session_status(&session_id, &event.turn.status)?;
                    }
                }
                "error" => {
                    let event: ErrorNotification = serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn_id == turn_id {
                        self.append_log_line(
                            &session_id,
                            format!("error> {}", event.error.message),
                        )?;
                        self.append_session_event(
                            &session_id,
                            SessionEventKind::Error,
                            event.error.message.clone(),
                        )?;
                        if let Some(details) = &event.error.additional_details {
                            self.append_log_line(&session_id, format!("error> {details}"))?;
                        }
                        if !event.will_retry {
                            final_error = Some(event.error);
                        }
                    }
                }
                "turn/completed" => {
                    let event: TurnCompletedNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn.id == turn_id {
                        if streamed_delta {
                            let _ = self.commit_live_buffer(&session_id)?;
                        }

                        if let Some(error) = event.turn.error.or(final_error) {
                            self.append_session_event(
                                &session_id,
                                SessionEventKind::Error,
                                format!("turn failed: {}", error.message),
                            )?;
                            self.write_role_status(
                                &role,
                                "failed",
                                Some(turn_id.clone()),
                                Some(error.message.clone()),
                            )?;
                            if let SessionRole::Worker(worker_id) = &role {
                                self.publish_worker_runtime_update(worker_id, "failed", batch_id)
                                    .await?;
                            }
                            return Err(anyhow!(error.message));
                        }

                        let parsed_master =
                            if matches!(role, SessionRole::Master) {
                                Some(parse_master_response(&assistant_text).with_context(|| {
                                    "failed to decode master orchestration block"
                                })?)
                            } else {
                                None
                            };

                        let mut last_message = if assistant_text.trim().is_empty() {
                            None
                        } else {
                            Some(compact_message(&assistant_text))
                        };

                        if let Some(parsed) = parsed_master {
                            if let Some(visible) = self
                                .apply_master_response(
                                    &session_id,
                                    &parsed,
                                    wait_for_follow_on,
                                    batch_id,
                                )
                                .await?
                            {
                                last_message = Some(compact_message(&visible));
                            }
                        }

                        self.write_role_status(
                            &role,
                            "completed",
                            Some(turn_id.clone()),
                            last_message,
                        )?;
                        if let SessionRole::Worker(worker_id) = &role {
                            self.publish_worker_runtime_update(worker_id, "completed", batch_id)
                                .await?;
                        }
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }

    async fn start_thread(
        &self,
        service_name: &str,
        developer_instructions: Option<String>,
        thread_name: Option<&str>,
        ephemeral: bool,
    ) -> Result<ThreadStartResponse> {
        let response: ThreadStartResponse = self
            .client
            .request(
                "thread/start",
                json!({
                    "cwd": self.workspace_root,
                    "sandbox": self.config.master.sandbox,
                    "approvalPolicy": self.config.master.approval,
                    "personality": "pragmatic",
                    "serviceName": service_name,
                    "developerInstructions": developer_instructions,
                    "model": self.config.master.model,
                    "ephemeral": ephemeral,
                }),
            )
            .await?;

        if let Some(name) = thread_name {
            let thread_id = response.thread.id.clone();
            let _: Value = self
                .client
                .request(
                    "thread/name/set",
                    json!({
                        "threadId": thread_id,
                        "name": name,
                    }),
                )
                .await?;
        }

        Ok(response)
    }

    async fn ensure_worker_thread(&self, worker_id: &str) -> Result<WorkerRecord> {
        let worker = {
            let state = self.state.lock().expect("state lock poisoned");
            state
                .workers
                .get(worker_id)
                .cloned()
                .with_context(|| format!("unknown worker `{worker_id}`"))?
        };

        let thread_name = format!("[{}] {}", worker.group, worker.task);
        let instructions =
            worker_instructions(&worker.group, &worker.task, Path::new(&worker.task_file));
        if self
            .resume_thread(&worker.thread_id, Some(instructions.clone()))
            .await
            .is_ok()
        {
            return Ok(worker);
        }

        let response = self
            .start_thread(&thread_name, Some(instructions), Some(&thread_name), false)
            .await?;

        let updated_worker = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let stored = state
                .workers
                .get_mut(worker_id)
                .with_context(|| format!("unknown worker `{worker_id}`"))?;
            stored.thread_id = response.thread.id.clone();
            stored.updated_at = now_unix_ts();
            stored.clone()
        };

        self.save_state()?;
        self.write_worker_status(&updated_worker)?;
        self.upsert_worker_session(&updated_worker);
        self.append_log_line(
            worker_id,
            format!(
                "system> resumed with fresh thread {}",
                updated_worker.thread_id
            ),
        )?;
        self.append_session_event(
            worker_id,
            SessionEventKind::System,
            format!("resumed with fresh thread {}", updated_worker.thread_id),
        )?;
        Ok(updated_worker)
    }

    async fn resume_thread(
        &self,
        thread_id: &str,
        developer_instructions: Option<String>,
    ) -> Result<ThreadStartResponse> {
        self.client
            .request(
                "thread/resume",
                json!({
                    "threadId": thread_id,
                    "cwd": self.workspace_root,
                    "sandbox": self.config.master.sandbox,
                    "approvalPolicy": self.config.master.approval,
                    "personality": "pragmatic",
                    "model": self.config.master.model,
                    "developerInstructions": developer_instructions,
                    "persistExtendedHistory": false,
                }),
            )
            .await
    }

    fn ensure_master_session(&self, thread_id: String) {
        let summary = self.master_summary();
        let last_message = self.master_last_message();
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        sessions
            .entry(MASTER_SESSION_ID.to_owned())
            .and_modify(|session| {
                session.set_thread_id(thread_id.clone());
                session.set_summary(summary.clone());
                session.set_last_message(last_message.clone());
            })
            .or_insert_with(|| {
                SessionView::master(
                    thread_id,
                    self.workspace_root.display().to_string(),
                    summary,
                    last_message,
                )
            });
    }

    fn upsert_worker_session(&self, worker: &WorkerRecord) {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        sessions.insert(
            worker.id.clone(),
            SessionView::from_worker(worker, self.workspace_root.display().to_string()),
        );
    }

    fn start_queued_turn(&self, turn: QueuedTurn) -> Result<()> {
        self.append_log_line(
            &turn.session_id,
            format!(
                "{}> {}",
                turn.source.log_prefix(),
                turn.source.format_prompt(&turn.prompt)
            ),
        )?;
        self.append_session_event(
            &turn.session_id,
            turn.source.event_kind(),
            turn.source.timeline_text(&turn.prompt, false, 0),
        )?;
        self.set_session_status(&turn.session_id, "queued")?;
        self.set_active_turn_batch(&turn.session_id, turn.batch_id)?;

        let controller = self.clone();
        tokio::spawn(async move {
            let result = controller
                .process_turn(
                    turn.batch_id,
                    turn.session_id.clone(),
                    turn.thread_id,
                    turn.log_label,
                    turn.prompt,
                    turn.role,
                    turn.wait_for_follow_on,
                )
                .await;
            if let Err(error) = &result {
                let _ = controller.append_log_line(&turn.session_id, format!("error> {error}"));
                let _ = controller.set_session_status(&turn.session_id, "failed");
            }
            let _ = turn.completion.send(result);
            let _ = controller.clear_active_turn_batch(&turn.session_id, turn.batch_id);
            let _ = controller.schedule_next_turn(&turn.session_id);
        });

        Ok(())
    }

    fn schedule_next_turn(&self, session_id: &str) -> Result<()> {
        let (next_turn, pending_count) = {
            let mut pending = self.pending_turns.lock().expect("pending lock poisoned");
            let next_turn = if let Some(queue) = pending.get_mut(session_id) {
                let next = queue.pop_front();
                let remaining = queue.len();
                let should_remove = remaining == 0;
                (next, remaining, should_remove)
            } else {
                (None, 0, false)
            };
            if next_turn.2 {
                pending.remove(session_id);
            }
            (next_turn.0, next_turn.1)
        };
        self.set_session_pending_turns(session_id, pending_count)?;
        if let Some(turn) = next_turn {
            self.start_queued_turn(turn)?;
        }
        Ok(())
    }

    fn session_is_busy(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock().expect("sessions lock poisoned");
        sessions
            .get(session_id)
            .map(|session| {
                matches!(
                    session.snapshot().status.as_str(),
                    "queued" | "running" | "active" | "inProgress"
                )
            })
            .unwrap_or(false)
    }

    fn set_session_pending_turns(&self, session_id: &str, pending_turns: usize) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_pending_turns(pending_turns);
        }
        Ok(())
    }

    fn set_active_turn_batch(&self, session_id: &str, batch_id: u64) -> Result<()> {
        let mut active = self
            .active_turn_batches
            .lock()
            .expect("active turn lock poisoned");
        active.insert(session_id.to_owned(), batch_id);
        Ok(())
    }

    fn clear_active_turn_batch(&self, session_id: &str, batch_id: u64) -> Result<()> {
        let mut active = self
            .active_turn_batches
            .lock()
            .expect("active turn lock poisoned");
        if active.get(session_id).copied() == Some(batch_id) {
            active.remove(session_id);
        }
        Ok(())
    }

    fn batch_has_pending_turns(&self, batch_id: u64) -> bool {
        let pending = self.pending_turns.lock().expect("pending lock poisoned");
        pending
            .values()
            .any(|queue| queue.iter().any(|turn| turn.batch_id == batch_id))
    }

    fn batch_has_active_turns(&self, batch_id: u64) -> bool {
        let active = self
            .active_turn_batches
            .lock()
            .expect("active turn lock poisoned");
        active
            .values()
            .any(|current_batch| *current_batch == batch_id)
    }

    async fn wait_for_batch_quiescence(&self, batch_id: u64) -> Result<()> {
        loop {
            if !self.batch_has_active_turns(batch_id) && !self.batch_has_pending_turns(batch_id) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    }

    fn append_log_line(&self, session_id: &str, line: impl Into<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.push_line(line);
        }
        Ok(())
    }

    fn append_session_event(
        &self,
        session_id: &str,
        kind: SessionEventKind,
        text: impl Into<String>,
    ) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.push_timeline_event(kind, text);
        }
        Ok(())
    }

    fn append_live_chunk(&self, session_id: &str, chunk: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.append_live_chunk(chunk);
        }
        Ok(())
    }

    fn set_live_buffer(&self, session_id: &str, content: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_live_buffer(content);
        }
        Ok(())
    }

    fn commit_live_buffer(&self, session_id: &str) -> Result<Option<String>> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        Ok(sessions
            .get_mut(session_id)
            .and_then(SessionView::commit_live_buffer))
    }

    fn replace_last_assistant_line(&self, session_id: &str, text: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.replace_last_assistant_line(text);
        }
        Ok(())
    }

    fn set_session_status(&self, session_id: &str, status: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            if session.set_status(status.to_owned()) {
                session.push_timeline_event(SessionEventKind::Status, format!("state -> {status}"));
            }
        }
        Ok(())
    }

    fn set_session_last_turn_id(&self, session_id: &str, turn_id: Option<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_last_turn_id(turn_id);
        }
        Ok(())
    }

    fn set_session_last_message(&self, session_id: &str, message: Option<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_last_message(message);
        }
        Ok(())
    }

    fn set_session_summary(&self, session_id: &str, summary: Option<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_summary(summary);
        }
        Ok(())
    }

    fn update_master_summary(&self, summary: &str) -> Result<()> {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state.master_summary = Some(summary.to_owned());
        }
        self.save_state()?;
        self.set_session_summary(MASTER_SESSION_ID, Some(summary.to_owned()))?;
        Ok(())
    }

    fn master_thread_id(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.master_thread_id.clone()
    }

    fn master_last_turn_id(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.master_last_turn_id.clone()
    }

    fn master_summary(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.master_summary.clone()
    }

    fn master_last_message(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.master_last_message.clone()
    }

    fn save_state(&self) -> Result<()> {
        let state = self.state.lock().expect("state lock poisoned");
        state.save(&self.paths.state_file)
    }

    fn log_notification(&self, log_label: &str, notification: &Notification) -> Result<()> {
        let log_path = self.paths.log_dir.join(format!("{log_label}.jsonl"));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("failed to open {}", log_path.display()))?;
        let line = serde_json::to_string(&json!({
            "ts": now_unix_ts(),
            "method": notification.method,
            "params": notification.params,
        }))
        .context("failed to encode log entry")?;
        writeln!(file, "{line}").with_context(|| format!("failed to append {}", log_path.display()))
    }

    fn write_role_status(
        &self,
        role: &SessionRole,
        state: &str,
        last_turn_id: Option<String>,
        last_message: Option<String>,
    ) -> Result<()> {
        match role {
            SessionRole::Master => {
                {
                    let mut persisted = self.state.lock().expect("state lock poisoned");
                    if let Some(turn_id) = &last_turn_id {
                        persisted.master_last_turn_id = Some(turn_id.clone());
                    }
                    if last_message.is_some() {
                        persisted.master_last_message = last_message.clone();
                    }
                }
                self.save_state()?;
                self.set_session_status(MASTER_SESSION_ID, state)?;
                self.set_session_last_turn_id(MASTER_SESSION_ID, last_turn_id.clone())?;
                self.set_session_last_message(MASTER_SESSION_ID, last_message.clone())?;
                self.write_master_status(state, last_turn_id, last_message)
            }
            SessionRole::Worker(worker_id) => {
                let worker = {
                    let mut persisted = self.state.lock().expect("state lock poisoned");
                    let worker = persisted
                        .workers
                        .get_mut(worker_id)
                        .with_context(|| format!("unknown worker `{worker_id}`"))?;
                    worker.status = worker_status_for(state);
                    worker.updated_at = now_unix_ts();
                    if last_turn_id.is_some() {
                        worker.last_turn_id = last_turn_id.clone();
                    }
                    if last_message.is_some() {
                        worker.last_message = last_message.clone();
                    }
                    worker.clone()
                };
                self.save_state()?;
                self.set_session_status(worker_id, state)?;
                self.set_session_last_turn_id(worker_id, last_turn_id)?;
                self.set_session_last_message(worker_id, last_message)?;
                self.set_session_summary(worker_id, worker.summary.clone())?;
                self.write_worker_status(&worker)
            }
        }
    }

    fn write_master_status(
        &self,
        state: &str,
        last_turn_id: Option<String>,
        last_message: Option<String>,
    ) -> Result<()> {
        let thread_id = self.master_thread_id().unwrap_or_default();
        let status = SessionStatus {
            role: "master".to_owned(),
            thread_id,
            state: state.to_owned(),
            updated_at: now_unix_ts(),
            summary: self
                .master_summary()
                .or_else(|| Some("Primary planner and dispatcher".to_owned())),
            last_turn_id,
            last_message,
        };
        status.write(&self.paths.status_dir.join("master.json"))
    }

    fn write_worker_status(&self, worker: &WorkerRecord) -> Result<()> {
        let status = SessionStatus {
            role: format!("worker:{}", worker.group),
            thread_id: worker.thread_id.clone(),
            state: worker.status.to_string(),
            updated_at: worker.updated_at,
            summary: worker.summary.clone(),
            last_turn_id: worker.last_turn_id.clone(),
            last_message: worker.last_message.clone(),
        };
        status.write(&worker.status_path(&self.paths.status_dir))
    }

    async fn publish_worker_runtime_update(
        &self,
        worker_id: &str,
        worker_state: &str,
        batch_id: u64,
    ) -> Result<()> {
        let worker = {
            let state = self.state.lock().expect("state lock poisoned");
            state
                .workers
                .get(worker_id)
                .cloned()
                .with_context(|| format!("unknown worker `{worker_id}`"))?
        };

        let master_thread_id = self.ensure_master_thread().await?;
        self.append_session_event(
            MASTER_SESSION_ID,
            SessionEventKind::Runtime,
            format!("worker {worker_id} reported {worker_state}"),
        )?;
        let prompt = format!(
            "CodeClaw runtime update. This is an internal worker status event, not a direct human message.\n\nWorker id: {worker_id}\nState: {worker_state}\nGroup: {}\nTask: {}\nSidebar summary: {}\nLast worker message: {}\n\nUpdate the operator with a concise coordination response and include the required <codeclaw-actions> block. If no follow-up is needed, return an empty actions list.",
            worker.group,
            worker.task,
            worker
                .summary
                .clone()
                .unwrap_or_else(|| "not set".to_owned()),
            worker
                .last_message
                .clone()
                .unwrap_or_else(|| "none".to_owned())
        );

        self.enqueue_turn(
            batch_id,
            MASTER_SESSION_ID.to_owned(),
            master_thread_id,
            MASTER_SESSION_ID.to_owned(),
            SessionRole::Master,
            &prompt,
            false,
            TurnSource::Runtime,
        )
    }

    fn master_instructions(&self) -> String {
        format!(
            "You are the master controller for CodeClaw in {}. Coordinate work across workers, keep plans concise, and prefer actionable task splits.\n\nYou may receive direct human prompts and internal runtime updates about worker completions or failures. Treat runtime updates as scheduler inputs: absorb the worker result, update summaries when useful, and dispatch follow-up actions only when they are actually needed.\n\nWhen you respond, append exactly one machine-readable block at the end using this format:\n<codeclaw-actions>\n{{\"summary\":\"short orchestration summary\",\"actions\":[...]}}\n</codeclaw-actions>\n\nAllowed actions:\n- {{\"type\":\"spawn_worker\",\"group\":\"backend|frontend|infra\",\"task\":\"short task title\",\"summary\":\"optional short sidebar summary\",\"prompt\":\"optional initial worker prompt\"}}\n- {{\"type\":\"send_worker_prompt\",\"worker_id\":\"existing-worker-id\",\"prompt\":\"follow-up instructions\"}}\n- {{\"type\":\"update_worker_summary\",\"worker_id\":\"existing-worker-id\",\"summary\":\"new short summary\"}}\n\nRules:\n- Always include the block, even when no actions are needed.\n- Keep `summary` short enough to fit a sidebar.\n- Use worker ids exactly as shown in the UI when referencing existing workers.",
            self.workspace_root.display()
        )
    }

    fn prepare_prompt_for_role(&self, prompt: &str, role: &SessionRole) -> String {
        match role {
            SessionRole::Master => format!(
                "{prompt}\n\nCodeClaw runtime reminder: finish with the required <codeclaw-actions> JSON block."
            ),
            SessionRole::Worker(_) => prompt.to_owned(),
        }
    }

    async fn apply_master_response(
        &self,
        session_id: &str,
        parsed: &ParsedMasterResponse,
        wait_for_follow_on: bool,
        batch_id: u64,
    ) -> Result<Option<String>> {
        let visible = parsed.visible_response.trim();
        if !visible.is_empty() {
            self.replace_last_assistant_line(session_id, visible)?;
        }

        if let Some(summary) = &parsed.envelope.summary {
            self.update_master_summary(summary)?;
            self.append_log_line(session_id, format!("orchestrator> summary: {summary}"))?;
            self.append_session_event(
                session_id,
                SessionEventKind::Orchestrator,
                format!("master summary -> {summary}"),
            )?;
        }

        for action in &parsed.envelope.actions {
            match action {
                MasterAction::SpawnWorker {
                    group,
                    task,
                    summary,
                    prompt,
                } => {
                    self.append_log_line(
                        session_id,
                        format!("orchestrator> spawn_worker group={group} task={task}"),
                    )?;
                    self.append_session_event(
                        session_id,
                        SessionEventKind::Orchestrator,
                        format!("spawn worker [{group}] {task}"),
                    )?;
                    match self
                        .spawn_worker_with_options(
                            group,
                            task,
                            summary.clone(),
                            prompt.clone(),
                            wait_for_follow_on,
                            batch_id,
                        )
                        .await
                    {
                        Ok(worker) => {
                            self.append_log_line(
                                session_id,
                                format!(
                                    "orchestrator> worker created: {} ({})",
                                    worker.id, worker.thread_id
                                ),
                            )?;
                            self.append_session_event(
                                session_id,
                                SessionEventKind::Orchestrator,
                                format!("worker ready: {}", worker.id),
                            )?;
                        }
                        Err(error) => {
                            self.append_log_line(
                                session_id,
                                format!("orchestrator> spawn_worker failed: {error}"),
                            )?;
                            self.append_session_event(
                                session_id,
                                SessionEventKind::Error,
                                format!("spawn worker failed: {error}"),
                            )?;
                        }
                    }
                }
                MasterAction::SendWorkerPrompt { worker_id, prompt } => {
                    self.append_log_line(
                        session_id,
                        format!("orchestrator> send_worker_prompt worker={worker_id}"),
                    )?;
                    self.append_session_event(
                        session_id,
                        SessionEventKind::Orchestrator,
                        format!("dispatch follow-up to {worker_id}"),
                    )?;
                    match self.resolve_worker_target(worker_id).await {
                        Ok((target_session_id, thread_id, log_label, role)) => {
                            let result = if wait_for_follow_on {
                                self.enqueue_turn_and_wait(
                                    batch_id,
                                    target_session_id,
                                    thread_id,
                                    log_label,
                                    role,
                                    prompt,
                                    false,
                                    TurnSource::Orchestrator,
                                )
                                .await
                            } else {
                                self.enqueue_turn(
                                    batch_id,
                                    target_session_id,
                                    thread_id,
                                    log_label,
                                    role,
                                    prompt,
                                    false,
                                    TurnSource::Orchestrator,
                                )
                            };
                            if let Err(error) = result {
                                self.append_log_line(
                                    session_id,
                                    format!("orchestrator> send_worker_prompt failed: {error}"),
                                )?;
                                self.append_session_event(
                                    session_id,
                                    SessionEventKind::Error,
                                    format!("follow-up dispatch failed: {error}"),
                                )?;
                            }
                        }
                        Err(error) => {
                            self.append_log_line(
                                session_id,
                                format!("orchestrator> send_worker_prompt failed: {error}"),
                            )?;
                            self.append_session_event(
                                session_id,
                                SessionEventKind::Error,
                                format!("follow-up dispatch failed: {error}"),
                            )?;
                        }
                    }
                }
                MasterAction::UpdateWorkerSummary { worker_id, summary } => {
                    self.append_log_line(
                        session_id,
                        format!("orchestrator> update_worker_summary worker={worker_id}"),
                    )?;
                    self.append_session_event(
                        session_id,
                        SessionEventKind::Orchestrator,
                        format!("update summary for {worker_id}"),
                    )?;
                    if let Err(error) = self.update_worker_summary(worker_id, summary).await {
                        self.append_log_line(
                            session_id,
                            format!("orchestrator> update_worker_summary failed: {error}"),
                        )?;
                        self.append_session_event(
                            session_id,
                            SessionEventKind::Error,
                            format!("summary update failed: {error}"),
                        )?;
                    }
                }
            }
        }

        Ok(if visible.is_empty() {
            parsed.envelope.summary.clone()
        } else {
            Some(visible.to_owned())
        })
    }
}

fn build_sessions(state: &AppState, workspace_root: &Path) -> BTreeMap<String, SessionView> {
    let mut sessions = BTreeMap::new();
    if let Some(thread_id) = state.master_thread_id.clone() {
        sessions.insert(
            MASTER_SESSION_ID.to_owned(),
            SessionView::master(
                thread_id,
                workspace_root.display().to_string(),
                state.master_summary.clone(),
                state.master_last_message.clone(),
            ),
        );
    }
    for worker in state.workers.values() {
        sessions.insert(
            worker.id.clone(),
            SessionView::from_worker(worker, workspace_root.display().to_string()),
        );
    }
    sessions
}

fn session_sort_key(id: &str) -> (u8, &str) {
    if id == MASTER_SESSION_ID {
        (0, id)
    } else {
        (1, id)
    }
}

fn thread_state_text(value: &Value) -> String {
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        kind.to_owned()
    } else {
        value.to_string()
    }
}

fn worker_status_for(state: &str) -> WorkerStatus {
    match state {
        "completed" => WorkerStatus::Completed,
        "failed" => WorkerStatus::Failed,
        "running" | "queued" | "active" | "inProgress" => WorkerStatus::Running,
        _ => WorkerStatus::Idle,
    }
}

fn render_task_file(task_number: u64, task: &str, lease_paths: &[String]) -> String {
    let lease_section = if lease_paths.is_empty() {
        "- (not specified)\n".to_owned()
    } else {
        lease_paths
            .iter()
            .map(|path| format!("- {path}\n"))
            .collect::<String>()
    };

    format!(
        "# TASK-{task_number:03}\n\n## Goal\n\n{task}\n\n## Acceptance Criteria\n\n- Make concrete progress on the assigned task.\n- Keep changes scoped to the leased area.\n- Report blockers explicitly.\n\n## Leased Paths\n\n{lease_section}"
    )
}

fn worker_instructions(group: &str, task: &str, task_file: &Path) -> String {
    format!(
        "You are the `{group}` worker for CodeClaw. Your current task is: {task}. Read the task file at {} before making changes. Stay focused on the assigned scope and report blockers clearly.",
        task_file.display()
    )
}

fn worker_bootstrap_prompt(worker: &WorkerRecord) -> String {
    format!(
        "Read {} and start executing the task. Work inside the current repository, make changes directly when justified, and summarize what you changed or what blocked you.",
        worker.task_file
    )
}

fn slug(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in input.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
        if slug.len() >= 24 {
            break;
        }
    }
    slug.trim_matches('-').to_owned()
}

fn compact_message(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= 200 {
        trimmed.to_owned()
    } else {
        format!("{}...", &trimmed[..200])
    }
}

fn map_broadcast_error(error: broadcast::error::RecvError) -> anyhow::Error {
    anyhow!("app-server notification channel error: {error}")
}
