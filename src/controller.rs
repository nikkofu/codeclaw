use crate::{
    app_server::{AppServerClient, Notification},
    config::{Config, CoordinationPaths, GroupConfig},
    orchestration::{parse_master_response, MasterAction, ParsedMasterResponse},
    session::{SessionSnapshot, SessionView},
    state::{now_unix_ts, AppState, SessionStatus, WorkerRecord, WorkerStatus},
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

const MASTER_SESSION_ID: &str = "master";

#[derive(Clone)]
pub struct Controller {
    workspace_root: PathBuf,
    pub config: Config,
    pub paths: CoordinationPaths,
    state: Arc<Mutex<AppState>>,
    sessions: Arc<Mutex<BTreeMap<String, SessionView>>>,
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
struct ThreadReadResponse {
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

impl Controller {
    pub async fn start(workspace_root: PathBuf) -> Result<Self> {
        let config = Config::load(&workspace_root)?;
        let paths = config.coordination_paths(&workspace_root);
        let state = AppState::load(&paths.state_file)?;
        let sessions = build_sessions(&state, &workspace_root);
        let client = Arc::new(
            AppServerClient::spawn(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await?,
        );

        Ok(Self {
            workspace_root,
            config,
            paths,
            state: Arc::new(Mutex::new(state)),
            sessions: Arc::new(Mutex::new(sessions)),
            client,
        })
    }

    pub fn init_workspace(&self) -> Result<Option<PathBuf>> {
        let config_path = Config::write_default_config_if_missing(&self.workspace_root)?;
        self.paths.ensure_layout()?;
        self.save_state()?;
        self.write_master_status("idle", None, None)?;
        Ok(config_path)
    }

    pub async fn doctor(&self) -> Result<DoctorReport> {
        let client =
            AppServerClient::spawn(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await?;
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
            if self.thread_exists(&thread_id).await {
                self.ensure_master_session(thread_id.clone());
                self.write_master_status("idle", self.master_last_turn_id(), None)?;
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
        }
        self.save_state()?;
        self.ensure_master_session(thread_id.clone());
        self.write_master_status("idle", None, None)?;
        Ok(thread_id)
    }

    pub async fn submit_prompt(&self, target: PromptTarget, prompt: &str) -> Result<()> {
        let (session_id, thread_id, log_label, role) = self.resolve_prompt_target(target).await?;
        self.enqueue_turn(session_id, thread_id, log_label, role, prompt)
    }

    fn enqueue_turn(
        &self,
        session_id: String,
        thread_id: String,
        log_label: String,
        role: SessionRole,
        prompt: &str,
    ) -> Result<()> {
        if self.session_is_busy(&session_id) {
            return Err(anyhow!("session `{session_id}` is already running"));
        }

        self.append_log_line(&session_id, format!("user> {prompt}"))?;
        self.set_session_status(&session_id, "queued")?;

        let controller = self.clone();
        let prompt = prompt.to_owned();
        tokio::spawn(async move {
            if let Err(error) = controller
                .process_turn(session_id.clone(), thread_id, log_label, prompt, role)
                .await
            {
                let _ = controller.append_log_line(&session_id, format!("error> {error}"));
                let _ = controller.set_session_status(&session_id, "failed");
            }
        });

        Ok(())
    }

    pub async fn spawn_worker(&self, group: &str, task: &str) -> Result<WorkerRecord> {
        self.spawn_worker_with_options(group, task, None, None)
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
        Ok(())
    }

    async fn spawn_worker_with_options(
        &self,
        group: &str,
        task: &str,
        summary: Option<String>,
        prompt: Option<String>,
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
        if let Some(summary) = summary {
            self.set_session_summary(&record.id, Some(summary.clone()))?;
            self.append_log_line(&record.id, format!("system> summary: {summary}"))?;
        }

        self.enqueue_turn(
            record.id.clone(),
            record.thread_id.clone(),
            record.id.clone(),
            SessionRole::Worker(record.id.clone()),
            &prompt.unwrap_or_else(|| worker_bootstrap_prompt(&record)),
        )?;

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
                let state = self.state.lock().expect("state lock poisoned");
                let worker = state
                    .workers
                    .get(&worker_id)
                    .cloned()
                    .with_context(|| format!("unknown worker `{worker_id}`"))?;
                Ok((
                    worker.id.clone(),
                    worker.thread_id.clone(),
                    worker.id.clone(),
                    SessionRole::Worker(worker.id),
                ))
            }
        }
    }

    fn resolve_worker_target(
        &self,
        worker_id: &str,
    ) -> Result<(String, String, String, SessionRole)> {
        let state = self.state.lock().expect("state lock poisoned");
        let worker = state
            .workers
            .get(worker_id)
            .cloned()
            .with_context(|| format!("unknown worker `{worker_id}`"))?;
        Ok((
            worker.id.clone(),
            worker.thread_id.clone(),
            worker.id.clone(),
            SessionRole::Worker(worker.id),
        ))
    }

    async fn process_turn(
        &self,
        session_id: String,
        thread_id: String,
        log_label: String,
        prompt: String,
        role: SessionRole,
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
                    ]
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
                        self.append_live_chunk(&session_id, &event.delta)?;
                        assistant_text.push_str(&event.delta);
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
                                self.append_log_line(&session_id, format!("assistant> {text}"))?;
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
                            self.write_role_status(
                                &role,
                                "failed",
                                Some(turn_id.clone()),
                                Some(error.message.clone()),
                            )?;
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
                            if let Some(visible) =
                                self.apply_master_response(&session_id, &parsed).await?
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

    async fn thread_exists(&self, thread_id: &str) -> bool {
        let response = self
            .client
            .request::<ThreadReadResponse>(
                "thread/read",
                json!({
                    "threadId": thread_id,
                    "includeTurns": false,
                }),
            )
            .await;

        match response {
            Ok(thread) => thread.thread.id == thread_id,
            Err(_) => false,
        }
    }

    fn ensure_master_session(&self, thread_id: String) {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        sessions
            .entry(MASTER_SESSION_ID.to_owned())
            .and_modify(|session| session.set_thread_id(thread_id.clone()))
            .or_insert_with(|| {
                SessionView::master(thread_id, self.workspace_root.display().to_string())
            });
    }

    fn upsert_worker_session(&self, worker: &WorkerRecord) {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        sessions.insert(
            worker.id.clone(),
            SessionView::from_worker(worker, self.workspace_root.display().to_string()),
        );
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

    fn append_log_line(&self, session_id: &str, line: impl Into<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.push_line(line);
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
            session.set_status(status.to_owned());
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

    fn master_thread_id(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.master_thread_id.clone()
    }

    fn master_last_turn_id(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.master_last_turn_id.clone()
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
                if let Some(turn_id) = &last_turn_id {
                    let mut persisted = self.state.lock().expect("state lock poisoned");
                    persisted.master_last_turn_id = Some(turn_id.clone());
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
            summary: Some("Primary planner and dispatcher".to_owned()),
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

    fn master_instructions(&self) -> String {
        format!(
            "You are the master controller for CodeClaw in {}. Coordinate work across workers, keep plans concise, and prefer actionable task splits.\n\nWhen you respond to the user, append exactly one machine-readable block at the end using this format:\n<codeclaw-actions>\n{{\"summary\":\"short orchestration summary\",\"actions\":[...]}}\n</codeclaw-actions>\n\nAllowed actions:\n- {{\"type\":\"spawn_worker\",\"group\":\"backend|frontend|infra\",\"task\":\"short task title\",\"summary\":\"optional short sidebar summary\",\"prompt\":\"optional initial worker prompt\"}}\n- {{\"type\":\"send_worker_prompt\",\"worker_id\":\"existing-worker-id\",\"prompt\":\"follow-up instructions\"}}\n- {{\"type\":\"update_worker_summary\",\"worker_id\":\"existing-worker-id\",\"summary\":\"new short summary\"}}\n\nRules:\n- Always include the block, even when no actions are needed.\n- Keep `summary` short enough to fit a sidebar.\n- Use worker ids exactly as shown in the UI when referencing existing workers.",
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
    ) -> Result<Option<String>> {
        let visible = parsed.visible_response.trim();
        if !visible.is_empty() {
            self.replace_last_assistant_line(session_id, visible)?;
        }

        if let Some(summary) = &parsed.envelope.summary {
            self.append_log_line(session_id, format!("orchestrator> summary: {summary}"))?;
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
                    match self
                        .spawn_worker_with_options(group, task, summary.clone(), prompt.clone())
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
                        }
                        Err(error) => {
                            self.append_log_line(
                                session_id,
                                format!("orchestrator> spawn_worker failed: {error}"),
                            )?;
                        }
                    }
                }
                MasterAction::SendWorkerPrompt { worker_id, prompt } => {
                    self.append_log_line(
                        session_id,
                        format!("orchestrator> send_worker_prompt worker={worker_id}"),
                    )?;
                    match self.resolve_worker_target(worker_id) {
                        Ok((target_session_id, thread_id, log_label, role)) => {
                            if let Err(error) = self.enqueue_turn(
                                target_session_id,
                                thread_id,
                                log_label,
                                role,
                                prompt,
                            ) {
                                self.append_log_line(
                                    session_id,
                                    format!("orchestrator> send_worker_prompt failed: {error}"),
                                )?;
                            }
                        }
                        Err(error) => {
                            self.append_log_line(
                                session_id,
                                format!("orchestrator> send_worker_prompt failed: {error}"),
                            )?;
                        }
                    }
                }
                MasterAction::UpdateWorkerSummary { worker_id, summary } => {
                    self.append_log_line(
                        session_id,
                        format!("orchestrator> update_worker_summary worker={worker_id}"),
                    )?;
                    if let Err(error) = self.update_worker_summary(worker_id, summary).await {
                        self.append_log_line(
                            session_id,
                            format!("orchestrator> update_worker_summary failed: {error}"),
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
            SessionView::master(thread_id, workspace_root.display().to_string()),
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
