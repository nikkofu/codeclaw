use crate::{
    app_server::{AppServerClient, Notification},
    config::{Config, CoordinationPaths, GroupConfig},
    orchestration::{
        parse_master_response, visible_stream_text, MasterAction, ParsedMasterResponse,
    },
    session::{
        SessionEvent, SessionEventKind, SessionSnapshot, SessionView, MAX_LOG_LINES,
        MAX_TIMELINE_EVENTS,
    },
    state::{
        now_unix_ts, AppState, BatchStatus, JobPolicy, JobRecord, JobStatus,
        OrchestrationBatchRecord, SessionStatus, WorkerRecord, WorkerStatus,
    },
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
    client: Arc<AppServerClient>,
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub config_source: String,
    pub coordination_root: PathBuf,
    pub codex_app_server_ok: bool,
    pub thread_start_ok: bool,
}

#[derive(Debug, Clone)]
pub struct BatchSessionSnapshot {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct BatchEventSnapshot {
    pub session_title: String,
    pub kind: SessionEventKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct BatchSnapshot {
    pub id: u64,
    pub job_id: Option<String>,
    pub root_session_id: String,
    pub root_session_title: String,
    pub root_prompt: String,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub sessions: Vec<BatchSessionSnapshot>,
    pub last_event: Option<String>,
    pub events: Vec<BatchEventSnapshot>,
}

#[derive(Debug, Clone)]
pub struct CreateJobRequest {
    pub title: String,
    pub objective: String,
    pub source_channel: String,
    pub requester: Option<String>,
    pub priority: String,
    pub pattern: String,
    pub approval_required: bool,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JobWorkerSnapshot {
    pub id: String,
    pub group: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JobBatchSnapshot {
    pub id: u64,
    pub status: String,
    pub root_session_id: String,
    pub root_prompt: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: String,
    pub status: String,
    pub title: String,
    pub objective: String,
    pub source_channel: String,
    pub requester: Option<String>,
    pub priority: String,
    pub pattern: String,
    pub approval_required: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub latest_summary: Option<String>,
    pub escalation_state: Option<String>,
    pub final_outcome: Option<String>,
    pub context: Option<String>,
    pub batch_ids: Vec<JobBatchSnapshot>,
    pub workers: Vec<JobWorkerSnapshot>,
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

    fn runtime_label(&self) -> &'static str {
        match self {
            Self::User => "user_prompt",
            Self::Bootstrap => "bootstrap",
            Self::Orchestrator => "orchestrator_follow_up",
            Self::Runtime => "runtime",
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
            None,
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

    pub fn session_snapshot(&self, session_id: &str) -> Option<SessionSnapshot> {
        let sessions = self.sessions.lock().expect("sessions lock poisoned");
        sessions.get(session_id).map(SessionView::snapshot)
    }

    pub fn list_workers(&self) -> Vec<WorkerRecord> {
        let state = self.state.lock().expect("state lock poisoned");
        state.workers.values().cloned().collect()
    }

    pub fn list_jobs(&self) -> Vec<JobRecord> {
        let state = self.state.lock().expect("state lock poisoned");
        let mut jobs = state.jobs.values().cloned().collect::<Vec<_>>();
        jobs.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        jobs
    }

    pub fn job_snapshot(&self, job_id: &str) -> Option<JobSnapshot> {
        let state = self.state.lock().expect("state lock poisoned");
        let job = state.jobs.get(job_id).cloned()?;

        let mut batches = job
            .batch_ids
            .iter()
            .filter_map(|batch_id| state.batches.get(batch_id))
            .map(|batch| JobBatchSnapshot {
                id: batch.id,
                status: batch_status_text(&batch.status).to_owned(),
                root_session_id: batch.root_session_id.clone(),
                root_prompt: batch.root_prompt.clone(),
                updated_at: batch.updated_at,
            })
            .collect::<Vec<_>>();
        batches.sort_by(|left, right| left.id.cmp(&right.id));

        let mut workers = job
            .worker_ids
            .iter()
            .filter_map(|worker_id| state.workers.get(worker_id))
            .map(|worker| JobWorkerSnapshot {
                id: worker.id.clone(),
                group: worker.group.clone(),
                task: worker.task.clone(),
                status: worker.status.to_string(),
                summary: worker.summary.clone().or(worker.lifecycle_note.clone()),
            })
            .collect::<Vec<_>>();
        workers.sort_by(|left, right| left.id.cmp(&right.id));

        Some(JobSnapshot {
            id: job.id,
            status: job.status.to_string(),
            title: job.title,
            objective: job.objective,
            source_channel: job.source_channel,
            requester: job.requester,
            priority: job.priority,
            pattern: job.policy.pattern,
            approval_required: job.policy.approval_required,
            created_at: job.created_at,
            updated_at: job.updated_at,
            latest_summary: job.latest_summary,
            escalation_state: job.escalation_state,
            final_outcome: job.final_outcome,
            context: job.context,
            batch_ids: batches,
            workers,
        })
    }

    pub fn create_job(&self, request: CreateJobRequest) -> Result<JobRecord> {
        let title = request.title.trim();
        if title.is_empty() {
            anyhow::bail!("job title must not be empty");
        }

        let objective = request.objective.trim();
        if objective.is_empty() {
            anyhow::bail!("job objective must not be empty");
        }

        let now = now_unix_ts();
        let job = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let job_number = state.next_job_number;
            state.next_job_number += 1;
            let job = JobRecord {
                id: format!("JOB-{job_number:03}"),
                source_channel: request.source_channel,
                requester: request.requester,
                title: title.to_owned(),
                objective: objective.to_owned(),
                context: request.context,
                status: JobStatus::Pending,
                priority: request.priority,
                policy: JobPolicy {
                    pattern: request.pattern,
                    approval_required: request.approval_required,
                },
                created_at: now,
                updated_at: now,
                batch_ids: Vec::new(),
                worker_ids: Vec::new(),
                latest_summary: Some("job created".to_owned()),
                latest_report_at: None,
                next_report_due_at: None,
                escalation_state: None,
                final_outcome: None,
            };
            state.jobs.insert(job.id.clone(), job.clone());
            job
        };

        self.save_state()?;
        Ok(job)
    }

    pub fn batch_snapshot(&self, batch_id: u64) -> Option<BatchSnapshot> {
        let (record, events) = {
            let state = self.state.lock().expect("state lock poisoned");
            let record = state.batches.get(&batch_id).cloned()?;
            let mut events = state
                .session_history
                .iter()
                .flat_map(|(session_id, session_events)| {
                    session_events
                        .iter()
                        .filter(move |event| event.batch_id == Some(batch_id))
                        .cloned()
                        .map(move |event| (session_id.clone(), event))
                })
                .collect::<Vec<_>>();
            events.sort_by(|left, right| {
                left.1
                    .ts
                    .cmp(&right.1.ts)
                    .then_with(|| left.0.cmp(&right.0))
                    .then_with(|| left.1.text.cmp(&right.1.text))
            });
            (record, events)
        };

        let session_meta = {
            let sessions = self.sessions.lock().expect("sessions lock poisoned");
            record
                .sessions
                .iter()
                .map(|session_id| {
                    let snapshot = sessions.get(session_id).map(SessionView::snapshot);
                    BatchSessionSnapshot {
                        id: session_id.clone(),
                        title: snapshot
                            .as_ref()
                            .map(|session| session.title.clone())
                            .unwrap_or_else(|| session_id.clone()),
                        status: snapshot
                            .as_ref()
                            .map(|session| session.status.clone())
                            .unwrap_or_else(|| "unknown".to_owned()),
                    }
                })
                .collect::<Vec<_>>()
        };

        let root_session_title = session_meta
            .iter()
            .find(|session| session.id == record.root_session_id)
            .map(|session| session.title.clone())
            .unwrap_or_else(|| record.root_session_id.clone());

        let events = events
            .into_iter()
            .map(|(session_id, event)| {
                let session_title = session_meta
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| session.title.clone())
                    .unwrap_or_else(|| session_id.clone());
                BatchEventSnapshot {
                    session_title,
                    kind: event.kind,
                    text: event.text,
                }
            })
            .collect::<Vec<_>>();

        Some(BatchSnapshot {
            id: record.id,
            job_id: record.job_id,
            root_session_id: record.root_session_id,
            root_session_title,
            root_prompt: record.root_prompt,
            status: batch_status_text(&record.status).to_owned(),
            created_at: record.created_at,
            updated_at: record.updated_at,
            sessions: session_meta,
            last_event: record.last_event,
            events,
        })
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
                    None,
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
        self.write_master_status("idle", None, None, None)?;
        Ok(thread_id)
    }

    pub async fn submit_prompt(&self, target: PromptTarget, prompt: &str) -> Result<()> {
        self.submit_prompt_for_job(target, prompt, None).await
    }

    pub async fn submit_prompt_for_job(
        &self,
        target: PromptTarget,
        prompt: &str,
        job_id: Option<&str>,
    ) -> Result<()> {
        let (session_id, thread_id, log_label, role) = self.resolve_prompt_target(target).await?;
        let batch_id = self.allocate_batch_id()?;
        self.register_batch(&session_id, batch_id, prompt, job_id)?;
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
        self.submit_prompt_and_wait_for_job(target, prompt, None).await
    }

    pub async fn submit_prompt_and_wait_for_job(
        &self,
        target: PromptTarget,
        prompt: &str,
        job_id: Option<&str>,
    ) -> Result<()> {
        let (session_id, thread_id, log_label, role) = self.resolve_prompt_target(target).await?;
        let batch_id = self.allocate_batch_id()?;
        self.register_batch(&session_id, batch_id, prompt, job_id)?;
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

    fn allocate_batch_id(&self) -> Result<u64> {
        let batch_id = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let batch_id = state.next_batch_id;
            state.next_batch_id += 1;
            batch_id
        };
        self.save_state()?;
        Ok(batch_id)
    }

    fn register_batch(
        &self,
        session_id: &str,
        batch_id: u64,
        prompt: &str,
        job_id: Option<&str>,
    ) -> Result<()> {
        if let Some(job_id) = job_id {
            self.ensure_job_exists(job_id)?;
        }

        {
            let mut state = self.state.lock().expect("state lock poisoned");
            state
                .batches
                .entry(batch_id)
                .or_insert(OrchestrationBatchRecord {
                    id: batch_id,
                    root_session_id: session_id.to_owned(),
                    root_prompt: compact_message(prompt),
                    job_id: job_id.map(str::to_owned),
                    status: BatchStatus::Running,
                    created_at: now_unix_ts(),
                    updated_at: now_unix_ts(),
                    sessions: vec![session_id.to_owned()],
                    last_event: Some("batch registered".to_owned()),
                });
            if let Some(job_id) = job_id {
                if let Some(batch) = state.batches.get_mut(&batch_id) {
                    match &batch.job_id {
                        Some(existing) if existing != job_id => {
                            anyhow::bail!(
                                "batch b{batch_id:03} is already linked to job `{existing}`"
                            );
                        }
                        Some(_) => {}
                        None => batch.job_id = Some(job_id.to_owned()),
                    }
                }
            }
        }
        self.save_state()?;
        if let Some(job_id) = job_id {
            self.link_batch_to_job(job_id, batch_id, prompt)?;
        }
        Ok(())
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
                Some(batch_id),
            )?;
            return Ok(rx);
        }

