use crate::{
    app_server::{AppServerClient, Notification},
    config::{Config, CoordinationPaths, GroupConfig},
    gateway, logging,
    orchestration::{
        parse_master_response, visible_stream_text, MasterAction, ParsedMasterResponse,
    },
    service::{RuntimeSnapshot, ServiceLifecycle, ServiceSnapshot},
    session::{
        SessionEvent, SessionEventKind, SessionOverviewSnapshot, SessionSnapshot, SessionView,
        MAX_LOG_LINES, MAX_TIMELINE_EVENTS,
    },
    state::{
        now_unix_ts, AppState, BatchStatus, JobPolicy, JobRecord, JobReportKind, JobReportRecord,
        JobStatus, OrchestrationBatchRecord, ReportChannel, ReportDeliveryRecord,
        ReportDeliveryStatus, ReportSubscriptionRecord, SessionAutomationRecord,
        SessionAutomationStatus, SessionStatus, WorkerRecord, WorkerStatus,
    },
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio::sync::{broadcast, oneshot};

const MASTER_SESSION_ID: &str = "master";
const ONBOARD_SESSION_ID: &str = "onboard";
const DEFAULT_REPORT_CADENCE_SECS: u64 = 900;
const DEFAULT_AUTOMATION_CONTINUE_INTERVAL_SECS: u64 = 300;
const MAX_JOB_REPORTS_PER_JOB: usize = 32;
const MAX_JOB_DELIVERIES_PER_JOB: usize = 64;

#[derive(Debug, Clone)]
struct RuntimeContext {
    mode: String,
    command_label: String,
    started_at: u64,
}

#[derive(Clone)]
pub struct Controller {
    workspace_root: PathBuf,
    pub config: Config,
    pub paths: CoordinationPaths,
    state: Arc<Mutex<AppState>>,
    state_file_fingerprint: Arc<Mutex<Option<FileFingerprint>>>,
    sessions: Arc<Mutex<BTreeMap<String, SessionView>>>,
    pending_turns: Arc<Mutex<BTreeMap<String, VecDeque<QueuedTurn>>>>,
    active_turn_batches: Arc<Mutex<BTreeMap<String, u64>>>,
    runtime_context: Arc<Mutex<Option<RuntimeContext>>>,
    client: Arc<AppServerClient>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
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
    pub auto_approve: bool,
    pub delegate_to_master_loop: bool,
    pub continue_for_secs: Option<u64>,
    pub continue_max_iterations: Option<u32>,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateSessionAutomationRequest {
    pub target_session_id: String,
    pub prompt: String,
    pub interval_secs: u64,
    pub max_runs: Option<u32>,
    pub run_for_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct JobAutomationSnapshot {
    pub state: String,
    pub auto_approve: bool,
    pub delegate_to_master_loop: bool,
    pub automation_started_at: Option<u64>,
    pub last_continue_at: Option<u64>,
    pub continue_iterations: u32,
    pub remaining_secs: Option<u64>,
    pub remaining_iterations: Option<u32>,
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
    pub auto_approve: bool,
    pub delegate_to_master_loop: bool,
    pub continue_for_secs: Option<u64>,
    pub continue_max_iterations: Option<u32>,
    pub created_at: u64,
    pub updated_at: u64,
    pub latest_summary: Option<String>,
    pub latest_report_at: Option<u64>,
    pub next_report_due_at: Option<u64>,
    pub escalation_state: Option<String>,
    pub final_outcome: Option<String>,
    pub automation: JobAutomationSnapshot,
    pub context: Option<String>,
    pub batch_ids: Vec<JobBatchSnapshot>,
    pub workers: Vec<JobWorkerSnapshot>,
    pub reports: Vec<JobReportSnapshot>,
    pub subscriptions: Vec<JobSubscriptionSnapshot>,
    pub deliveries: Vec<JobDeliverySnapshot>,
}

#[derive(Debug, Clone)]
pub struct JobReportSnapshot {
    pub id: u64,
    pub kind: String,
    pub status: String,
    pub summary: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct JobSubscriptionSnapshot {
    pub id: u64,
    pub channel: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct JobDeliverySnapshot {
    pub id: u64,
    pub report_id: u64,
    pub channel: String,
    pub target: String,
    pub status: String,
    pub attempts: u32,
    pub updated_at: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OnboardLaneItem {
    pub job_id: String,
    pub title: String,
    pub status: String,
    pub operator_state: String,
    pub summary: String,
    pub workers: usize,
    pub automation: JobAutomationSnapshot,
}

#[derive(Debug, Clone)]
pub struct OnboardSnapshot {
    pub status: String,
    pub summary: String,
    pub service_status: Option<String>,
    pub service_tick: Option<u64>,
    pub runtime_connected: bool,
    pub runtime_pid: Option<u32>,
    pub runtime_mode: Option<String>,
    pub runtime_command_label: Option<String>,
    pub active_turns: usize,
    pub queued_turns: usize,
    pub running_workers: usize,
    pub queued_deliveries: usize,
    pub delegated_jobs: usize,
    pub auto_approve_jobs: usize,
    pub budget_exhausted_jobs: usize,
    pub continued_jobs: Vec<String>,
    pub armed_automations: usize,
    pub paused_automations: usize,
    pub due_automations: usize,
    pub automations: Vec<SessionAutomationSnapshot>,
    pub pending: Vec<OnboardLaneItem>,
    pub running: Vec<OnboardLaneItem>,
    pub blocked: Vec<OnboardLaneItem>,
    pub completed: Vec<OnboardLaneItem>,
    pub failed: Vec<OnboardLaneItem>,
}

#[derive(Debug, Clone)]
pub struct SessionAutomationSnapshot {
    pub id: String,
    pub target_session_id: String,
    pub status: String,
    pub prompt_preview: String,
    pub interval_secs: u64,
    pub run_count: u32,
    pub max_runs: Option<u32>,
    pub run_for_secs: Option<u64>,
    pub remaining_runs: Option<u32>,
    pub remaining_secs: Option<u64>,
    pub next_run_at: Option<u64>,
    pub last_run_at: Option<u64>,
    pub last_batch_id: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonitorSessionSnapshot {
    pub id: String,
    pub title: String,
    pub role: String,
    pub group: Option<String>,
    pub task: Option<String>,
    pub job_id: Option<String>,
    pub status: String,
    pub work_state: String,
    pub pending_turns: usize,
    pub latest_batch_id: Option<u64>,
    pub latest_user_prompt: Option<String>,
    pub latest_response: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MonitorSnapshot {
    pub runtime_connected: bool,
    pub runtime_pid: Option<u32>,
    pub runtime_mode: Option<String>,
    pub runtime_command_label: Option<String>,
    pub total_codex_sessions: usize,
    pub active_codex_sessions: usize,
    pub queued_codex_sessions: usize,
    pub blocked_codex_sessions: usize,
    pub sessions: Vec<MonitorSessionSnapshot>,
}

pub fn job_intake_prompt(job: &JobRecord) -> String {
    service_job_intake_prompt(job)
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
        let state_file_fingerprint = file_fingerprint(&paths.state_file);
        let sessions = build_sessions(&state, &workspace_root);
        let client = Arc::new(
            AppServerClient::spawn(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                &config.master.reasoning_effort,
                paths.log_dir.clone(),
                config.logging.retention_days,
                config.logging.notification_channel_capacity,
            )
            .await?,
        );

        Ok(Self {
            workspace_root,
            config,
            paths,
            state: Arc::new(Mutex::new(state)),
            state_file_fingerprint: Arc::new(Mutex::new(state_file_fingerprint)),
            sessions: Arc::new(Mutex::new(sessions)),
            pending_turns: Arc::new(Mutex::new(BTreeMap::new())),
            active_turn_batches: Arc::new(Mutex::new(BTreeMap::new())),
            runtime_context: Arc::new(Mutex::new(None)),
            client,
        })
    }

    pub fn init_workspace(&self) -> Result<Option<PathBuf>> {
        let config_path = Config::write_default_config_if_missing(&self.workspace_root)?;
        self.paths.ensure_layout()?;
        self.maintain_logs()?;
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
            self.paths.log_dir.clone(),
            self.config.logging.retention_days,
            self.config.logging.notification_channel_capacity,
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

    pub fn sessions_overview_snapshot(&self) -> Vec<SessionOverviewSnapshot> {
        self.try_sync_state_from_disk();
        let sessions = self.sessions.lock().expect("sessions lock poisoned");
        let mut snapshots = sessions
            .values()
            .map(SessionView::overview)
            .collect::<Vec<_>>();
        drop(sessions);
        snapshots = self.annotate_session_overviews(snapshots);
        snapshots.push(self.onboard_session_overview());
        snapshots.sort_by(|left, right| {
            session_sort_key(&left.id)
                .cmp(&session_sort_key(&right.id))
                .then_with(|| left.title.cmp(&right.title))
        });
        snapshots
    }

    pub fn sessions_snapshot(&self) -> Vec<SessionSnapshot> {
        self.try_sync_state_from_disk();
        let sessions = self.sessions.lock().expect("sessions lock poisoned");
        let mut snapshots = sessions
            .values()
            .map(SessionView::snapshot)
            .collect::<Vec<_>>();
        drop(sessions);
        snapshots = self.annotate_session_snapshots(snapshots);
        snapshots.push(self.onboard_session_snapshot());
        snapshots.sort_by(|left, right| {
            session_sort_key(&left.id)
                .cmp(&session_sort_key(&right.id))
                .then_with(|| left.title.cmp(&right.title))
        });
        snapshots
    }

    pub fn session_snapshot(&self, session_id: &str) -> Option<SessionSnapshot> {
        self.try_sync_state_from_disk();
        if session_id == ONBOARD_SESSION_ID {
            return Some(self.onboard_session_snapshot());
        }
        let sessions = self.sessions.lock().expect("sessions lock poisoned");
        let snapshot = sessions.get(session_id).map(SessionView::snapshot)?;
        drop(sessions);
        Some(self.annotate_session_snapshot(snapshot))
    }

    pub fn monitor_snapshot(&self) -> MonitorSnapshot {
        self.try_sync_state_from_disk();
        let onboard = self.onboard_snapshot();
        let active_sessions = self
            .active_turn_batches
            .lock()
            .expect("active turn lock poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let queued_turns = self
            .pending_turns
            .lock()
            .expect("pending lock poisoned")
            .iter()
            .map(|(session_id, queue)| (session_id.clone(), queue.len()))
            .collect::<BTreeMap<_, _>>();

        let sessions = self
            .sessions_snapshot()
            .into_iter()
            .filter(|session| session.id != ONBOARD_SESSION_ID)
            .map(|session| {
                let session_id = session.id.clone();
                build_monitor_session_snapshot(
                    session,
                    active_sessions.iter().any(|active| active == &session_id),
                    queued_turns.get(&session_id).copied().unwrap_or(0),
                )
            })
            .collect::<Vec<_>>();

        MonitorSnapshot {
            runtime_connected: onboard.runtime_connected,
            runtime_pid: onboard.runtime_pid,
            runtime_mode: onboard.runtime_mode,
            runtime_command_label: onboard.runtime_command_label,
            total_codex_sessions: sessions.len(),
            active_codex_sessions: sessions
                .iter()
                .filter(|session| matches!(session.work_state.as_str(), "active" | "active+queued"))
                .count(),
            queued_codex_sessions: sessions
                .iter()
                .filter(|session| matches!(session.work_state.as_str(), "queued" | "active+queued"))
                .count(),
            blocked_codex_sessions: sessions
                .iter()
                .filter(|session| matches!(session.status.as_str(), "blocked" | "failed"))
                .count(),
            sessions,
        }
    }

    pub fn monitor_session_snapshot(&self, session_id: &str) -> Option<MonitorSessionSnapshot> {
        if session_id == ONBOARD_SESSION_ID {
            return None;
        }

        self.monitor_snapshot()
            .sessions
            .into_iter()
            .find(|session| session.id == session_id)
    }

    pub fn session_batch_ids(&self, session_id: &str) -> Vec<u64> {
        self.try_sync_state_from_disk();
        if session_id == ONBOARD_SESSION_ID {
            return Vec::new();
        }
        let state = self.state.lock().expect("state lock poisoned");
        let mut batch_ids = state
            .batches
            .values()
            .filter(|batch| {
                batch.root_session_id == session_id
                    || batch
                        .sessions
                        .iter()
                        .any(|candidate| candidate == session_id)
            })
            .map(|batch| batch.id)
            .collect::<Vec<_>>();
        batch_ids.sort_unstable();
        batch_ids.dedup();
        batch_ids
    }

    pub fn onboard_snapshot(&self) -> OnboardSnapshot {
        self.try_sync_state_from_disk();
        let now = now_unix_ts();
        let service = self.service_snapshot().ok().flatten();
        let persisted_runtime = self.runtime_snapshot().ok().flatten();
        let active_runtime = {
            let runtime = self.runtime_context.lock().expect("runtime lock poisoned");
            runtime.clone()
        };
        let (
            runtime_connected,
            runtime_pid,
            runtime_mode,
            runtime_command_label,
            active_turns,
            queued_turns,
            runtime_running_workers,
        ) = if let Some(context) = active_runtime {
            let runtime = self.client.runtime_snapshot();
            let active_turns = self
                .active_turn_batches
                .lock()
                .expect("active turn lock poisoned")
                .len();
            let queued_turns = self
                .pending_turns
                .lock()
                .expect("pending lock poisoned")
                .values()
                .map(VecDeque::len)
                .sum();
            (
                runtime.connected,
                runtime.pid,
                Some(context.mode),
                Some(context.command_label),
                active_turns,
                queued_turns,
                None,
            )
        } else if let Some(snapshot) = persisted_runtime {
            (
                snapshot.app_server_connected
                    && matches!(
                        snapshot.status,
                        ServiceLifecycle::Starting | ServiceLifecycle::Running
                    ),
                snapshot.app_server_pid,
                Some(snapshot.mode),
                Some(snapshot.command_label),
                snapshot.active_turns,
                snapshot.queued_turns,
                Some(snapshot.running_workers.len()),
            )
        } else {
            (false, None, None, None, 0, 0, None)
        };
        let state = self.state.lock().expect("state lock poisoned");

        let mut pending = Vec::new();
        let mut running = Vec::new();
        let mut blocked = Vec::new();
        let mut completed = Vec::new();
        let mut failed = Vec::new();
        let mut delegated_jobs = 0usize;
        let mut auto_approve_jobs = 0usize;
        let mut budget_exhausted_jobs = 0usize;
        let mut automations = state
            .session_automations
            .values()
            .map(|automation| session_automation_snapshot(automation, now))
            .collect::<Vec<_>>();
        automations.sort_by(|left, right| {
            session_automation_sort_key(left)
                .cmp(&session_automation_sort_key(right))
                .then_with(|| left.id.cmp(&right.id))
        });
        let armed_automations = automations
            .iter()
            .filter(|automation| automation.status == "armed")
            .count();
        let paused_automations = automations
            .iter()
            .filter(|automation| automation.status == "paused")
            .count();
        let due_automations = automations
            .iter()
            .filter(|automation| automation.status == "armed")
            .filter(|automation| {
                automation
                    .next_run_at
                    .is_some_and(|next_run_at| next_run_at <= now)
            })
            .count();

        for job in state.jobs.values() {
            let automation = job_automation_snapshot(job, now);
            if job.policy.delegate_to_master_loop {
                delegated_jobs += 1;
            }
            if job.policy.auto_approve {
                auto_approve_jobs += 1;
            }
            if matches!(
                automation.state.as_str(),
                "budget_exhausted_time" | "budget_exhausted_iterations"
            ) {
                budget_exhausted_jobs += 1;
            }

            let item = OnboardLaneItem {
                job_id: job.id.clone(),
                title: job.title.clone(),
                status: job.status.to_string(),
                operator_state: operator_state_for_job(job, &automation),
                summary: job
                    .latest_summary
                    .clone()
                    .unwrap_or_else(|| default_summary_for_job_status(&job.status).to_owned()),
                workers: job.worker_ids.len(),
                automation,
            };

            match job.status {
                JobStatus::Pending => pending.push(item),
                JobStatus::Running => running.push(item),
                JobStatus::Blocked => blocked.push(item),
                JobStatus::Completed => completed.push(item),
                JobStatus::Failed => failed.push(item),
            }
        }

        for lane in [
            &mut pending,
            &mut running,
            &mut blocked,
            &mut completed,
            &mut failed,
        ] {
            lane.sort_by(|left, right| left.job_id.cmp(&right.job_id));
        }

        let summary = format!(
            "{} pending | {} running | {} blocked | {} completed | {} failed",
            pending.len(),
            running.len(),
            blocked.len(),
            completed.len(),
            failed.len()
        );
        let status = if !blocked.is_empty() {
            "blocked"
        } else if !running.is_empty() || !pending.is_empty() {
            "running"
        } else if !failed.is_empty() && completed.is_empty() {
            "failed"
        } else if !completed.is_empty() {
            "completed"
        } else {
            "idle"
        }
        .to_owned();

        OnboardSnapshot {
            status,
            summary,
            service_status: service.as_ref().map(|snapshot| snapshot.status.to_string()),
            service_tick: service.as_ref().map(|snapshot| snapshot.tick),
            runtime_connected,
            runtime_pid,
            runtime_mode,
            runtime_command_label,
            active_turns,
            queued_turns,
            running_workers: service
                .as_ref()
                .map(|snapshot| snapshot.running_workers.len())
                .or(runtime_running_workers)
                .unwrap_or_else(|| {
                    state
                        .workers
                        .values()
                        .filter(|worker| {
                            matches!(
                                worker.status,
                                WorkerStatus::SpawnRequested
                                    | WorkerStatus::Bootstrapping
                                    | WorkerStatus::Running
                            )
                        })
                        .count()
                }),
            queued_deliveries: service
                .as_ref()
                .map(|snapshot| snapshot.queued_deliveries.len())
                .unwrap_or_else(|| {
                    state
                        .report_deliveries
                        .values()
                        .filter(|delivery| matches!(delivery.status, ReportDeliveryStatus::Queued))
                        .count()
                }),
            delegated_jobs,
            auto_approve_jobs,
            budget_exhausted_jobs,
            continued_jobs: service
                .map(|snapshot| snapshot.continued_jobs)
                .unwrap_or_default(),
            armed_automations,
            paused_automations,
            due_automations,
            automations,
            pending,
            running,
            blocked,
            completed,
            failed,
        }
    }

    fn onboard_session_snapshot(&self) -> SessionSnapshot {
        let onboard = self.onboard_snapshot();
        let mut session = SessionView::onboard(
            self.workspace_root.display().to_string(),
            Some(onboard.summary.clone()),
            Some(format!(
                "delegated={} auto={} exhausted={}",
                onboard.delegated_jobs, onboard.auto_approve_jobs, onboard.budget_exhausted_jobs
            )),
        );
        session.set_status(onboard.status.clone());
        session.set_lifecycle_note(Some(format!(
            "workers={} | queued deliveries={} | continued this tick={}",
            onboard.running_workers,
            onboard.queued_deliveries,
            onboard.continued_jobs.len()
        )));
        session.snapshot()
    }

    fn onboard_session_overview(&self) -> SessionOverviewSnapshot {
        let onboard = self.onboard_snapshot();
        let mut session = SessionView::onboard(
            self.workspace_root.display().to_string(),
            Some(onboard.summary.clone()),
            Some(format!(
                "delegated={} auto={} exhausted={}",
                onboard.delegated_jobs, onboard.auto_approve_jobs, onboard.budget_exhausted_jobs
            )),
        );
        session.set_status(onboard.status.clone());
        session.set_lifecycle_note(Some(format!(
            "workers={} | queued deliveries={} | continued this tick={}",
            onboard.running_workers,
            onboard.queued_deliveries,
            onboard.continued_jobs.len()
        )));
        session.overview()
    }

    fn annotate_session_overviews(
        &self,
        snapshots: Vec<SessionOverviewSnapshot>,
    ) -> Vec<SessionOverviewSnapshot> {
        snapshots
            .into_iter()
            .map(|snapshot| self.annotate_session_overview(snapshot))
            .collect()
    }

    fn annotate_session_overview(
        &self,
        mut snapshot: SessionOverviewSnapshot,
    ) -> SessionOverviewSnapshot {
        let Some(job_id) = snapshot.job_id.as_deref() else {
            return snapshot;
        };
        let state = self.state.lock().expect("state lock poisoned");
        let Some(job) = state.jobs.get(job_id) else {
            return snapshot;
        };
        let automation = job_automation_snapshot(job, now_unix_ts());
        let mut badges = Vec::new();
        if job.policy.delegate_to_master_loop {
            badges.push(format!("loop:{}", automation.state));
        }
        if job.policy.auto_approve {
            badges.push("auto-approve".to_owned());
        }
        if !badges.is_empty() {
            snapshot.subtitle = format!("{} | {}", badges.join(" | "), snapshot.subtitle);
        }
        snapshot
    }

    fn annotate_session_snapshots(&self, snapshots: Vec<SessionSnapshot>) -> Vec<SessionSnapshot> {
        snapshots
            .into_iter()
            .map(|snapshot| self.annotate_session_snapshot(snapshot))
            .collect()
    }

    fn annotate_session_snapshot(&self, mut snapshot: SessionSnapshot) -> SessionSnapshot {
        let Some(job_id) = snapshot.job_id.as_deref() else {
            return snapshot;
        };
        let state = self.state.lock().expect("state lock poisoned");
        let Some(job) = state.jobs.get(job_id) else {
            return snapshot;
        };
        let automation = job_automation_snapshot(job, now_unix_ts());
        let mut badges = Vec::new();
        if job.policy.delegate_to_master_loop {
            badges.push(format!("loop:{}", automation.state));
        }
        if job.policy.auto_approve {
            badges.push("auto-approve".to_owned());
        }
        if !badges.is_empty() {
            let note = badges.join(" | ");
            snapshot.subtitle = format!("{} | {}", note, snapshot.subtitle);
            if snapshot.lifecycle_note.is_none() {
                snapshot.lifecycle_note = Some(note);
            }
        }
        snapshot
    }

    pub fn list_workers(&self) -> Vec<WorkerRecord> {
        self.try_sync_state_from_disk();
        let state = self.state.lock().expect("state lock poisoned");
        state.workers.values().cloned().collect()
    }

    pub fn list_jobs(&self) -> Vec<JobRecord> {
        self.try_sync_state_from_disk();
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
        self.try_sync_state_from_disk();
        let state = self.state.lock().expect("state lock poisoned");
        let job = state.jobs.get(job_id).cloned()?;
        let automation = job_automation_snapshot(&job, now_unix_ts());

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

        let mut reports = state
            .reports
            .values()
            .filter(|report| report.job_id == job.id)
            .map(|report| JobReportSnapshot {
                id: report.id,
                kind: report.kind.to_string(),
                status: report.job_status.to_string(),
                summary: report.summary.clone(),
                body: report.body.clone(),
                created_at: report.created_at,
            })
            .collect::<Vec<_>>();
        reports.sort_by(|left, right| left.id.cmp(&right.id));
        if reports.len() > 12 {
            reports.drain(0..reports.len() - 12);
        }

        let mut subscriptions = state
            .report_subscriptions
            .values()
            .filter(|subscription| subscription.job_id == job.id)
            .map(|subscription| JobSubscriptionSnapshot {
                id: subscription.id,
                channel: subscription.channel.to_string(),
                target: subscription.target.clone(),
            })
            .collect::<Vec<_>>();
        subscriptions.sort_by(|left, right| left.id.cmp(&right.id));

        let mut deliveries = state
            .report_deliveries
            .values()
            .filter(|delivery| delivery.job_id == job.id)
            .map(|delivery| JobDeliverySnapshot {
                id: delivery.id,
                report_id: delivery.report_id,
                channel: delivery.channel.to_string(),
                target: delivery.target.clone(),
                status: delivery.status.to_string(),
                attempts: delivery.attempts,
                updated_at: delivery.updated_at,
                last_error: delivery.last_error.clone(),
            })
            .collect::<Vec<_>>();
        deliveries.sort_by(|left, right| left.id.cmp(&right.id));
        if deliveries.len() > 12 {
            deliveries.drain(0..deliveries.len() - 12);
        }

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
            auto_approve: job.policy.auto_approve,
            delegate_to_master_loop: job.policy.delegate_to_master_loop,
            continue_for_secs: job.policy.continue_for_secs,
            continue_max_iterations: job.policy.continue_max_iterations,
            created_at: job.created_at,
            updated_at: job.updated_at,
            latest_summary: job.latest_summary,
            latest_report_at: job.latest_report_at,
            next_report_due_at: job.next_report_due_at,
            escalation_state: job.escalation_state,
            final_outcome: job.final_outcome,
            automation,
            context: job.context,
            batch_ids: batches,
            workers,
            reports,
            subscriptions,
            deliveries,
        })
    }

    pub fn list_session_automations(&self) -> Vec<SessionAutomationSnapshot> {
        self.try_sync_state_from_disk();
        let now = now_unix_ts();
        let state = self.state.lock().expect("state lock poisoned");
        let mut automations = state
            .session_automations
            .values()
            .map(|automation| session_automation_snapshot(automation, now))
            .collect::<Vec<_>>();
        automations.sort_by(|left, right| {
            session_automation_sort_key(left)
                .cmp(&session_automation_sort_key(right))
                .then_with(|| left.id.cmp(&right.id))
        });
        automations
    }

    pub fn session_automation_snapshot(
        &self,
        automation_id: &str,
    ) -> Option<SessionAutomationSnapshot> {
        self.try_sync_state_from_disk();
        let now = now_unix_ts();
        let state = self.state.lock().expect("state lock poisoned");
        state
            .session_automations
            .get(automation_id)
            .map(|automation| session_automation_snapshot(automation, now))
    }

    pub fn create_session_automation(
        &self,
        request: CreateSessionAutomationRequest,
    ) -> Result<SessionAutomationRecord> {
        let target_session_id = request.target_session_id.trim();
        if target_session_id.is_empty() {
            anyhow::bail!("automation target must not be empty");
        }
        if target_session_id == ONBOARD_SESSION_ID {
            anyhow::bail!("`onboard` is a virtual supervisor session and cannot be automated");
        }

        let prompt = request.prompt.trim();
        if prompt.is_empty() {
            anyhow::bail!("automation prompt must not be empty");
        }
        if request.interval_secs == 0 {
            anyhow::bail!("automation interval must be at least 1 second");
        }
        if request.max_runs == Some(0) {
            anyhow::bail!("automation max runs must be greater than zero");
        }
        if request.run_for_secs == Some(0) {
            anyhow::bail!("automation run-for-secs must be greater than zero");
        }

        self.ensure_session_automation_target(target_session_id)?;

        let now = now_unix_ts();
        let automation = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let automation_number = state.next_session_automation_number;
            state.next_session_automation_number += 1;
            let automation = SessionAutomationRecord {
                id: format!("AUTO-{automation_number:03}"),
                target_session_id: target_session_id.to_owned(),
                prompt: prompt.to_owned(),
                interval_secs: request.interval_secs,
                max_runs: request.max_runs,
                run_for_secs: request.run_for_secs,
                status: SessionAutomationStatus::Armed,
                created_at: now,
                updated_at: now,
                started_at: Some(now),
                next_run_at: Some(now),
                last_run_at: None,
                run_count: 0,
                last_error: None,
                last_batch_id: None,
            };
            state
                .session_automations
                .insert(automation.id.clone(), automation.clone());
            automation
        };
        self.save_state()?;
        Ok(automation)
    }

    pub fn pause_session_automation(&self, automation_id: &str) -> Result<SessionAutomationRecord> {
        self.update_session_automation_status(automation_id, SessionAutomationStatus::Paused)
    }

    pub fn resume_session_automation(
        &self,
        automation_id: &str,
    ) -> Result<SessionAutomationRecord> {
        let target_session_id = {
            let state = self.state.lock().expect("state lock poisoned");
            state
                .session_automations
                .get(automation_id)
                .with_context(|| format!("unknown automation `{automation_id}`"))?
                .target_session_id
                .clone()
        };
        self.ensure_session_automation_target(&target_session_id)?;
        let automation = {
            let now = now_unix_ts();
            let mut state = self.state.lock().expect("state lock poisoned");
            let automation = state
                .session_automations
                .get_mut(automation_id)
                .with_context(|| format!("unknown automation `{automation_id}`"))?;
            automation.status = SessionAutomationStatus::Armed;
            automation.updated_at = now;
            automation.next_run_at = Some(now);
            automation.last_error = None;
            automation.clone()
        };
        self.save_state()?;
        Ok(automation)
    }

    pub fn cancel_session_automation(
        &self,
        automation_id: &str,
    ) -> Result<SessionAutomationRecord> {
        self.update_session_automation_status(automation_id, SessionAutomationStatus::Cancelled)
    }

    fn ensure_session_automation_target(&self, session_id: &str) -> Result<()> {
        if session_id == MASTER_SESSION_ID {
            return Ok(());
        }
        if session_id == ONBOARD_SESSION_ID {
            anyhow::bail!("`onboard` cannot receive automated prompts");
        }

        let sessions = self.sessions_overview_snapshot();
        if sessions.iter().any(|session| session.id == session_id) {
            Ok(())
        } else {
            anyhow::bail!("unknown session `{session_id}`")
        }
    }

    fn update_session_automation_status(
        &self,
        automation_id: &str,
        status: SessionAutomationStatus,
    ) -> Result<SessionAutomationRecord> {
        let automation = {
            let now = now_unix_ts();
            let mut state = self.state.lock().expect("state lock poisoned");
            let automation = state
                .session_automations
                .get_mut(automation_id)
                .with_context(|| format!("unknown automation `{automation_id}`"))?;
            automation.status = status;
            automation.updated_at = now;
            automation.next_run_at = None;
            automation.clone()
        };
        self.save_state()?;
        Ok(automation)
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
                    auto_approve: request.auto_approve,
                    delegate_to_master_loop: request.delegate_to_master_loop,
                    continue_for_secs: request.continue_for_secs,
                    continue_max_iterations: request.continue_max_iterations,
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
                automation_started_at: None,
                last_continue_at: None,
                continue_iterations: 0,
            };
            state.jobs.insert(job.id.clone(), job.clone());
            job
        };

        self.save_state()?;
        self.ensure_default_report_subscription(&job)?;
        self.emit_job_report(
            &job.id,
            JobReportKind::Accepted,
            format!("job accepted: {}", job.title),
            render_job_report_body(&job, JobReportKind::Accepted, job.latest_summary.as_deref()),
        )?;
        Ok(job)
    }

    pub fn add_report_subscription(
        &self,
        job_id: &str,
        channel: ReportChannel,
        target: Option<String>,
    ) -> Result<ReportSubscriptionRecord> {
        self.ensure_job_exists(job_id)?;

        let resolved_target = target
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| gateway::default_target_for_channel(&channel, &self.paths.root));
        let now = now_unix_ts();

        let subscription = {
            let mut state = self.state.lock().expect("state lock poisoned");
            if let Some(existing) = state
                .report_subscriptions
                .values()
                .find(|subscription| {
                    subscription.job_id == job_id
                        && subscription.channel == channel
                        && subscription.target == resolved_target
                })
                .cloned()
            {
                existing
            } else {
                let subscription_id = state.next_report_subscription_number;
                state.next_report_subscription_number += 1;
                let subscription = ReportSubscriptionRecord {
                    id: subscription_id,
                    job_id: job_id.to_owned(),
                    channel,
                    target: resolved_target,
                    notify_on_accepted: true,
                    notify_on_progress: true,
                    notify_on_blocker: true,
                    notify_on_completion: true,
                    notify_on_failure: true,
                    notify_on_digest: true,
                    created_at: now,
                    updated_at: now,
                };
                state
                    .report_subscriptions
                    .insert(subscription_id, subscription.clone());
                subscription
            }
        };

        self.save_state()?;
        Ok(subscription)
    }

    pub fn service_snapshot(&self) -> Result<Option<ServiceSnapshot>> {
        ServiceSnapshot::load(&self.service_file())
    }

    pub fn runtime_snapshot(&self) -> Result<Option<RuntimeSnapshot>> {
        RuntimeSnapshot::load(&self.runtime_file())
    }

    pub fn begin_runtime_session(
        &self,
        mode: impl Into<String>,
        command_label: impl Into<String>,
    ) -> Result<()> {
        let context = RuntimeContext {
            mode: mode.into(),
            command_label: command_label.into(),
            started_at: now_unix_ts(),
        };
        {
            let mut runtime = self.runtime_context.lock().expect("runtime lock poisoned");
            *runtime = Some(context);
        }
        self.refresh_runtime_snapshot()
    }

    pub fn finish_runtime_session(
        &self,
        lifecycle: ServiceLifecycle,
        last_error: Option<String>,
    ) -> Result<()> {
        let context = {
            let runtime = self.runtime_context.lock().expect("runtime lock poisoned");
            runtime.clone()
        };
        let Some(context) = context else {
            return Ok(());
        };

        let snapshot = self.build_runtime_snapshot(&context, lifecycle, last_error);
        snapshot.write(&self.runtime_file())?;

        let mut runtime = self.runtime_context.lock().expect("runtime lock poisoned");
        *runtime = None;
        Ok(())
    }

    pub fn write_service_lifecycle(
        &self,
        lifecycle: ServiceLifecycle,
        started_at: u64,
        tick: u64,
        dispatched_jobs: Vec<String>,
        last_error: Option<String>,
    ) -> Result<()> {
        let snapshot = self.build_service_snapshot(
            lifecycle,
            started_at,
            tick,
            900,
            dispatched_jobs,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            last_error,
        );
        snapshot.write(&self.service_file())
    }

    pub async fn service_tick(
        &self,
        started_at: u64,
        tick: u64,
        stall_after_secs: u64,
    ) -> Result<ServiceSnapshot> {
        self.reconcile_jobs()?;

        let pending_jobs = self.pending_service_jobs();
        let has_live_jobs = !pending_jobs.is_empty()
            || self
                .list_jobs()
                .into_iter()
                .any(|job| matches!(job.status, JobStatus::Running | JobStatus::Blocked));

        if has_live_jobs {
            let _ = self.ensure_master_thread().await?;
        }

        let mut dispatched_jobs = Vec::new();
        for job in pending_jobs {
            let prompt = service_job_intake_prompt(&job);
            self.submit_prompt_for_job(PromptTarget::Master, &prompt, Some(&job.id))
                .await?;
            dispatched_jobs.push(job.id);
        }

        self.reconcile_jobs()?;
        let continued_jobs = self.continue_automated_jobs().await?;
        let _continued_session_automations = self.continue_session_automations().await?;
        let generated_reports = self.emit_due_job_reports()?;
        let delivered_notifications = self.deliver_queued_reports()?;
        let snapshot = self.build_service_snapshot(
            ServiceLifecycle::Running,
            started_at,
            tick,
            stall_after_secs,
            dispatched_jobs,
            continued_jobs,
            generated_reports,
            delivered_notifications,
            None,
        );
        snapshot.write(&self.service_file())?;
        Ok(snapshot)
    }

    pub fn batch_snapshot(&self, batch_id: u64) -> Option<BatchSnapshot> {
        self.try_sync_state_from_disk();
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
        self.submit_prompt_for_job_with_batch(target, prompt, job_id)
            .await
            .map(|_| ())
    }

    pub async fn submit_prompt_for_job_with_batch(
        &self,
        target: PromptTarget,
        prompt: &str,
        job_id: Option<&str>,
    ) -> Result<u64> {
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
        )?;
        Ok(batch_id)
    }

    pub async fn submit_prompt_and_wait(&self, target: PromptTarget, prompt: &str) -> Result<()> {
        self.submit_prompt_and_wait_for_job(target, prompt, None)
            .await
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

    pub async fn wait_for_batch_completion(&self, batch_id: u64) -> Result<()> {
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
        let (_, worker) = self
            .spawn_worker_and_wait_for_job_with_batch(group, task, job_id)
            .await?;
        Ok(worker)
    }

    pub async fn spawn_worker_and_wait_for_job_with_batch(
        &self,
        group: &str,
        task: &str,
        job_id: Option<&str>,
    ) -> Result<(u64, WorkerRecord)> {
        let batch_id = self.allocate_batch_id()?;
        self.register_batch(
            MASTER_SESSION_ID,
            batch_id,
            &format!("spawn worker [{group}] {task}"),
            job_id,
        )?;
        let worker = self
            .spawn_worker_with_options(group, task, None, None, true, batch_id)
            .await?;
        Ok((batch_id, worker))
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
            render_task_file(
                task_number,
                task,
                &group_config.lease_paths,
                linked_job.as_ref(),
            ),
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
            let notification = match receiver.recv().await {
                Ok(notification) => notification,
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let warning =
                        format!("app-server notification stream lagged; skipped {skipped} events");
                    self.append_log_line(&session_id, format!("warn> {warning}"))?;
                    self.append_session_event(
                        &session_id,
                        SessionEventKind::Error,
                        warning.clone(),
                        Some(batch_id),
                    )?;
                    self.log_runtime_event(
                        "warn",
                        "app-server notification stream lagged",
                        Some(&session_id),
                        json!({
                            "thread_id": thread_id,
                            "turn_id": turn_id,
                            "skipped": skipped,
                            "log_label": log_label,
                        }),
                    )?;
                    continue;
                }
                Err(error) => return Err(map_broadcast_error(error)),
            };
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
        {
            let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
            if let Some(session) = sessions.get_mut(session_id) {
                session.set_pending_turns(pending_turns);
            }
        }
        self.refresh_runtime_snapshot()
    }

    fn set_active_turn_batch(&self, session_id: &str, batch_id: u64) -> Result<()> {
        {
            let mut active = self
                .active_turn_batches
                .lock()
                .expect("active turn lock poisoned");
            active.insert(session_id.to_owned(), batch_id);
        }
        self.touch_batch(batch_id, session_id, None)?;
        self.refresh_runtime_snapshot()
    }

    fn clear_active_turn_batch(&self, session_id: &str, batch_id: u64) -> Result<()> {
        {
            let mut active = self
                .active_turn_batches
                .lock()
                .expect("active turn lock poisoned");
            if active.get(session_id).copied() == Some(batch_id) {
                active.remove(session_id);
            }
        }
        self.refresh_runtime_snapshot()
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
        {
            let state = self.state.lock().expect("state lock poisoned");
            state.save(&self.paths.state_file)?;
        }
        let mut fingerprint = self
            .state_file_fingerprint
            .lock()
            .expect("state fingerprint lock poisoned");
        *fingerprint = file_fingerprint(&self.paths.state_file);
        Ok(())
    }

    fn try_sync_state_from_disk(&self) {
        let next_fingerprint = file_fingerprint(&self.paths.state_file);
        {
            let current = self
                .state_file_fingerprint
                .lock()
                .expect("state fingerprint lock poisoned");
            if *current == next_fingerprint {
                return;
            }
        }
        let Ok(state) = AppState::load(&self.paths.state_file) else {
            return;
        };
        let mut sessions = build_sessions(&state, &self.workspace_root);
        let pending_turns = {
            let pending = self.pending_turns.lock().expect("pending lock poisoned");
            pending
                .iter()
                .map(|(session_id, queue)| (session_id.clone(), queue.len()))
                .collect::<Vec<_>>()
        };
        for (session_id, pending_turns) in pending_turns {
            if let Some(session) = sessions.get_mut(&session_id) {
                session.set_pending_turns(pending_turns);
            }
        }

        {
            let mut current_state = self.state.lock().expect("state lock poisoned");
            *current_state = state;
        }
        let mut current_sessions = self.sessions.lock().expect("sessions lock poisoned");
        *current_sessions = sessions;
        let mut current_fingerprint = self
            .state_file_fingerprint
            .lock()
            .expect("state fingerprint lock poisoned");
        *current_fingerprint = next_fingerprint;
    }

    fn service_file(&self) -> PathBuf {
        self.paths.root.join("service.json")
    }

    fn runtime_file(&self) -> PathBuf {
        self.paths.root.join("runtime.json")
    }

    fn refresh_runtime_snapshot(&self) -> Result<()> {
        let context = {
            let runtime = self.runtime_context.lock().expect("runtime lock poisoned");
            runtime.clone()
        };
        let Some(context) = context else {
            return Ok(());
        };

        let snapshot = self.build_runtime_snapshot(&context, ServiceLifecycle::Running, None);
        snapshot.write(&self.runtime_file())
    }

    fn maintain_logs(&self) -> Result<()> {
        let entry = json!({
            "ts": now_unix_ts(),
            "level": "info",
            "source": "controller",
            "message": "log maintenance",
        });
        let _ = logging::append_jsonl(
            &self.paths.log_dir,
            self.config.logging.retention_days,
            "runtime/maintenance",
            &entry,
        )?;
        Ok(())
    }

    fn log_runtime_event(
        &self,
        level: &str,
        message: &str,
        session_id: Option<&str>,
        fields: Value,
    ) -> Result<()> {
        let entry = json!({
            "ts": now_unix_ts(),
            "level": level,
            "source": "controller",
            "session_id": session_id,
            "message": message,
            "fields": fields,
        });
        logging::append_jsonl(
            &self.paths.log_dir,
            self.config.logging.retention_days,
            "runtime/controller",
            &entry,
        )?;
        Ok(())
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
        let (previous_status, previous_summary, next_status, next_summary, report_job, changed) = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let (batch_ids, worker_ids, existing_summary, previous_status, job_record) = {
                let job = state
                    .jobs
                    .get(job_id)
                    .with_context(|| format!("unknown job `{job_id}`"))?;
                (
                    job.batch_ids.clone(),
                    job.worker_ids.clone(),
                    job.latest_summary.clone(),
                    job.status.clone(),
                    job.clone(),
                )
            };

            let next_summary = latest_summary.or(existing_summary);
            let next_status = derive_job_status_from_state(
                &state,
                &batch_ids,
                &worker_ids,
                next_summary.as_deref(),
            );
            let changed = previous_status != next_status
                || next_summary.as_deref().map(str::trim)
                    != job_record.latest_summary.as_deref().map(str::trim);
            let mut report_job = job_record.clone();
            report_job.status = next_status.clone();
            if next_summary.is_some() {
                report_job.latest_summary = next_summary.clone();
            }
            if let Some(job) = state.jobs.get_mut(job_id) {
                job.status = next_status.clone();
                if changed {
                    job.updated_at = now_unix_ts();
                }
                if next_summary.is_some() {
                    job.latest_summary = next_summary.clone();
                }
                job.final_outcome = match next_status {
                    JobStatus::Completed | JobStatus::Failed => job.latest_summary.clone(),
                    _ => None,
                };
            }
            (
                previous_status,
                job_record.latest_summary.clone(),
                next_status,
                next_summary,
                report_job,
                changed,
            )
        };

        if changed {
            self.save_state()?;
        }

        if let Some(report_kind) = report_kind_for_job_update(
            &previous_status,
            previous_summary.as_deref(),
            &next_status,
            next_summary.as_deref(),
        ) {
            let report_summary = report_summary_for_job_update(
                &report_job,
                &report_kind,
                next_summary.as_deref(),
                &next_status,
            );
            let report_body =
                render_job_report_body(&report_job, report_kind.clone(), next_summary.as_deref());
            self.emit_job_report(job_id, report_kind, report_summary, report_body)?;
        }

        Ok(())
    }

    fn reconcile_jobs(&self) -> Result<()> {
        let job_ids = {
            let state = self.state.lock().expect("state lock poisoned");
            state.jobs.keys().cloned().collect::<Vec<_>>()
        };
        for job_id in job_ids {
            self.refresh_job_tracking(&job_id, None)?;
        }
        Ok(())
    }

    fn pending_service_jobs(&self) -> Vec<JobRecord> {
        let state = self.state.lock().expect("state lock poisoned");
        let mut jobs = state
            .jobs
            .values()
            .filter(|job| matches!(job.status, JobStatus::Pending) && job.batch_ids.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        jobs
    }

    async fn continue_automated_jobs(&self) -> Result<Vec<String>> {
        if self.session_is_busy(MASTER_SESSION_ID) {
            return Ok(Vec::new());
        }

        let candidate = {
            let now = now_unix_ts();
            let state = self.state.lock().expect("state lock poisoned");
            state
                .jobs
                .values()
                .filter(|job| job_is_eligible_for_master_loop(job, now))
                .filter(|job| !job_has_active_workers(&state, job))
                .min_by(|left, right| {
                    left.last_continue_at
                        .unwrap_or(0)
                        .cmp(&right.last_continue_at.unwrap_or(0))
                        .then_with(|| left.updated_at.cmp(&right.updated_at))
                        .then_with(|| left.id.cmp(&right.id))
                })
                .cloned()
        };

        let Some(job) = candidate else {
            return Ok(Vec::new());
        };

        let automation = job_automation_snapshot(&job, now_unix_ts());
        let prompt = service_job_continue_prompt(&job, &automation);
        self.mark_job_continue_dispatch(&job.id)?;
        self.submit_prompt_for_job(PromptTarget::Master, &prompt, Some(&job.id))
            .await?;
        Ok(vec![job.id])
    }

    async fn continue_session_automations(&self) -> Result<Vec<String>> {
        let now = now_unix_ts();
        let candidates = {
            let state = self.state.lock().expect("state lock poisoned");
            let mut automations = state
                .session_automations
                .values()
                .filter(|automation| session_automation_is_due(automation, now))
                .cloned()
                .collect::<Vec<_>>();
            automations.sort_by(|left, right| {
                left.next_run_at
                    .unwrap_or(0)
                    .cmp(&right.next_run_at.unwrap_or(0))
                    .then_with(|| left.id.cmp(&right.id))
            });
            automations
        };

        let mut dispatched = Vec::new();
        for automation in candidates {
            if session_automation_is_exhausted(&automation, now) {
                self.complete_session_automation(&automation.id)?;
                continue;
            }

            if let Err(error) = self.ensure_session_automation_target(&automation.target_session_id)
            {
                self.fail_session_automation(&automation.id, error.to_string())?;
                continue;
            }

            if self.session_is_busy(&automation.target_session_id) {
                continue;
            }

            let target = automation_prompt_target(&automation.target_session_id);
            match self
                .submit_prompt_for_job_with_batch(target, &automation.prompt, None)
                .await
            {
                Ok(batch_id) => {
                    self.mark_session_automation_dispatch(&automation.id, batch_id)?;
                    dispatched.push(automation.id);
                }
                Err(error) => {
                    self.fail_session_automation(&automation.id, error.to_string())?;
                }
            }
        }

        Ok(dispatched)
    }

    fn mark_job_continue_dispatch(&self, job_id: &str) -> Result<()> {
        let now = now_unix_ts();
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let job = state
                .jobs
                .get_mut(job_id)
                .with_context(|| format!("unknown job `{job_id}`"))?;
            if job.automation_started_at.is_none() {
                job.automation_started_at = Some(now);
            }
            job.last_continue_at = Some(now);
            job.continue_iterations += 1;
            job.updated_at = now;
            job.latest_summary = Some(format!(
                "onboard delegated master loop pass {}",
                job.continue_iterations
            ));
        }
        self.save_state()
    }

    fn mark_session_automation_dispatch(&self, automation_id: &str, batch_id: u64) -> Result<()> {
        let now = now_unix_ts();
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let automation = state
                .session_automations
                .get_mut(automation_id)
                .with_context(|| format!("unknown automation `{automation_id}`"))?;
            automation.updated_at = now;
            automation.last_run_at = Some(now);
            automation.last_batch_id = Some(batch_id);
            automation.last_error = None;
            automation.run_count += 1;
            if session_automation_is_exhausted(automation, now) {
                automation.status = SessionAutomationStatus::Completed;
                automation.next_run_at = None;
            } else {
                automation.status = SessionAutomationStatus::Armed;
                automation.next_run_at = Some(now.saturating_add(automation.interval_secs));
            }
        }
        self.save_state()
    }

    fn complete_session_automation(&self, automation_id: &str) -> Result<()> {
        let now = now_unix_ts();
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let automation = state
                .session_automations
                .get_mut(automation_id)
                .with_context(|| format!("unknown automation `{automation_id}`"))?;
            automation.status = SessionAutomationStatus::Completed;
            automation.updated_at = now;
            automation.next_run_at = None;
        }
        self.save_state()
    }

    fn fail_session_automation(&self, automation_id: &str, error: String) -> Result<()> {
        let now = now_unix_ts();
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            let automation = state
                .session_automations
                .get_mut(automation_id)
                .with_context(|| format!("unknown automation `{automation_id}`"))?;
            automation.status = SessionAutomationStatus::Failed;
            automation.updated_at = now;
            automation.next_run_at = None;
            automation.last_error = Some(error);
        }
        self.save_state()
    }

    fn emit_due_job_reports(&self) -> Result<Vec<String>> {
        let due_jobs = {
            let state = self.state.lock().expect("state lock poisoned");
            let now = now_unix_ts();
            let mut jobs = state
                .jobs
                .values()
                .filter(|job| {
                    matches!(job.status, JobStatus::Running | JobStatus::Blocked)
                        && job.next_report_due_at.is_some_and(|due_at| due_at <= now)
                })
                .cloned()
                .collect::<Vec<_>>();
            jobs.sort_by(|left, right| {
                left.next_report_due_at
                    .cmp(&right.next_report_due_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            jobs
        };

        let mut emitted = Vec::new();
        for job in due_jobs {
            let summary = report_summary_for_job_update(
                &job,
                &JobReportKind::Digest,
                job.latest_summary.as_deref(),
                &job.status,
            );
            let body =
                render_job_report_body(&job, JobReportKind::Digest, job.latest_summary.as_deref());
            let report = self.emit_job_report(&job.id, JobReportKind::Digest, summary, body)?;
            emitted.push(format!("RPT-{:03}", report.id));
        }

        Ok(emitted)
    }

    fn ensure_default_report_subscription(&self, job: &JobRecord) -> Result<()> {
        let mut state = self.state.lock().expect("state lock poisoned");
        if state
            .report_subscriptions
            .values()
            .any(|subscription| subscription.job_id == job.id)
        {
            return Ok(());
        }

        let subscription_id = state.next_report_subscription_number;
        state.next_report_subscription_number += 1;
        let now = now_unix_ts();
        state.report_subscriptions.insert(
            subscription_id,
            ReportSubscriptionRecord {
                id: subscription_id,
                job_id: job.id.clone(),
                channel: ReportChannel::Console,
                target: gateway::default_target_for_channel(
                    &ReportChannel::Console,
                    &self.paths.root,
                ),
                notify_on_accepted: true,
                notify_on_progress: true,
                notify_on_blocker: true,
                notify_on_completion: true,
                notify_on_failure: true,
                notify_on_digest: true,
                created_at: now,
                updated_at: now,
            },
        );
        drop(state);
        self.save_state()
    }

    fn job_for_batch(&self, batch_id: u64) -> Option<JobRecord> {
        let state = self.state.lock().expect("state lock poisoned");
        let job_id = state.batches.get(&batch_id)?.job_id.clone()?;
        state.jobs.get(&job_id).cloned()
    }

    fn emit_job_report(
        &self,
        job_id: &str,
        kind: JobReportKind,
        summary: String,
        body: String,
    ) -> Result<JobReportRecord> {
        let now = now_unix_ts();
        let (report, subscriptions) = {
            let mut state = self.state.lock().expect("state lock poisoned");
            let (job_status, report_id) = {
                let job = state
                    .jobs
                    .get(job_id)
                    .with_context(|| format!("unknown job `{job_id}`"))?;
                (job.status.clone(), state.next_report_number)
            };
            state.next_report_number += 1;
            let report = JobReportRecord {
                id: report_id,
                job_id: job_id.to_owned(),
                kind: kind.clone(),
                job_status: job_status.clone(),
                summary,
                body,
                created_at: now,
            };
            state.reports.insert(report_id, report.clone());
            trim_reports_for_job(&mut state.reports, job_id);
            let subscriptions = state
                .report_subscriptions
                .values()
                .filter(|subscription| {
                    subscription.job_id == job_id
                        && subscription_accepts_report_kind(subscription, &kind)
                })
                .cloned()
                .collect::<Vec<_>>();
            if let Some(job) = state.jobs.get_mut(job_id) {
                job.latest_report_at = Some(now);
                job.next_report_due_at = match kind {
                    JobReportKind::Completion | JobReportKind::Failure => None,
                    _ => Some(now + DEFAULT_REPORT_CADENCE_SECS),
                };
            }
            (report, subscriptions)
        };
        self.enqueue_report_deliveries(&report, &subscriptions)?;
        self.save_state()?;
        Ok(report)
    }

    fn enqueue_report_deliveries(
        &self,
        report: &JobReportRecord,
        subscriptions: &[ReportSubscriptionRecord],
    ) -> Result<()> {
        if subscriptions.is_empty() {
            return Ok(());
        }

        let mut state = self.state.lock().expect("state lock poisoned");
        let now = now_unix_ts();
        for subscription in subscriptions {
            let delivery_id = state.next_report_delivery_number;
            state.next_report_delivery_number += 1;
            state.report_deliveries.insert(
                delivery_id,
                ReportDeliveryRecord {
                    id: delivery_id,
                    report_id: report.id,
                    job_id: report.job_id.clone(),
                    channel: subscription.channel.clone(),
                    target: subscription.target.clone(),
                    status: ReportDeliveryStatus::Queued,
                    attempts: 0,
                    created_at: now,
                    updated_at: now,
                    last_error: None,
                },
            );
        }
        trim_deliveries_for_job(&mut state.report_deliveries, &report.job_id);
        drop(state);
        self.save_state()
    }

    fn deliver_queued_reports(&self) -> Result<Vec<String>> {
        let deliveries = {
            let state = self.state.lock().expect("state lock poisoned");
            let mut deliveries = state
                .report_deliveries
                .values()
                .filter(|delivery| matches!(delivery.status, ReportDeliveryStatus::Queued))
                .cloned()
                .collect::<Vec<_>>();
            deliveries.sort_by(|left, right| left.id.cmp(&right.id));
            deliveries
        };

        let mut delivered_messages = Vec::new();
        let mut state_changed = false;
        for delivery in deliveries {
            let Some(report) = ({
                let state = self.state.lock().expect("state lock poisoned");
                state.reports.get(&delivery.report_id).cloned()
            }) else {
                let mut state = self.state.lock().expect("state lock poisoned");
                let Some(stored_delivery) = state.report_deliveries.get_mut(&delivery.id) else {
                    continue;
                };
                stored_delivery.status = ReportDeliveryStatus::Failed;
                stored_delivery.updated_at = now_unix_ts();
                stored_delivery.attempts += 1;
                stored_delivery.last_error = Some("missing report".to_owned());
                state_changed = true;
                continue;
            };

            match gateway::deliver_report(
                &delivery.channel,
                &delivery.target,
                &self.paths.root,
                &report,
            ) {
                Ok(delivered_to) => {
                    let mut state = self.state.lock().expect("state lock poisoned");
                    let Some(stored_delivery) = state.report_deliveries.get_mut(&delivery.id)
                    else {
                        continue;
                    };
                    stored_delivery.status = ReportDeliveryStatus::Delivered;
                    stored_delivery.updated_at = now_unix_ts();
                    stored_delivery.attempts += 1;
                    stored_delivery.last_error = None;
                    state_changed = true;
                    delivered_messages.push(format_report_delivery_message(&report, &delivered_to));
                }
                Err(error) => {
                    let mut state = self.state.lock().expect("state lock poisoned");
                    let Some(stored_delivery) = state.report_deliveries.get_mut(&delivery.id)
                    else {
                        continue;
                    };
                    stored_delivery.status = ReportDeliveryStatus::Failed;
                    stored_delivery.updated_at = now_unix_ts();
                    stored_delivery.attempts += 1;
                    stored_delivery.last_error = Some(error.to_string());
                    state_changed = true;
                }
            };
        }

        if state_changed {
            self.save_state()?;
        }
        Ok(delivered_messages)
    }

    fn build_service_snapshot(
        &self,
        lifecycle: ServiceLifecycle,
        started_at: u64,
        tick: u64,
        stall_after_secs: u64,
        dispatched_jobs: Vec<String>,
        continued_jobs: Vec<String>,
        generated_reports: Vec<String>,
        delivered_notifications: Vec<String>,
        last_error: Option<String>,
    ) -> ServiceSnapshot {
        let now = now_unix_ts();
        let current_pid = std::process::id();
        let state = self.state.lock().expect("state lock poisoned");

        let mut pending_jobs = Vec::new();
        let mut running_jobs = Vec::new();
        let mut blocked_jobs = Vec::new();
        let mut completed_jobs = Vec::new();
        let mut failed_jobs = Vec::new();
        let mut stalled_jobs = Vec::new();
        let mut delegated_jobs = Vec::new();
        let mut auto_approve_jobs = Vec::new();
        let mut budget_exhausted_jobs = Vec::new();
        let mut queued_deliveries = state
            .report_deliveries
            .values()
            .filter(|delivery| matches!(delivery.status, ReportDeliveryStatus::Queued))
            .map(|delivery| format!("DLY-{:03}", delivery.id))
            .collect::<Vec<_>>();

        for job in state.jobs.values() {
            match job.status {
                JobStatus::Pending => pending_jobs.push(job.id.clone()),
                JobStatus::Running => running_jobs.push(job.id.clone()),
                JobStatus::Blocked => blocked_jobs.push(job.id.clone()),
                JobStatus::Completed => completed_jobs.push(job.id.clone()),
                JobStatus::Failed => failed_jobs.push(job.id.clone()),
            }
            if matches!(job.status, JobStatus::Running)
                && now.saturating_sub(job.updated_at) >= stall_after_secs
            {
                stalled_jobs.push(job.id.clone());
            }
            if job.policy.delegate_to_master_loop {
                delegated_jobs.push(job.id.clone());
            }
            if job.policy.auto_approve {
                auto_approve_jobs.push(job.id.clone());
            }
            if matches!(
                job_automation_snapshot(job, now).state.as_str(),
                "budget_exhausted_time" | "budget_exhausted_iterations"
            ) {
                budget_exhausted_jobs.push(job.id.clone());
            }
        }

        let mut running_workers = state
            .workers
            .values()
            .filter(|worker| {
                matches!(
                    worker.status,
                    WorkerStatus::SpawnRequested
                        | WorkerStatus::Bootstrapping
                        | WorkerStatus::Running
                )
            })
            .map(|worker| worker.id.clone())
            .collect::<Vec<_>>();

        pending_jobs.sort();
        running_jobs.sort();
        blocked_jobs.sort();
        completed_jobs.sort();
        failed_jobs.sort();
        stalled_jobs.sort();
        running_workers.sort();
        queued_deliveries.sort();
        delegated_jobs.sort();
        auto_approve_jobs.sort();
        budget_exhausted_jobs.sort();

        ServiceSnapshot {
            status: lifecycle,
            pid: current_pid,
            started_at,
            updated_at: now,
            tick,
            master_thread_id: state.master_thread_id.clone(),
            pending_jobs,
            running_jobs,
            blocked_jobs,
            completed_jobs,
            failed_jobs,
            stalled_jobs,
            running_workers,
            dispatched_jobs,
            continued_jobs,
            generated_reports,
            queued_deliveries,
            delivered_notifications,
            delegated_jobs,
            auto_approve_jobs,
            budget_exhausted_jobs,
            last_error,
        }
    }

    fn build_runtime_snapshot(
        &self,
        context: &RuntimeContext,
        lifecycle: ServiceLifecycle,
        last_error: Option<String>,
    ) -> RuntimeSnapshot {
        let now = now_unix_ts();
        let state = self.state.lock().expect("state lock poisoned");
        let runtime = self.client.runtime_snapshot();

        let mut active_sessions = self
            .active_turn_batches
            .lock()
            .expect("active turn lock poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let (mut queued_sessions, queued_turns) = {
            let pending = self.pending_turns.lock().expect("pending lock poisoned");
            let queued_sessions = pending
                .iter()
                .filter_map(|(session_id, queue)| {
                    if queue.is_empty() {
                        None
                    } else {
                        Some(session_id.clone())
                    }
                })
                .collect::<Vec<_>>();
            let queued_turns = pending.values().map(VecDeque::len).sum();
            (queued_sessions, queued_turns)
        };
        let mut running_workers = state
            .workers
            .values()
            .filter(|worker| {
                matches!(
                    worker.status,
                    WorkerStatus::SpawnRequested
                        | WorkerStatus::Bootstrapping
                        | WorkerStatus::Running
                )
            })
            .map(|worker| worker.id.clone())
            .collect::<Vec<_>>();

        active_sessions.sort();
        queued_sessions.sort();
        running_workers.sort();

        RuntimeSnapshot {
            status: lifecycle,
            mode: context.mode.clone(),
            pid: std::process::id(),
            app_server_pid: runtime.pid,
            app_server_connected: runtime.connected,
            started_at: context.started_at,
            updated_at: now,
            command_label: context.command_label.clone(),
            master_thread_id: state.master_thread_id.clone(),
            active_turns: active_sessions.len(),
            queued_turns,
            active_sessions,
            queued_sessions,
            running_workers,
            last_error,
        }
    }

    fn log_notification(&self, log_label: &str, notification: &Notification) -> Result<()> {
        let entry = json!({
            "ts": now_unix_ts(),
            "method": notification.method,
            "params": notification.params,
        });
        logging::append_jsonl(
            &self.paths.log_dir,
            self.config.logging.retention_days,
            &format!("sessions/{log_label}"),
            &entry,
        )?;
        Ok(())
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
            "You are the master controller for CodeClaw in {}. Coordinate work across workers, keep plans concise, and prefer actionable task splits.\n\nYou may receive direct human prompts and internal runtime updates about worker lifecycle changes. Treat runtime updates as scheduler inputs: absorb the worker result, update summaries when useful, and dispatch follow-up actions only when they are actually needed.\n\nExecution policy:\n- Prefer to start real work, not only meta-planning, when a safe next step is obvious.\n- Make reasonable assumptions and move forward unless missing information genuinely blocks useful progress.\n- Ask for clarification only when safety, approvals, or objective ambiguity makes the next step too risky.\n\nImportant lifecycle meanings:\n- bootstrapped: the worker completed its initial bootstrap turn\n- handed_back: the worker completed a later turn and returned control\n- blocked: the worker reported a blocker and likely needs intervention\n- failed: the worker turn failed unexpectedly\n\nWhen you respond, append exactly one machine-readable block at the end using this format:\n<codeclaw-actions>\n{{\"summary\":\"short orchestration summary\",\"actions\":[...]}}\n</codeclaw-actions>\n\nAllowed actions:\n- {{\"type\":\"spawn_worker\",\"group\":\"backend|frontend|infra\",\"task\":\"short task title\",\"summary\":\"optional short sidebar summary\",\"prompt\":\"optional initial worker prompt\"}}\n- {{\"type\":\"send_worker_prompt\",\"worker_id\":\"existing-worker-id\",\"prompt\":\"follow-up instructions\"}}\n- {{\"type\":\"update_worker_summary\",\"worker_id\":\"existing-worker-id\",\"summary\":\"new short summary\"}}\n\nRules:\n- Always include the block, even when no actions are needed.\n- Keep `summary` short enough to fit a sidebar.\n- Use worker ids exactly as shown in the UI when referencing existing workers.",
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

fn file_fingerprint(path: &Path) -> Option<FileFingerprint> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn session_sort_key(id: &str) -> (u8, &str) {
    if id == ONBOARD_SESSION_ID {
        (0, id)
    } else if id == MASTER_SESSION_ID {
        (1, id)
    } else {
        (2, id)
    }
}

fn build_monitor_session_snapshot(
    session: SessionSnapshot,
    active: bool,
    queued_turns: usize,
) -> MonitorSessionSnapshot {
    let latest_user_prompt = session.latest_user_prompt();
    let latest_response = session.latest_assistant_output();
    let (role, group, task) = match &session.kind {
        crate::session::SessionKind::Onboard => ("onboard".to_owned(), None, None),
        crate::session::SessionKind::Master => (
            "master".to_owned(),
            None,
            Some("global coordination".to_owned()),
        ),
        crate::session::SessionKind::Worker { group, task, .. } => (
            format!("worker:{group}"),
            Some(group.clone()),
            Some(task.clone()),
        ),
    };

    MonitorSessionSnapshot {
        id: session.id,
        title: session.title,
        role,
        group,
        task,
        job_id: session.job_id,
        status: session.status,
        work_state: session_work_state(active, queued_turns),
        pending_turns: session.pending_turns,
        latest_batch_id: session.latest_batch_id,
        latest_user_prompt,
        latest_response,
        summary: session
            .summary
            .or(session.lifecycle_note)
            .or(session.last_message),
    }
}

fn session_work_state(active: bool, queued_turns: usize) -> String {
    match (active, queued_turns > 0) {
        (true, true) => "active+queued".to_owned(),
        (true, false) => "active".to_owned(),
        (false, true) => "queued".to_owned(),
        (false, false) => "idle".to_owned(),
    }
}

fn automation_prompt_target(session_id: &str) -> PromptTarget {
    if session_id == MASTER_SESSION_ID {
        PromptTarget::Master
    } else {
        PromptTarget::Worker(session_id.to_owned())
    }
}

fn session_automation_snapshot(
    automation: &SessionAutomationRecord,
    now: u64,
) -> SessionAutomationSnapshot {
    SessionAutomationSnapshot {
        id: automation.id.clone(),
        target_session_id: automation.target_session_id.clone(),
        status: automation.status.to_string(),
        prompt_preview: compact_message(&automation.prompt),
        interval_secs: automation.interval_secs,
        run_count: automation.run_count,
        max_runs: automation.max_runs,
        run_for_secs: automation.run_for_secs,
        remaining_runs: session_automation_remaining_runs(automation),
        remaining_secs: session_automation_remaining_secs(automation, now),
        next_run_at: automation.next_run_at,
        last_run_at: automation.last_run_at,
        last_batch_id: automation.last_batch_id,
        last_error: automation.last_error.clone(),
    }
}

fn session_automation_sort_key(automation: &SessionAutomationSnapshot) -> (u8, u64, &str) {
    (
        session_automation_status_rank(&automation.status),
        automation.next_run_at.unwrap_or(u64::MAX),
        automation.id.as_str(),
    )
}

fn session_automation_status_rank(status: &str) -> u8 {
    match status {
        "armed" => 0,
        "paused" => 1,
        "failed" => 2,
        "completed" => 3,
        "cancelled" => 4,
        _ => 5,
    }
}

fn session_automation_remaining_runs(automation: &SessionAutomationRecord) -> Option<u32> {
    automation
        .max_runs
        .map(|max_runs| max_runs.saturating_sub(automation.run_count))
}

fn session_automation_remaining_secs(
    automation: &SessionAutomationRecord,
    now: u64,
) -> Option<u64> {
    match (automation.started_at, automation.run_for_secs) {
        (Some(started_at), Some(limit)) => {
            Some(limit.saturating_sub(now.saturating_sub(started_at)))
        }
        _ => None,
    }
}

fn session_automation_is_due(automation: &SessionAutomationRecord, now: u64) -> bool {
    matches!(automation.status, SessionAutomationStatus::Armed)
        && automation
            .next_run_at
            .is_some_and(|next_run_at| next_run_at <= now)
}

fn session_automation_is_exhausted(automation: &SessionAutomationRecord, now: u64) -> bool {
    session_automation_remaining_runs(automation) == Some(0)
        || session_automation_remaining_secs(automation, now) == Some(0)
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
    latest_summary: Option<&str>,
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
    } else if latest_summary.is_some_and(job_summary_indicates_blocker) {
        JobStatus::Blocked
    } else {
        JobStatus::Completed
    }
}

fn job_summary_indicates_blocker(summary: &str) -> bool {
    job_summary_indicates_approval_wait(summary)
        || job_summary_indicates_input_wait(summary)
        || job_summary_indicates_clarification_wait(summary)
        || summary.to_ascii_lowercase().contains("blocked")
}

fn job_summary_indicates_approval_wait(summary: &str) -> bool {
    let normalized = summary.to_ascii_lowercase();
    ["need approval", "awaiting approval", "waiting on approval"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn job_summary_indicates_input_wait(summary: &str) -> bool {
    let normalized = summary.to_ascii_lowercase();
    [
        "need input",
        "need guidance",
        "awaiting input",
        "waiting on",
        "cannot continue",
        "can't continue",
        "unable to continue",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn job_summary_indicates_clarification_wait(summary: &str) -> bool {
    let normalized = summary.to_ascii_lowercase();
    [
        "requested clarification",
        "request clarification",
        "need clarification",
        "needs clarification",
        "awaiting clarification",
        "scope clarification",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn operator_state_for_job(job: &JobRecord, automation: &JobAutomationSnapshot) -> String {
    match job.status {
        JobStatus::Blocked => {
            let summary = job.latest_summary.as_deref().unwrap_or_default();
            if job.policy.approval_required && !job.policy.auto_approve
                || job_summary_indicates_approval_wait(summary)
            {
                "awaiting approval".to_owned()
            } else if job_summary_indicates_clarification_wait(summary) {
                "awaiting clarification".to_owned()
            } else if job_summary_indicates_input_wait(summary) {
                "awaiting input".to_owned()
            } else {
                "blocked".to_owned()
            }
        }
        JobStatus::Running => {
            if job.policy.delegate_to_master_loop {
                format!("running ({})", automation_state_label(&automation.state))
            } else {
                "running".to_owned()
            }
        }
        JobStatus::Pending => "pending".to_owned(),
        JobStatus::Completed => "completed".to_owned(),
        JobStatus::Failed => "failed".to_owned(),
    }
}

fn format_job_context(job: &JobRecord) -> String {
    let requester = job
        .requester
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let context = job.context.clone().unwrap_or_else(|| "none".to_owned());
    let automation = job_automation_snapshot(job, now_unix_ts());
    format!(
        "Job context:\n- Job id: {}\n- Title: {}\n- Objective: {}\n- Source channel: {}\n- Requester: {}\n- Priority: {}\n- Pattern: {}\n- Approval required: {}\n- Auto approve: {}\n- Delegate master loop: {}\n- Automation state: {}\n- Current summary: {}\n- Context: {}",
        job.id,
        job.title,
        job.objective,
        job.source_channel,
        requester,
        job.priority,
        job.policy.pattern,
        if job.policy.approval_required { "yes" } else { "no" },
        if job.policy.auto_approve { "yes" } else { "no" },
        if job.policy.delegate_to_master_loop { "yes" } else { "no" },
        automation_state_label(&automation.state),
        job.latest_summary
            .clone()
            .unwrap_or_else(|| "none".to_owned()),
        context
    )
}

fn report_kind_for_job_update(
    previous_status: &JobStatus,
    previous_summary: Option<&str>,
    next_status: &JobStatus,
    next_summary: Option<&str>,
) -> Option<JobReportKind> {
    if previous_status != next_status {
        return match next_status {
            JobStatus::Pending => None,
            JobStatus::Running => Some(JobReportKind::Progress),
            JobStatus::Blocked => Some(JobReportKind::Blocker),
            JobStatus::Completed => Some(JobReportKind::Completion),
            JobStatus::Failed => Some(JobReportKind::Failure),
        };
    }

    let previous_summary = previous_summary
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let next_summary = next_summary
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if previous_summary == next_summary {
        return None;
    }

    match next_status {
        JobStatus::Running => Some(JobReportKind::Progress),
        JobStatus::Blocked => Some(JobReportKind::Blocker),
        JobStatus::Completed => Some(JobReportKind::Completion),
        JobStatus::Failed => Some(JobReportKind::Failure),
        JobStatus::Pending => None,
    }
}

fn report_summary_for_job_update(
    job: &JobRecord,
    kind: &JobReportKind,
    summary: Option<&str>,
    status: &JobStatus,
) -> String {
    let summary = summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_summary_for_job_status(status));
    match kind {
        JobReportKind::Accepted => format!("job accepted: {}", job.title),
        JobReportKind::Progress => format!("progress: {summary}"),
        JobReportKind::Blocker => format!("blocker: {summary}"),
        JobReportKind::Completion => format!("completed: {summary}"),
        JobReportKind::Failure => format!("failed: {summary}"),
        JobReportKind::Digest => format!("digest: {summary}"),
    }
}

fn render_job_report_body(job: &JobRecord, kind: JobReportKind, summary: Option<&str>) -> String {
    let summary = summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_summary_for_job_status(&job.status));
    let requester = job
        .requester
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let context = job.context.clone().unwrap_or_else(|| "none".to_owned());
    let next_step = next_step_for_job_status(&job.status, &job.policy.approval_required);
    let automation = job_automation_snapshot(job, now_unix_ts());

    format!(
        "Job: {}\nKind: {}\nStatus: {}\nSummary: {}\nRequester: {}\nSource channel: {}\nPriority: {}\nPattern: {}\nApproval required: {}\nAuto approve: {}\nDelegate master loop: {}\nAutomation state: {}\nContinue budget secs: {}\nContinue budget iterations: {}\nContinue iterations used: {}\nBatches: {}\nWorkers: {}\nContext: {}\nNext step: {}",
        job.id,
        kind,
        job.status,
        summary,
        requester,
        job.source_channel,
        job.priority,
        job.policy.pattern,
        if job.policy.approval_required { "yes" } else { "no" },
        if job.policy.auto_approve { "yes" } else { "no" },
        if job.policy.delegate_to_master_loop { "yes" } else { "no" },
        automation_state_label(&automation.state),
        job.policy
            .continue_for_secs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        job.policy
            .continue_max_iterations
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        job.continue_iterations,
        job.batch_ids.len(),
        job.worker_ids.len(),
        context,
        next_step
    )
}

fn default_summary_for_job_status(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "job is pending intake",
        JobStatus::Running => "job is running",
        JobStatus::Blocked => "job is blocked",
        JobStatus::Completed => "job completed successfully",
        JobStatus::Failed => "job failed",
    }
}

fn next_step_for_job_status(status: &JobStatus, approval_required: &bool) -> &'static str {
    match status {
        JobStatus::Pending => "awaiting service intake or operator dispatch",
        JobStatus::Running if *approval_required => {
            "continue execution, but keep risky actions behind approval gates"
        }
        JobStatus::Running => "continue execution and report major milestones",
        JobStatus::Blocked => "operator attention is likely required to unblock work",
        JobStatus::Completed => "deliver completion summary and close the job",
        JobStatus::Failed => "inspect failure details and decide whether to retry or escalate",
    }
}

fn job_automation_snapshot(job: &JobRecord, now: u64) -> JobAutomationSnapshot {
    let remaining_secs = match (job.automation_started_at, job.policy.continue_for_secs) {
        (_, None) => None,
        (Some(started_at), Some(limit)) => {
            Some(limit.saturating_sub(now.saturating_sub(started_at)))
        }
        (None, Some(limit)) => Some(limit),
    };
    let remaining_iterations = job
        .policy
        .continue_max_iterations
        .map(|limit| limit.saturating_sub(job.continue_iterations));

    let state = if !job.policy.delegate_to_master_loop {
        "manual"
    } else if matches!(job.status, JobStatus::Completed | JobStatus::Failed) {
        "terminal"
    } else if matches!(remaining_secs, Some(0)) {
        "budget_exhausted_time"
    } else if matches!(remaining_iterations, Some(0)) {
        "budget_exhausted_iterations"
    } else if matches!(job.status, JobStatus::Blocked)
        && job.policy.approval_required
        && !job.policy.auto_approve
    {
        "awaiting_manual_approval"
    } else if job.automation_started_at.is_some() {
        "active"
    } else {
        "armed"
    };

    JobAutomationSnapshot {
        state: state.to_owned(),
        auto_approve: job.policy.auto_approve,
        delegate_to_master_loop: job.policy.delegate_to_master_loop,
        automation_started_at: job.automation_started_at,
        last_continue_at: job.last_continue_at,
        continue_iterations: job.continue_iterations,
        remaining_secs,
        remaining_iterations,
    }
}

fn job_is_eligible_for_master_loop(job: &JobRecord, now: u64) -> bool {
    if !matches!(job.status, JobStatus::Running | JobStatus::Blocked) {
        return false;
    }

    let automation = job_automation_snapshot(job, now);
    if !matches!(automation.state.as_str(), "armed" | "active") {
        return false;
    }

    if let Some(last_continue_at) = job.last_continue_at {
        if now.saturating_sub(last_continue_at) < DEFAULT_AUTOMATION_CONTINUE_INTERVAL_SECS {
            return false;
        }
    }

    true
}

fn job_has_active_workers(state: &AppState, job: &JobRecord) -> bool {
    job.worker_ids.iter().any(|worker_id| {
        state
            .workers
            .get(worker_id)
            .map(|worker| {
                matches!(
                    worker.status,
                    WorkerStatus::SpawnRequested
                        | WorkerStatus::Bootstrapping
                        | WorkerStatus::Running
                )
            })
            .unwrap_or(false)
    })
}

fn automation_state_label(state: &str) -> &'static str {
    match state {
        "manual" => "manual",
        "armed" => "armed",
        "active" => "active",
        "awaiting_manual_approval" => "awaiting approval",
        "budget_exhausted_time" => "time exhausted",
        "budget_exhausted_iterations" => "loop exhausted",
        "terminal" => "terminal",
        _ => "unknown",
    }
}

fn format_duration_compact(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn trim_reports_for_job(reports: &mut BTreeMap<u64, JobReportRecord>, job_id: &str) {
    let mut ids = reports
        .iter()
        .filter_map(|(report_id, report)| (report.job_id == job_id).then_some(*report_id))
        .collect::<Vec<_>>();
    if ids.len() <= MAX_JOB_REPORTS_PER_JOB {
        return;
    }
    ids.sort_unstable();
    let remove_count = ids.len().saturating_sub(MAX_JOB_REPORTS_PER_JOB);
    for report_id in ids.into_iter().take(remove_count) {
        reports.remove(&report_id);
    }
}

fn trim_deliveries_for_job(deliveries: &mut BTreeMap<u64, ReportDeliveryRecord>, job_id: &str) {
    let mut ids = deliveries
        .iter()
        .filter_map(|(delivery_id, delivery)| (delivery.job_id == job_id).then_some(*delivery_id))
        .collect::<Vec<_>>();
    if ids.len() <= MAX_JOB_DELIVERIES_PER_JOB {
        return;
    }
    ids.sort_unstable();
    let remove_count = ids.len().saturating_sub(MAX_JOB_DELIVERIES_PER_JOB);
    for delivery_id in ids.into_iter().take(remove_count) {
        deliveries.remove(&delivery_id);
    }
}

fn subscription_accepts_report_kind(
    subscription: &ReportSubscriptionRecord,
    kind: &JobReportKind,
) -> bool {
    match kind {
        JobReportKind::Accepted => subscription.notify_on_accepted,
        JobReportKind::Progress => subscription.notify_on_progress,
        JobReportKind::Blocker => subscription.notify_on_blocker,
        JobReportKind::Completion => subscription.notify_on_completion,
        JobReportKind::Failure => subscription.notify_on_failure,
        JobReportKind::Digest => subscription.notify_on_digest,
    }
}

fn format_report_delivery_message(report: &JobReportRecord, endpoint: &str) -> String {
    format!(
        "[{}] {} [{}] {}",
        endpoint, report.job_id, report.kind, report.summary
    )
}

fn service_job_intake_prompt(job: &JobRecord) -> String {
    let context = job.context.clone().unwrap_or_else(|| "none".to_owned());
    format!(
        "New job intake for CodeClaw service mode.\n\nTitle: {}\nObjective: {}\nSource channel: {}\nPriority: {}\nPattern: {}\nApproval required: {}\nContext: {}\n\nPlan the job, keep the coordination summary concise, and dispatch worker actions if the work should start now. Prefer to begin concrete execution instead of stopping at a meta-plan when the next safe step is obvious. Make reasonable assumptions and continue unless missing information, approvals, or safety constraints truly block the work. Respect the selected orchestration pattern and keep risky work approval-gated when required.",
        job.title,
        job.objective,
        job.source_channel,
        job.priority,
        job.policy.pattern,
        if job.policy.approval_required { "yes" } else { "no" },
        context
    )
}

fn service_job_continue_prompt(job: &JobRecord, automation: &JobAutomationSnapshot) -> String {
    let remaining_secs = automation
        .remaining_secs
        .map(format_duration_compact)
        .unwrap_or_else(|| "unbounded".to_owned());
    let remaining_iterations = automation
        .remaining_iterations
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unbounded".to_owned());
    let context = job.context.clone().unwrap_or_else(|| "none".to_owned());

    format!(
        "Onboard supervision loop for CodeClaw service mode.\n\nJob id: {}\nTitle: {}\nObjective: {}\nCurrent status: {}\nCurrent summary: {}\nPriority: {}\nApproval required: {}\nAuto approve: {}\nDelegate master loop: {}\nAutomation state: {}\nRemaining automation time: {}\nRemaining automation iterations: {}\nContinue iterations used: {}\nContext: {}\n\nDecide whether this job should continue now. If more work is needed, plan the next safe step and dispatch workers as needed. If the job should pause, say why. Respect approval boundaries and avoid infinite loops.",
        job.id,
        job.title,
        job.objective,
        job.status,
        job.latest_summary
            .clone()
            .unwrap_or_else(|| default_summary_for_job_status(&job.status).to_owned()),
        job.priority,
        if job.policy.approval_required { "yes" } else { "no" },
        if job.policy.auto_approve { "yes" } else { "no" },
        if job.policy.delegate_to_master_loop { "yes" } else { "no" },
        automation_state_label(&automation.state),
        remaining_secs,
        remaining_iterations,
        job.continue_iterations,
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
        automation_state_label, blocker_reason_from_message, completed_state_for_turn,
        derive_job_status_from_state, job_automation_snapshot, job_is_eligible_for_master_loop,
        lifecycle_note_for, render_job_report_body, report_kind_for_job_update,
        report_summary_for_job_update, service_job_continue_prompt, service_job_intake_prompt,
        session_work_state, worker_message_indicates_blocker, worker_status_for, SessionRole,
        TurnSource,
    };
    use crate::state::{
        AppState, BatchStatus, JobPolicy, JobRecord, JobReportKind, JobStatus,
        OrchestrationBatchRecord, WorkerStatus,
    };

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
    fn session_work_state_distinguishes_active_and_queue_backlog() {
        assert_eq!(session_work_state(true, 0), "active");
        assert_eq!(session_work_state(false, 2), "queued");
        assert_eq!(session_work_state(true, 3), "active+queued");
        assert_eq!(session_work_state(false, 0), "idle");
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
            derive_job_status_from_state(&state, &[1], &[], None),
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
            derive_job_status_from_state(&state, &[1], &["backend-001".to_owned()], None),
            JobStatus::Blocked
        );

        if let Some(worker) = state.workers.get_mut("backend-001") {
            worker.status = WorkerStatus::Failed;
        }
        assert_eq!(
            derive_job_status_from_state(&state, &[1], &["backend-001".to_owned()], None),
            JobStatus::Failed
        );

        state.workers.clear();
        if let Some(batch) = state.batches.get_mut(&1) {
            batch.status = BatchStatus::Completed;
        }
        assert_eq!(
            derive_job_status_from_state(&state, &[1], &[], None),
            JobStatus::Completed
        );
        assert_eq!(
            derive_job_status_from_state(&state, &[], &[], None),
            JobStatus::Pending
        );
    }

    #[test]
    fn completed_batches_with_clarification_summary_are_marked_blocked() {
        let mut state = AppState::default();
        state.batches.insert(
            1,
            OrchestrationBatchRecord {
                id: 1,
                root_session_id: "master".to_owned(),
                root_prompt: "root".to_owned(),
                job_id: Some("JOB-013".to_owned()),
                status: BatchStatus::Completed,
                created_at: 1,
                updated_at: 1,
                sessions: vec!["master".to_owned()],
                last_event: None,
            },
        );

        assert_eq!(
            derive_job_status_from_state(
                &state,
                &[1],
                &[],
                Some("Planned JOB-013 and requested scope clarification."),
            ),
            JobStatus::Blocked
        );
    }

    #[test]
    fn service_job_intake_prompt_carries_policy_context() {
        let job = JobRecord {
            id: "JOB-001".to_owned(),
            source_channel: "cli".to_owned(),
            requester: Some("operator".to_owned()),
            title: "Payment API refactor".to_owned(),
            objective: "Refactor payment orchestration".to_owned(),
            context: Some("Keep rollout low risk".to_owned()),
            status: JobStatus::Pending,
            priority: "high".to_owned(),
            policy: JobPolicy {
                pattern: "planner_executor_reviewer".to_owned(),
                approval_required: true,
                auto_approve: false,
                delegate_to_master_loop: false,
                continue_for_secs: None,
                continue_max_iterations: None,
            },
            created_at: 1,
            updated_at: 1,
            batch_ids: Vec::new(),
            worker_ids: Vec::new(),
            latest_summary: None,
            latest_report_at: None,
            next_report_due_at: None,
            escalation_state: None,
            final_outcome: None,
            automation_started_at: None,
            last_continue_at: None,
            continue_iterations: 0,
        };
        let prompt = service_job_intake_prompt(&job);

        assert!(prompt.contains("Payment API refactor"));
        assert!(prompt.contains("planner_executor_reviewer"));
        assert!(prompt.contains("Approval required: yes"));
        assert!(prompt.contains("Keep rollout low risk"));
        assert!(prompt.contains("Prefer to begin concrete execution"));
        assert!(prompt.contains("Make reasonable assumptions"));

        assert_eq!(
            report_kind_for_job_update(
                &JobStatus::Pending,
                Some("job created"),
                &JobStatus::Running,
                Some("worker assigned"),
            ),
            Some(JobReportKind::Progress)
        );
        assert_eq!(
            report_kind_for_job_update(
                &JobStatus::Running,
                Some("worker assigned"),
                &JobStatus::Running,
                Some("implemented API changes"),
            ),
            Some(JobReportKind::Progress)
        );
        assert_eq!(
            report_kind_for_job_update(
                &JobStatus::Running,
                Some("implemented API changes"),
                &JobStatus::Blocked,
                Some("waiting for approval"),
            ),
            Some(JobReportKind::Blocker)
        );

        let summary = report_summary_for_job_update(
            &job,
            &JobReportKind::Digest,
            Some("implemented API changes"),
            &JobStatus::Running,
        );
        assert_eq!(summary, "digest: implemented API changes");

        let body = render_job_report_body(
            &JobRecord {
                status: JobStatus::Blocked,
                latest_summary: Some("waiting for approval".to_owned()),
                ..job
            },
            JobReportKind::Blocker,
            Some("waiting for approval"),
        );
        assert!(body.contains("Kind: blocker"));
        assert!(body.contains("Status: blocked"));
        assert!(body.contains("Next step: operator attention is likely required"));
    }

    #[test]
    fn automation_snapshot_marks_budget_exhaustion_and_manual_approval() {
        let blocked_job = JobRecord {
            id: "JOB-009".to_owned(),
            source_channel: "cli".to_owned(),
            requester: None,
            title: "Blocked rollout".to_owned(),
            objective: "Continue only when safe".to_owned(),
            context: None,
            status: JobStatus::Blocked,
            priority: "high".to_owned(),
            policy: JobPolicy {
                pattern: "supervisor_worker".to_owned(),
                approval_required: true,
                auto_approve: false,
                delegate_to_master_loop: true,
                continue_for_secs: Some(3600),
                continue_max_iterations: Some(5),
            },
            created_at: 10,
            updated_at: 10,
            batch_ids: vec![1],
            worker_ids: vec![],
            latest_summary: Some("waiting on approval".to_owned()),
            latest_report_at: None,
            next_report_due_at: None,
            escalation_state: None,
            final_outcome: None,
            automation_started_at: Some(100),
            last_continue_at: Some(200),
            continue_iterations: 2,
        };

        let blocked = job_automation_snapshot(&blocked_job, 250);
        assert_eq!(blocked.state, "awaiting_manual_approval");
        assert_eq!(automation_state_label(&blocked.state), "awaiting approval");

        let exhausted_job = JobRecord {
            continue_iterations: 5,
            ..blocked_job
        };
        let exhausted = job_automation_snapshot(&exhausted_job, 250);
        assert_eq!(exhausted.state, "budget_exhausted_iterations");
    }

    #[test]
    fn delegated_jobs_are_eligible_only_after_cooldown_and_with_budget() {
        let job = JobRecord {
            id: "JOB-010".to_owned(),
            source_channel: "cli".to_owned(),
            requester: None,
            title: "Loop me".to_owned(),
            objective: "Keep going".to_owned(),
            context: None,
            status: JobStatus::Running,
            priority: "normal".to_owned(),
            policy: JobPolicy {
                pattern: "supervisor_worker".to_owned(),
                approval_required: false,
                auto_approve: true,
                delegate_to_master_loop: true,
                continue_for_secs: Some(7200),
                continue_max_iterations: Some(3),
            },
            created_at: 10,
            updated_at: 10,
            batch_ids: vec![1],
            worker_ids: vec![],
            latest_summary: Some("continue".to_owned()),
            latest_report_at: None,
            next_report_due_at: None,
            escalation_state: None,
            final_outcome: None,
            automation_started_at: Some(100),
            last_continue_at: Some(350),
            continue_iterations: 1,
        };

        assert!(!job_is_eligible_for_master_loop(&job, 400));
        assert!(job_is_eligible_for_master_loop(&job, 700));

        let prompt = service_job_continue_prompt(&job, &job_automation_snapshot(&job, 700));
        assert!(prompt.contains("Auto approve: yes"));
        assert!(prompt.contains("Remaining automation iterations: 2"));
    }
}