        self.start_queued_turn(turn)?;

        Ok(rx)
    }

    pub async fn spawn_worker(&self, group: &str, task: &str) -> Result<WorkerRecord> {
        self.spawn_worker_for_job(group, task, None).await
    }

    pub async fn spawn_worker_for_job(
        &self,
        group: &str,
        task: &str,
        job_id: Option<&str>,
    ) -> Result<WorkerRecord> {
        let batch_id = self.allocate_batch_id()?;
        self.register_batch(
            MASTER_SESSION_ID,
            batch_id,
            &format!("spawn worker [{group}] {task}"),
            job_id,
        )?;
        self.spawn_worker_with_options(group, task, None, None, false, batch_id)
            .await
    }

    pub async fn spawn_worker_and_wait(&self, group: &str, task: &str) -> Result<WorkerRecord> {
        self.spawn_worker_and_wait_for_job(group, task, None).await
    }

    pub async fn spawn_worker_and_wait_for_job(
        &self,
        group: &str,
        task: &str,
        job_id: Option<&str>,
    ) -> Result<WorkerRecord> {
        let batch_id = self.allocate_batch_id()?;
        self.register_batch(
            MASTER_SESSION_ID,
            batch_id,
            &format!("spawn worker [{group}] {task}"),
            job_id,
        )?;
        self.spawn_worker_with_options(group, task, None, None, true, batch_id)
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
        if let Some(job_id) = worker.job_id.as_deref() {
            self.refresh_job_tracking(job_id, Some(summary.to_owned()))?;
        }
        self.append_log_line(worker_id, format!("system> summary updated: {summary}"))?;
        self.append_session_event(
            worker_id,
            SessionEventKind::System,
            format!("summary updated: {summary}"),
            self.current_active_batch_id(worker_id),
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
        let linked_job = self.job_for_batch(batch_id);
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
            render_task_file(task_number, task, &group_config.lease_paths, linked_job.as_ref()),
        )
        .with_context(|| format!("failed to write {}", task_file.display()))?;

        let now = now_unix_ts();
        let mut record = WorkerRecord {
            id: worker_id.clone(),
            group: group.to_owned(),
            task: task.to_owned(),
            job_id: linked_job.as_ref().map(|job| job.id.clone()),
            summary: summary.clone(),
            lifecycle_note: None,
            task_file: task_file.display().to_string(),
            thread_id: "pending".to_owned(),
            status: WorkerStatus::SpawnRequested,
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
        if let Some(job) = linked_job.as_ref() {
            self.link_worker_to_job(&job.id, &record.id)?;
        }
        self.write_worker_status(&record)?;
        self.upsert_worker_session(&record);
        self.write_role_status(
            &SessionRole::Worker(record.id.clone()),
            "spawn_requested",
            None,
            None,
            None,
        )?;
        self.append_log_line(
            &record.id,
            format!("system> worker created from {}", record.task_file),
        )?;
        self.append_session_event(
            &record.id,
            SessionEventKind::System,
            format!("spawn requested from {}", record.task_file),
            Some(batch_id),
        )?;
        if let Some(summary) = summary {
            self.set_session_summary(&record.id, Some(summary.clone()))?;
            self.append_log_line(&record.id, format!("system> summary: {summary}"))?;
            self.append_session_event(
                &record.id,
                SessionEventKind::System,
                format!("initial summary: {summary}"),
                Some(batch_id),
            )?;
        }

        let response = match self
            .start_thread(
                &thread_name,
                Some(worker_instructions(group, task, &task_file)),
                Some(&thread_name),
                false,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.append_log_line(
                    &record.id,
                    format!("error> failed to start worker: {error}"),
                )?;
                self.append_session_event(
                    &record.id,
                    SessionEventKind::Error,
                    format!("worker spawn failed: {error}"),
                    Some(batch_id),
                )?;
                self.write_role_status(
                    &SessionRole::Worker(record.id.clone()),
                    "failed",
                    None,
                    Some(compact_message(&error.to_string())),
                    Some(compact_message(&error.to_string())),
                )?;
                return Err(error);
            }
        };

        self.update_worker_thread(&record.id, response.thread.id.clone())?;
        record.thread_id = response.thread.id;
        record.status = WorkerStatus::Bootstrapping;
        self.write_role_status(
            &SessionRole::Worker(record.id.clone()),
            "bootstrapping",
            None,
            None,
            None,
        )?;
        self.append_log_line(
            &record.id,
            format!("system> worker thread started: {}", record.thread_id),
        )?;
        self.append_session_event(
            &record.id,
            SessionEventKind::System,
            format!("worker thread ready: {}", record.thread_id),
            Some(batch_id),
        )?;

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

        let latest = {
            let state = self.state.lock().expect("state lock poisoned");
            state.workers.get(&record.id).cloned().unwrap_or(record)
        };

        Ok(latest)
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
        source: TurnSource,
        wait_for_follow_on: bool,
    ) -> Result<()> {
        let mut receiver = self.client.subscribe();
        self.write_role_status(
            &role,
            active_state_for_turn(&role, &source),
            None,
            None,
            None,
        )?;
        let model_prompt = self.prepare_prompt_for_role(&prompt, &role, batch_id);

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
        self.write_role_status(
            &role,
            active_state_for_turn(&role, &source),
            Some(turn_id.clone()),
            None,
            None,
        )?;

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
                                    Some(batch_id),
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
                        if let Some(mapped) = inflight_runtime_state(&status, &role, &source) {
                            self.set_session_status(&session_id, mapped)?;
                        }
                    }
                }
                "turn/started" => {
                    let event: TurnStartedNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn.id == turn_id {
                        let _ = &event.turn.status;
                        self.set_session_status(
                            &session_id,
                            active_state_for_turn(&role, &source),
                        )?;
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
                            Some(batch_id),
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
                                Some(batch_id),
                            )?;
                            self.set_batch_status(batch_id, BatchStatus::Failed)?;
                            self.write_role_status(
                                &role,
                                "failed",
                                Some(turn_id.clone()),
                                Some(error.message.clone()),
                                Some(compact_message(&error.message)),
                            )?;
                            if let SessionRole::Worker(worker_id) = &role {
                                self.publish_worker_runtime_update(
                                    worker_id, "failed", batch_id, &source,
                                )
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

                        let next_state = completed_state_for_turn(&role, &source, &assistant_text);
                        let lifecycle_note = match &role {
                            SessionRole::Worker(_) => {
                                lifecycle_note_for(next_state, &assistant_text)
                            }
                            SessionRole::Master => None,
                        };
                        self.write_role_status(
                            &role,
                            next_state,
                            Some(turn_id.clone()),
                            last_message,
                            lifecycle_note,
                        )?;
                        if let SessionRole::Worker(worker_id) = &role {
                            self.publish_worker_runtime_update(
                                worker_id, next_state, batch_id, &source,
                            )
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
            None,
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
        let (history, output, live_buffer) = {
            let state = self.state.lock().expect("state lock poisoned");
            (
                state.session_history.get(MASTER_SESSION_ID).cloned(),
                state.session_output.get(MASTER_SESSION_ID).cloned(),
                state.session_live_buffers.get(MASTER_SESSION_ID).cloned(),
            )
        };
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(MASTER_SESSION_ID) {
            session.set_thread_id(thread_id);
            session.set_summary(summary);
            session.set_last_message(last_message);
            if let Some(history) = &history {
                session.restore_timeline(history);
            }
            if let Some(output) = &output {
                if session.output_is_empty() {
                    session.restore_output(output);
                }
            }
            if let Some(live_buffer) = &live_buffer {
                session.restore_live_buffer(live_buffer);
            }
        } else {
            let mut session = SessionView::master(
                thread_id,
                self.workspace_root.display().to_string(),
                summary,
                last_message,
            );
            if let Some(history) = &history {
                session.restore_timeline(history);
            }
            if let Some(output) = &output {
                session.restore_output(output);
            }
            if let Some(live_buffer) = &live_buffer {
                session.restore_live_buffer(live_buffer);
            }
            sessions.insert(MASTER_SESSION_ID.to_owned(), session);
        }
    }

    fn upsert_worker_session(&self, worker: &WorkerRecord) {
        let (history, output, live_buffer) = {
            let state = self.state.lock().expect("state lock poisoned");
            (
                state.session_history.get(&worker.id).cloned(),
                state.session_output.get(&worker.id).cloned(),
                state.session_live_buffers.get(&worker.id).cloned(),
            )
        };
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        let mut session =
            SessionView::from_worker(worker, self.workspace_root.display().to_string());
        if let Some(history) = history {
            session.restore_timeline(&history);
        }
        if let Some(output) = output {
            session.restore_output(&output);
        }
        if let Some(live_buffer) = live_buffer {
            session.restore_live_buffer(&live_buffer);
        }
        sessions.insert(worker.id.clone(), session);
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
            Some(turn.batch_id),
        )?;
        self.set_active_turn_batch(&turn.session_id, turn.batch_id)?;
        self.set_session_status(
            &turn.session_id,
            queued_state_for_turn(&turn.role, &turn.source),
        )?;

        let controller = self.clone();
        let source = turn.source.clone();
        tokio::spawn(async move {
            let result = controller
                .process_turn(
                    turn.batch_id,
                    turn.session_id.clone(),
                    turn.thread_id,
                    turn.log_label,
                    turn.prompt,
                    turn.role,
                    source,
                    turn.wait_for_follow_on,
                )
                .await;
            if let Err(error) = &result {
                let _ = controller.append_log_line(&turn.session_id, format!("error> {error}"));
                let _ = controller.set_batch_status(turn.batch_id, BatchStatus::Failed);
                let _ = controller.set_session_status(&turn.session_id, "failed");
            }
            let _ = turn.completion.send(result);
            let _ = controller.clear_active_turn_batch(&turn.session_id, turn.batch_id);
            let _ = controller.schedule_next_turn(&turn.session_id);
            let _ = controller.refresh_batch_state(turn.batch_id);
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
        drop(active);
        self.touch_batch(batch_id, session_id, None)
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

    fn current_active_batch_id(&self, session_id: &str) -> Option<u64> {
        let active = self
            .active_turn_batches
            .lock()
            .expect("active turn lock poisoned");
        active.get(session_id).copied()
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
                self.refresh_batch_state(batch_id)?;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    }

    fn append_log_line(&self, session_id: &str, line: impl Into<String>) -> Result<()> {
        let line = line.into();
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.push_line(line.clone());
        }
        drop(sessions);
        self.persist_log_line(session_id, line)
    }

    fn persist_log_line(&self, session_id: &str, line: String) -> Result<()> {
        let mut state = self.state.lock().expect("state lock poisoned");
        let output = state
            .session_output
            .entry(session_id.to_owned())
            .or_default();
        output.push(line);
        trim_persisted_log_lines(output);
        drop(state);
        self.save_state()
    }

    fn append_session_event(
        &self,
        session_id: &str,
        kind: SessionEventKind,
        text: impl Into<String>,
        batch_id: Option<u64>,
    ) -> Result<()> {
        let text = text.into();
        let event = SessionEvent::new(kind, text.clone(), batch_id);
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.push_timeline_event(event.clone());
        }
        drop(sessions);

        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let history = state
                .session_history
                .entry(session_id.to_owned())
                .or_default();
            history.push(event);
            trim_persisted_events(history);

            if let Some(batch_id) = batch_id {
                let batch =
                    state
                        .batches
                        .entry(batch_id)
                        .or_insert_with(|| OrchestrationBatchRecord {
                            id: batch_id,
                            root_session_id: session_id.to_owned(),
                            root_prompt: "(implicit batch)".to_owned(),
                            job_id: None,
                            status: BatchStatus::Running,
                            created_at: now_unix_ts(),
                            updated_at: now_unix_ts(),
                            sessions: Vec::new(),
                            last_event: None,
                        });
                batch.touch(session_id, Some(&text));
            }
        }
        self.save_state()
    }

    fn append_live_chunk(&self, session_id: &str, chunk: &str) -> Result<()> {
        {
            let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
            if let Some(session) = sessions.get_mut(session_id) {
                session.append_live_chunk(chunk);
            }
        }
        self.persist_live_buffer_append(session_id, chunk)
    }

    fn set_live_buffer(&self, session_id: &str, content: &str) -> Result<()> {
        {
            let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
            if let Some(session) = sessions.get_mut(session_id) {
                session.set_live_buffer(content);
            }
        }
        self.persist_live_buffer_set(session_id, content)
    }

    fn commit_live_buffer(&self, session_id: &str) -> Result<Option<String>> {
        let committed = {
            let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
            sessions
                .get_mut(session_id)
                .and_then(SessionView::commit_live_buffer)
        };
        self.persist_committed_live_buffer(session_id, committed.as_deref())?;
        Ok(committed)
    }

    fn replace_last_assistant_line(&self, session_id: &str, text: &str) -> Result<()> {
        let replacement = format!("assistant> {text}");
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.replace_last_assistant_line(text);
        }
        drop(sessions);
        let mut state = self.state.lock().expect("state lock poisoned");
        if let Some(lines) = state.session_output.get_mut(session_id) {
            if let Some(last) = lines.last_mut() {
                if last.starts_with("assistant> ") {
                    *last = replacement;
                }
            }
        }
        drop(state);
        self.save_state()
    }

    fn set_session_status(&self, session_id: &str, status: &str) -> Result<()> {
        let changed = {
            let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
            if let Some(session) = sessions.get_mut(session_id) {
                session.set_status(status.to_owned())
            } else {
                false
            }
        };
        if changed {
            self.append_session_event(
                session_id,
                SessionEventKind::Status,
                format!("state -> {status}"),
                self.current_active_batch_id(session_id),
            )?;
        }
        Ok(())
    }

    fn update_worker_thread(&self, worker_id: &str, thread_id: String) -> Result<()> {
        let worker = {
            let mut persisted = self.state.lock().expect("state lock poisoned");
            let worker = persisted
                .workers
                .get_mut(worker_id)
                .with_context(|| format!("unknown worker `{worker_id}`"))?;
            worker.thread_id = thread_id.clone();
            worker.updated_at = now_unix_ts();
            worker.clone()
        };
        self.save_state()?;
        {
            let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
            if let Some(session) = sessions.get_mut(worker_id) {
                session.set_thread_id(thread_id);
            }
        }
        self.write_worker_status(&worker)
    }

    fn touch_batch(&self, batch_id: u64, session_id: &str, event_text: Option<&str>) -> Result<()> {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let batch = state
                .batches
                .entry(batch_id)
                .or_insert_with(|| OrchestrationBatchRecord {
                    id: batch_id,
                    root_session_id: session_id.to_owned(),
                    root_prompt: "(implicit batch)".to_owned(),
                    job_id: None,
                    status: BatchStatus::Running,
                    created_at: now_unix_ts(),
                    updated_at: now_unix_ts(),
                    sessions: Vec::new(),
                    last_event: None,
                });
            batch.touch(session_id, event_text);
        }
        self.save_state()
    }

    fn set_batch_status(&self, batch_id: u64, status: BatchStatus) -> Result<()> {
        let linked_job_id = {
            let mut state = self.state.lock().expect("state lock poisoned");
            if let Some(batch) = state.batches.get_mut(&batch_id) {
                batch.status = status.clone();
                batch.updated_at = now_unix_ts();
                batch.job_id.clone()
            } else {
                None
            }
        };
        self.save_state()?;
        if let Some(job_id) = linked_job_id.as_deref() {
            let summary = match status {
                BatchStatus::Running => None,
                BatchStatus::Completed => Some(format!("batch b{batch_id:03} completed")),
                BatchStatus::Failed => Some(format!("batch b{batch_id:03} failed")),
            };
            self.refresh_job_tracking(job_id, summary)?;
        }
        Ok(())
    }

    fn refresh_batch_state(&self, batch_id: u64) -> Result<()> {
        let has_work =
            self.batch_has_active_turns(batch_id) || self.batch_has_pending_turns(batch_id);
        let next_status = {
            let state = self.state.lock().expect("state lock poisoned");
            state.batches.get(&batch_id).map(|batch| {
                if has_work {
                    batch.status.clone()
                } else if matches!(batch.status, BatchStatus::Failed) {
                    BatchStatus::Failed
                } else {
                    BatchStatus::Completed
                }
            })
        };

        if let Some(next_status) = next_status {
            self.set_batch_status(batch_id, next_status)?;
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

    fn set_session_lifecycle_note(&self, session_id: &str, note: Option<String>) -> Result<()> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        if let Some(session) = sessions.get_mut(session_id) {
            session.set_lifecycle_note(note);
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

    fn ensure_job_exists(&self, job_id: &str) -> Result<()> {
        let state = self.state.lock().expect("state lock poisoned");
        if state.jobs.contains_key(job_id) {
            Ok(())
        } else {
            anyhow::bail!("unknown job `{job_id}`");
        }
    }

    fn link_batch_to_job(&self, job_id: &str, batch_id: u64, prompt: &str) -> Result<()> {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let batch = state
                .batches
                .get_mut(&batch_id)
                .with_context(|| format!("unknown batch `b{batch_id}`"))?;
            match &batch.job_id {
                Some(existing) if existing != job_id => {
                    anyhow::bail!("batch b{batch_id:03} is already linked to job `{existing}`");
                }
                Some(_) => {}
                None => batch.job_id = Some(job_id.to_owned()),
            }

            let job = state
                .jobs
                .get_mut(job_id)
                .with_context(|| format!("unknown job `{job_id}`"))?;
            if !job.batch_ids.contains(&batch_id) {
                job.batch_ids.push(batch_id);
                job.batch_ids.sort_unstable();
            }
        }

        self.refresh_job_tracking(
            job_id,
            Some(format!(
                "batch b{batch_id:03} started: {}",
                compact_message(prompt)
            )),
        )
    }

    fn link_worker_to_job(&self, job_id: &str, worker_id: &str) -> Result<()> {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let worker = state
                .workers
                .get_mut(worker_id)
                .with_context(|| format!("unknown worker `{worker_id}`"))?;
            match &worker.job_id {
                Some(existing) if existing != job_id => {
                    anyhow::bail!("worker `{worker_id}` is already linked to job `{existing}`");
                }
                Some(_) => {}
                None => worker.job_id = Some(job_id.to_owned()),
            }

            let job = state
                .jobs
                .get_mut(job_id)
                .with_context(|| format!("unknown job `{job_id}`"))?;
            if !job.worker_ids.iter().any(|existing| existing == worker_id) {
                job.worker_ids.push(worker_id.to_owned());
                job.worker_ids.sort();
            }
        }

        self.refresh_job_tracking(job_id, Some(format!("worker assigned: {worker_id}")))
    }

    fn refresh_job_tracking(&self, job_id: &str, latest_summary: Option<String>) -> Result<()> {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let (batch_ids, worker_ids, existing_summary) = {
                let job = state
                    .jobs
                    .get(job_id)
                    .with_context(|| format!("unknown job `{job_id}`"))?;
                (
                    job.batch_ids.clone(),
                    job.worker_ids.clone(),
                    job.latest_summary.clone(),
                )
            };

            let status = derive_job_status_from_state(&state, &batch_ids, &worker_ids);
            let next_summary = latest_summary.or(existing_summary);
            if let Some(job) = state.jobs.get_mut(job_id) {
                job.status = status.clone();
                job.updated_at = now_unix_ts();
                if next_summary.is_some() {
                    job.latest_summary = next_summary.clone();
                }
                job.final_outcome = match status {
                    JobStatus::Completed | JobStatus::Failed => job.latest_summary.clone(),
                    _ => None,
                };
            }
        }
        self.save_state()
    }

    fn job_for_batch(&self, batch_id: u64) -> Option<JobRecord> {
        let state = self.state.lock().expect("state lock poisoned");
        let job_id = state.batches.get(&batch_id)?.job_id.clone()?;
        state.jobs.get(&job_id).cloned()
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
        lifecycle_note: Option<String>,
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
                self.set_session_lifecycle_note(MASTER_SESSION_ID, lifecycle_note.clone())?;
                self.write_master_status(state, last_turn_id, last_message, lifecycle_note)
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
                    worker.lifecycle_note = lifecycle_note.clone();
                    worker.clone()
                };
                self.save_state()?;
                if let Some(job_id) = worker.job_id.as_deref() {
                    self.refresh_job_tracking(
                        job_id,
                        worker
                            .lifecycle_note
                            .clone()
                            .or(worker.summary.clone())
                            .or(worker.last_message.clone()),
                    )?;
                }
                self.set_session_status(worker_id, state)?;
                self.set_session_last_turn_id(worker_id, last_turn_id)?;
                self.set_session_last_message(worker_id, last_message)?;
                self.set_session_lifecycle_note(worker_id, lifecycle_note)?;
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
        lifecycle_note: Option<String>,
    ) -> Result<()> {
        let thread_id = self.master_thread_id().unwrap_or_default();
        let status = SessionStatus {
            role: "master".to_owned(),
            thread_id,
            state: state.to_owned(),
            updated_at: now_unix_ts(),
            job_id: None,
            summary: self
                .master_summary()
                .or_else(|| Some("Primary planner and dispatcher".to_owned())),
            lifecycle_note,
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
            job_id: worker.job_id.clone(),
            summary: worker.summary.clone(),
            lifecycle_note: worker.lifecycle_note.clone(),
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
        source: &TurnSource,
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
            format!(
                "worker {worker_id} reported {worker_state} ({})",
                source.runtime_label()
            ),
            Some(batch_id),
        )?;
        let prompt = format!(
            "CodeClaw runtime update. This is an internal worker status event, not a direct human message.\n\nWorker id: {worker_id}\nLifecycle state: {worker_state}\nOrigin: {}\nGroup: {}\nTask: {}\nSidebar summary: {}\nLifecycle note: {}\nLast worker message: {}\n\nInterpret lifecycle states as follows:\n- bootstrapped: the worker completed its initial bootstrap turn and is ready for supervision\n- handed_back: the worker finished a follow-up turn and returned control to the master\n- blocked: the worker reported a blocker and likely needs intervention\n- failed: the worker turn failed unexpectedly\n\nUpdate the operator with a concise coordination response and include the required <codeclaw-actions> block. If no follow-up is needed, return an empty actions list.",
            source.runtime_label(),
            worker.group,
            worker.task,
            worker
                .summary
                .clone()
                .unwrap_or_else(|| "not set".to_owned()),
            worker
                .lifecycle_note
                .clone()
                .unwrap_or_else(|| "none".to_owned()),
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
            "You are the master controller for CodeClaw in {}. Coordinate work across workers, keep plans concise, and prefer actionable task splits.\n\nYou may receive direct human prompts and internal runtime updates about worker lifecycle changes. Treat runtime updates as scheduler inputs: absorb the worker result, update summaries when useful, and dispatch follow-up actions only when they are actually needed.\n\nImportant lifecycle meanings:\n- bootstrapped: the worker completed its initial bootstrap turn\n- handed_back: the worker completed a later turn and returned control\n- blocked: the worker reported a blocker and likely needs intervention\n- failed: the worker turn failed unexpectedly\n\nWhen you respond, append exactly one machine-readable block at the end using this format:\n<codeclaw-actions>\n{{\"summary\":\"short orchestration summary\",\"actions\":[...]}}\n</codeclaw-actions>\n\nAllowed actions:\n- {{\"type\":\"spawn_worker\",\"group\":\"backend|frontend|infra\",\"task\":\"short task title\",\"summary\":\"optional short sidebar summary\",\"prompt\":\"optional initial worker prompt\"}}\n- {{\"type\":\"send_worker_prompt\",\"worker_id\":\"existing-worker-id\",\"prompt\":\"follow-up instructions\"}}\n- {{\"type\":\"update_worker_summary\",\"worker_id\":\"existing-worker-id\",\"summary\":\"new short summary\"}}\n\nRules:\n- Always include the block, even when no actions are needed.\n- Keep `summary` short enough to fit a sidebar.\n- Use worker ids exactly as shown in the UI when referencing existing workers.",
            self.workspace_root.display()
        )
    }

    fn prepare_prompt_for_role(&self, prompt: &str, role: &SessionRole, batch_id: u64) -> String {
        let job_context = self
            .job_for_batch(batch_id)
            .map(|job| format_job_context(&job));

        match role {
            SessionRole::Master => {
                if let Some(job_context) = job_context {
                    format!(
                        "{job_context}\n\nOperator prompt:\n{prompt}\n\nCodeClaw runtime reminder: finish with the required <codeclaw-actions> JSON block."
                    )
                } else {
                    format!(
                        "{prompt}\n\nCodeClaw runtime reminder: finish with the required <codeclaw-actions> JSON block."
                    )
                }
            }
            SessionRole::Worker(_) => {
                if let Some(job_context) = job_context {
                    format!("{job_context}\n\nTask prompt:\n{prompt}")
                } else {
                    prompt.to_owned()
                }
            }
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
                Some(batch_id),
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
                        Some(batch_id),
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
                                Some(batch_id),
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
                                Some(batch_id),
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
                        Some(batch_id),
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
                                    Some(batch_id),
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
                                Some(batch_id),
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
                        Some(batch_id),
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
                            Some(batch_id),
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
        let mut session = SessionView::master(
            thread_id,
            workspace_root.display().to_string(),
            state.master_summary.clone(),
            state.master_last_message.clone(),
        );
        if let Some(events) = state.session_history.get(MASTER_SESSION_ID) {
            session.restore_timeline(events);
        }
        if let Some(lines) = state.session_output.get(MASTER_SESSION_ID) {
            session.restore_output(lines);
        }
        if let Some(live_buffer) = state.session_live_buffers.get(MASTER_SESSION_ID) {
            session.restore_live_buffer(live_buffer);
        }
        sessions.insert(MASTER_SESSION_ID.to_owned(), session);
    }
    for worker in state.workers.values() {
        let mut session = SessionView::from_worker(worker, workspace_root.display().to_string());
        if let Some(events) = state.session_history.get(&worker.id) {
            session.restore_timeline(events);
        }
        if let Some(lines) = state.session_output.get(&worker.id) {
            session.restore_output(lines);
        }
        if let Some(live_buffer) = state.session_live_buffers.get(&worker.id) {
            session.restore_live_buffer(live_buffer);
        }
        sessions.insert(worker.id.clone(), session);
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
        "spawn_requested" => WorkerStatus::SpawnRequested,
        "bootstrapping" => WorkerStatus::Bootstrapping,
        "bootstrapped" => WorkerStatus::Bootstrapped,
        "blocked" => WorkerStatus::Blocked,
        "handed_back" => WorkerStatus::HandedBack,
        "completed" => WorkerStatus::Completed,
        "failed" => WorkerStatus::Failed,
        "running" | "queued" | "active" | "inProgress" => WorkerStatus::Running,
        _ => WorkerStatus::Idle,
    }
}

fn queued_state_for_turn(role: &SessionRole, source: &TurnSource) -> &'static str {
    match (role, source) {
        (SessionRole::Worker(_), TurnSource::Bootstrap) => "bootstrapping",
        _ => "queued",
    }
}

fn active_state_for_turn(role: &SessionRole, source: &TurnSource) -> &'static str {
    match (role, source) {
        (SessionRole::Worker(_), TurnSource::Bootstrap) => "bootstrapping",
        _ => "running",
    }
}

fn inflight_runtime_state<'a>(
    raw_state: &'a str,
    role: &SessionRole,
    source: &TurnSource,
) -> Option<&'a str> {
    match raw_state {
        "queued" => Some(queued_state_for_turn(role, source)),
        "running" | "active" | "inProgress" => Some(active_state_for_turn(role, source)),
        "failed" => Some("failed"),
        _ => None,
    }
}

fn completed_state_for_turn(
    role: &SessionRole,
    source: &TurnSource,
    assistant_text: &str,
) -> &'static str {
    match role {
        SessionRole::Master => "completed",
        SessionRole::Worker(_) => {
            if worker_message_indicates_blocker(assistant_text) {
                "blocked"
            } else if matches!(source, TurnSource::Bootstrap) {
                "bootstrapped"
            } else {
                "handed_back"
            }
        }
    }
}

fn lifecycle_note_for(state: &str, assistant_text: &str) -> Option<String> {
    match state {
        "blocked" => blocker_reason_from_message(assistant_text)
            .or_else(|| first_meaningful_line(assistant_text))
            .or_else(|| Some("worker reported a blocker".to_owned())),
        "bootstrapped" => first_meaningful_line(assistant_text)
            .or_else(|| Some("initial handoff ready".to_owned())),
        "handed_back" => first_meaningful_line(assistant_text)
            .or_else(|| Some("worker returned control".to_owned())),
        _ => None,
    }
}

fn worker_message_indicates_blocker(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    [
        "blocked",
        "blocker",
        "need approval",
        "need input",
        "need guidance",
        "waiting on",
        "cannot continue",
        "can't continue",
        "unable to continue",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn blocker_reason_from_message(message: &str) -> Option<String> {
    message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| worker_message_indicates_blocker(line))
        .map(compact_message)
}

fn first_meaningful_line(message: &str) -> Option<String> {
    message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(compact_message)
        .next()
}

fn batch_status_text(status: &BatchStatus) -> &'static str {
    match status {
        BatchStatus::Running => "running",
        BatchStatus::Completed => "completed",
        BatchStatus::Failed => "failed",
    }
}

fn derive_job_status_from_state(
    state: &AppState,
    batch_ids: &[u64],
    worker_ids: &[String],
) -> JobStatus {
    let has_failed_batch = batch_ids.iter().any(|batch_id| {
        state
            .batches
            .get(batch_id)
            .map(|batch| matches!(batch.status, BatchStatus::Failed))
            .unwrap_or(false)
    });
    let has_failed_worker = worker_ids.iter().any(|worker_id| {
        state
            .workers
            .get(worker_id)
            .map(|worker| matches!(worker.status, WorkerStatus::Failed))
            .unwrap_or(false)
    });
    if has_failed_batch || has_failed_worker {
        return JobStatus::Failed;
    }

    let has_blocked_worker = worker_ids.iter().any(|worker_id| {
        state
            .workers
            .get(worker_id)
            .map(|worker| matches!(worker.status, WorkerStatus::Blocked))
            .unwrap_or(false)
    });
    if has_blocked_worker {
        return JobStatus::Blocked;
    }

    let has_running_batch = batch_ids.iter().any(|batch_id| {
        state
            .batches
            .get(batch_id)
            .map(|batch| matches!(batch.status, BatchStatus::Running))
            .unwrap_or(false)
    });
    if has_running_batch {
        return JobStatus::Running;
    }

    if batch_ids.is_empty() {
        JobStatus::Pending
    } else {
        JobStatus::Completed
    }
}

fn format_job_context(job: &JobRecord) -> String {
    let requester = job
        .requester
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let context = job
        .context
        .clone()
        .unwrap_or_else(|| "none".to_owned());
    format!(
        "Job context:\n- Job id: {}\n- Title: {}\n- Objective: {}\n- Source channel: {}\n- Requester: {}\n- Priority: {}\n- Pattern: {}\n- Approval required: {}\n- Current summary: {}\n- Context: {}",
        job.id,
        job.title,
        job.objective,
        job.source_channel,
        requester,
        job.priority,
        job.policy.pattern,
        if job.policy.approval_required { "yes" } else { "no" },
        job.latest_summary
            .clone()
            .unwrap_or_else(|| "none".to_owned()),
        context
    )
}

fn render_task_file(
    task_number: u64,
    task: &str,
    lease_paths: &[String],
    job: Option<&JobRecord>,
) -> String {
    let lease_section = if lease_paths.is_empty() {
        "- (not specified)\n".to_owned()
    } else {
        lease_paths
            .iter()
            .map(|path| format!("- {path}\n"))
            .collect::<String>()
    };
    let job_section = if let Some(job) = job {
        format!(
            "## Job Context\n\n- id: {}\n- title: {}\n- objective: {}\n- priority: {}\n- pattern: {}\n- approval required: {}\n\n",
            job.id,
            job.title,
            job.objective,
            job.priority,
            job.policy.pattern,
            if job.policy.approval_required { "yes" } else { "no" }
        )
    } else {
        String::new()
    };

    format!(
        "# TASK-{task_number:03}\n\n## Goal\n\n{task}\n\n{job_section}## Acceptance Criteria\n\n- Make concrete progress on the assigned task.\n- Keep changes scoped to the leased area.\n- Report blockers explicitly.\n\n## Leased Paths\n\n{lease_section}"
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

fn trim_persisted_events(events: &mut Vec<SessionEvent>) {
    if events.len() > MAX_TIMELINE_EVENTS {
        let keep_from = events.len() - MAX_TIMELINE_EVENTS;
        events.drain(0..keep_from);
    }
}

fn trim_persisted_log_lines(lines: &mut Vec<String>) {
    if lines.len() > MAX_LOG_LINES {
        let keep_from = lines.len() - MAX_LOG_LINES;
        lines.drain(0..keep_from);
    }
}

impl Controller {
    fn persist_live_buffer_append(&self, session_id: &str, chunk: &str) -> Result<()> {
        let mut state = self.state.lock().expect("state lock poisoned");
        state
            .session_live_buffers
            .entry(session_id.to_owned())
            .or_default()
            .push_str(chunk);
        drop(state);
        self.save_state()
    }

    fn persist_live_buffer_set(&self, session_id: &str, content: &str) -> Result<()> {
        let mut state = self.state.lock().expect("state lock poisoned");
        if content.is_empty() {
            state.session_live_buffers.remove(session_id);
        } else {
            state
                .session_live_buffers
                .insert(session_id.to_owned(), content.to_owned());
        }
        drop(state);
        self.save_state()
    }

    fn persist_committed_live_buffer(
        &self,
        session_id: &str,
        committed: Option<&str>,
    ) -> Result<()> {
        let mut state = self.state.lock().expect("state lock poisoned");
        state.session_live_buffers.remove(session_id);
        if let Some(text) = committed {
            let output = state
                .session_output
                .entry(session_id.to_owned())
                .or_default();
            output.push(format!("assistant> {text}"));
            trim_persisted_log_lines(output);
        }
        drop(state);
        self.save_state()
    }
}

fn map_broadcast_error(error: broadcast::error::RecvError) -> anyhow::Error {
    anyhow!("app-server notification channel error: {error}")
}

#[cfg(test)]
mod tests {
    use super::{
        blocker_reason_from_message, completed_state_for_turn, derive_job_status_from_state,
        lifecycle_note_for, worker_message_indicates_blocker, worker_status_for, SessionRole,
        TurnSource,
    };
    use crate::state::{AppState, BatchStatus, JobStatus, OrchestrationBatchRecord, WorkerStatus};

    #[test]
    fn worker_status_maps_lifecycle_states() {
        assert_eq!(
            worker_status_for("spawn_requested"),
            WorkerStatus::SpawnRequested
        );
        assert_eq!(
            worker_status_for("bootstrapping"),
            WorkerStatus::Bootstrapping
        );
        assert_eq!(
            worker_status_for("bootstrapped"),
            WorkerStatus::Bootstrapped
        );
        assert_eq!(worker_status_for("blocked"), WorkerStatus::Blocked);
        assert_eq!(worker_status_for("handed_back"), WorkerStatus::HandedBack);
    }

    #[test]
    fn blocker_detection_matches_common_worker_phrases() {
        assert!(worker_message_indicates_blocker(
            "Blocked: I need approval before I can continue."
        ));
        assert!(worker_message_indicates_blocker(
            "I am waiting on input from the operator."
        ));
        assert!(!worker_message_indicates_blocker(
            "Implemented the requested refactor and handed the result back."
        ));
        assert_eq!(
            blocker_reason_from_message(
                "Done with prep.\nWaiting on DBA approval before I can continue."
            )
            .as_deref(),
            Some("Waiting on DBA approval before I can continue.")
        );
    }

    #[test]
    fn completed_worker_turns_map_to_bootstrap_and_handoff_states() {
        let worker_role = SessionRole::Worker("backend-001-test".to_owned());

        assert_eq!(
            completed_state_for_turn(
                &worker_role,
                &TurnSource::Bootstrap,
                "Task finished cleanly."
            ),
            "bootstrapped"
        );
        assert_eq!(
            completed_state_for_turn(
                &worker_role,
                &TurnSource::Orchestrator,
                "Implemented and summarized."
            ),
            "handed_back"
        );
        assert_eq!(
            completed_state_for_turn(
                &worker_role,
                &TurnSource::Orchestrator,
                "Blocked on missing approval."
            ),
            "blocked"
        );
    }

    #[test]
    fn lifecycle_notes_capture_blockers_and_handoffs() {
        assert_eq!(
            lifecycle_note_for(
                "blocked",
                "Implemented the migration.\nNeed approval from infra before production apply."
            )
            .as_deref(),
            Some("Need approval from infra before production apply.")
        );
        assert_eq!(
            lifecycle_note_for("handed_back", "Implemented API validation and added tests.")
                .as_deref(),
            Some("Implemented API validation and added tests.")
        );
        assert_eq!(
            lifecycle_note_for("bootstrapped", "").as_deref(),
            Some("initial handoff ready")
        );
    }

    #[test]
    fn job_status_prefers_failed_then_blocked_then_running_then_completed() {
        let mut state = AppState::default();
        state.batches.insert(
            1,
            OrchestrationBatchRecord {
                id: 1,
                root_session_id: "master".to_owned(),
                root_prompt: "root".to_owned(),
                job_id: Some("JOB-001".to_owned()),
                status: BatchStatus::Running,
                created_at: 1,
                updated_at: 1,
                sessions: vec!["master".to_owned()],
                last_event: None,
            },
        );

        assert_eq!(
            derive_job_status_from_state(&state, &[1], &[]),
            JobStatus::Running
        );

        state.workers.insert(
            "backend-001".to_owned(),
            crate::state::WorkerRecord {
                id: "backend-001".to_owned(),
                group: "backend".to_owned(),
                task: "task".to_owned(),
                job_id: Some("JOB-001".to_owned()),
                summary: None,
                lifecycle_note: None,
                task_file: "TASK-001.md".to_owned(),
                thread_id: "thread-1".to_owned(),
                status: WorkerStatus::Blocked,
                created_at: 1,
                updated_at: 1,
                last_turn_id: None,
                last_message: None,
            },
        );
        assert_eq!(
            derive_job_status_from_state(&state, &[1], &["backend-001".to_owned()]),
            JobStatus::Blocked
        );

        if let Some(worker) = state.workers.get_mut("backend-001") {
            worker.status = WorkerStatus::Failed;
        }
        assert_eq!(
            derive_job_status_from_state(&state, &[1], &["backend-001".to_owned()]),
            JobStatus::Failed
        );

        state.workers.clear();
        if let Some(batch) = state.batches.get_mut(&1) {
            batch.status = BatchStatus::Completed;
        }
        assert_eq!(
            derive_job_status_from_state(&state, &[1], &[]),
            JobStatus::Completed
        );
        assert_eq!(derive_job_status_from_state(&state, &[], &[]), JobStatus::Pending);
    }
}
