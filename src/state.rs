use crate::session::SessionEvent;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub master_thread_id: Option<String>,
    pub master_last_turn_id: Option<String>,
    #[serde(default)]
    pub master_summary: Option<String>,
    #[serde(default)]
    pub master_last_message: Option<String>,
    #[serde(default = "default_next_batch_id")]
    pub next_batch_id: u64,
    #[serde(default = "default_next_job_number")]
    pub next_job_number: u64,
    #[serde(default = "default_next_report_number")]
    pub next_report_number: u64,
    #[serde(default = "default_next_report_subscription_number")]
    pub next_report_subscription_number: u64,
    #[serde(default = "default_next_report_delivery_number")]
    pub next_report_delivery_number: u64,
    #[serde(default = "default_next_session_automation_number")]
    pub next_session_automation_number: u64,
    pub next_task_number: u64,
    #[serde(default)]
    pub jobs: BTreeMap<String, JobRecord>,
    #[serde(default)]
    pub reports: BTreeMap<u64, JobReportRecord>,
    #[serde(default)]
    pub report_subscriptions: BTreeMap<u64, ReportSubscriptionRecord>,
    #[serde(default)]
    pub report_deliveries: BTreeMap<u64, ReportDeliveryRecord>,
    #[serde(default)]
    pub workers: BTreeMap<String, WorkerRecord>,
    #[serde(default)]
    pub session_history: BTreeMap<String, Vec<SessionEvent>>,
    #[serde(default)]
    pub session_output: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub session_live_buffers: BTreeMap<String, String>,
    #[serde(default)]
    pub batches: BTreeMap<u64, OrchestrationBatchRecord>,
    #[serde(default)]
    pub session_automations: BTreeMap<String, SessionAutomationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationBatchRecord {
    pub id: u64,
    pub root_session_id: String,
    pub root_prompt: String,
    #[serde(default)]
    pub job_id: Option<String>,
    pub status: BatchStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub sessions: Vec<String>,
    #[serde(default)]
    pub last_event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Blocked,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPolicy {
    #[serde(default = "default_job_pattern")]
    pub pattern: String,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default)]
    pub delegate_to_master_loop: bool,
    #[serde(default)]
    pub continue_for_secs: Option<u64>,
    #[serde(default)]
    pub continue_max_iterations: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub source_channel: String,
    #[serde(default)]
    pub requester: Option<String>,
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub context: Option<String>,
    pub status: JobStatus,
    #[serde(default = "default_job_priority")]
    pub priority: String,
    #[serde(default)]
    pub policy: JobPolicy,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub batch_ids: Vec<u64>,
    #[serde(default)]
    pub worker_ids: Vec<String>,
    #[serde(default)]
    pub latest_summary: Option<String>,
    #[serde(default)]
    pub latest_report_at: Option<u64>,
    #[serde(default)]
    pub next_report_due_at: Option<u64>,
    #[serde(default)]
    pub escalation_state: Option<String>,
    #[serde(default)]
    pub final_outcome: Option<String>,
    #[serde(default)]
    pub automation_started_at: Option<u64>,
    #[serde(default)]
    pub last_continue_at: Option<u64>,
    #[serde(default)]
    pub continue_iterations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobReportKind {
    Accepted,
    Progress,
    Blocker,
    Completion,
    Failure,
    Digest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobReportRecord {
    pub id: u64,
    pub job_id: String,
    pub kind: JobReportKind,
    pub job_status: JobStatus,
    pub summary: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportChannel {
    Console,
    MockFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSubscriptionRecord {
    pub id: u64,
    pub job_id: String,
    pub channel: ReportChannel,
    pub target: String,
    pub notify_on_accepted: bool,
    pub notify_on_progress: bool,
    pub notify_on_blocker: bool,
    pub notify_on_completion: bool,
    pub notify_on_failure: bool,
    pub notify_on_digest: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportDeliveryStatus {
    Queued,
    Delivered,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportDeliveryRecord {
    pub id: u64,
    pub report_id: u64,
    pub job_id: String,
    pub channel: ReportChannel,
    pub target: String,
    pub status: ReportDeliveryStatus,
    pub attempts: u32,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub id: String,
    pub group: String,
    pub task: String,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub lifecycle_note: Option<String>,
    pub task_file: String,
    pub thread_id: String,
    pub status: WorkerStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_turn_id: Option<String>,
    pub last_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAutomationRecord {
    pub id: String,
    pub target_session_id: String,
    pub prompt: String,
    pub interval_secs: u64,
    #[serde(default)]
    pub max_runs: Option<u32>,
    #[serde(default)]
    pub run_for_secs: Option<u64>,
    pub status: SessionAutomationStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub next_run_at: Option<u64>,
    #[serde(default)]
    pub last_run_at: Option<u64>,
    #[serde(default)]
    pub run_count: u32,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_batch_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionAutomationStatus {
    Armed,
    Paused,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Idle,
    SpawnRequested,
    Bootstrapping,
    Bootstrapped,
    Running,
    Blocked,
    HandedBack,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub role: String,
    pub thread_id: String,
    pub state: String,
    pub updated_at: u64,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub lifecycle_note: Option<String>,
    pub last_turn_id: Option<String>,
    pub last_message: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            master_thread_id: None,
            master_last_turn_id: None,
            master_summary: None,
            master_last_message: None,
            next_batch_id: default_next_batch_id(),
            next_job_number: default_next_job_number(),
            next_report_number: default_next_report_number(),
            next_report_subscription_number: default_next_report_subscription_number(),
            next_report_delivery_number: default_next_report_delivery_number(),
            next_session_automation_number: default_next_session_automation_number(),
            next_task_number: 1,
            jobs: BTreeMap::new(),
            reports: BTreeMap::new(),
            report_subscriptions: BTreeMap::new(),
            report_deliveries: BTreeMap::new(),
            workers: BTreeMap::new(),
            session_history: BTreeMap::new(),
            session_output: BTreeMap::new(),
            session_live_buffers: BTreeMap::new(),
            batches: BTreeMap::new(),
            session_automations: BTreeMap::new(),
        }
    }
}

impl AppState {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse state from {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let raw = serde_json::to_string_pretty(self).context("failed to encode state")?;
        fs::write(path, format!("{raw}\n"))
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

impl SessionStatus {
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let raw = serde_json::to_string_pretty(self).context("failed to encode status")?;
        fs::write(path, format!("{raw}\n"))
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

impl WorkerRecord {
    pub fn status_path(&self, status_dir: &Path) -> PathBuf {
        status_dir.join(format!("{}.json", self.id))
    }
}

impl OrchestrationBatchRecord {
    pub fn touch(&mut self, session_id: &str, event_text: Option<&str>) {
        self.updated_at = now_unix_ts();
        if !self.sessions.iter().any(|existing| existing == session_id) {
            self.sessions.push(session_id.to_owned());
            self.sessions.sort();
        }
        if let Some(event_text) = event_text {
            self.last_event = Some(event_text.to_owned());
        }
    }
}

impl Default for JobPolicy {
    fn default() -> Self {
        Self {
            pattern: default_job_pattern(),
            approval_required: false,
            auto_approve: false,
            delegate_to_master_loop: false,
            continue_for_secs: None,
            continue_max_iterations: None,
        }
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
        };
        f.write_str(value)
    }
}

impl fmt::Display for JobReportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Accepted => "accepted",
            Self::Progress => "progress",
            Self::Blocker => "blocker",
            Self::Completion => "completion",
            Self::Failure => "failure",
            Self::Digest => "digest",
        };
        f.write_str(value)
    }
}

impl fmt::Display for ReportChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Console => "console",
            Self::MockFile => "mock_file",
        };
        f.write_str(value)
    }
}

impl FromStr for ReportChannel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "console" => Ok(Self::Console),
            "mock-file" | "mock_file" | "mockfile" => Ok(Self::MockFile),
            other => Err(format!("unsupported report channel `{other}`")),
        }
    }
}

impl fmt::Display for ReportDeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Queued => "queued",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
        };
        f.write_str(value)
    }
}

impl fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Idle => "idle",
            Self::SpawnRequested => "spawn_requested",
            Self::Bootstrapping => "bootstrapping",
            Self::Bootstrapped => "bootstrapped",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::HandedBack => "handed_back",
            Self::Completed => "completed",
            Self::Failed => "failed",
        };
        f.write_str(value)
    }
}

impl fmt::Display for SessionAutomationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Armed => "armed",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        };
        f.write_str(value)
    }
}

pub fn now_unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn default_next_batch_id() -> u64 {
    1
}

fn default_next_job_number() -> u64 {
    1
}

fn default_next_report_number() -> u64 {
    1
}

fn default_next_report_subscription_number() -> u64 {
    1
}

fn default_next_report_delivery_number() -> u64 {
    1
}

fn default_next_session_automation_number() -> u64 {
    1
}

fn default_job_priority() -> String {
    "normal".to_owned()
}

fn default_job_pattern() -> String {
    "supervisor_worker".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, BatchStatus, JobPolicy, JobRecord, JobReportKind, JobReportRecord, JobStatus,
        OrchestrationBatchRecord, ReportChannel, ReportDeliveryRecord, ReportDeliveryStatus,
        ReportSubscriptionRecord, SessionAutomationRecord, SessionAutomationStatus, WorkerStatus,
    };
    use crate::session::{SessionEvent, SessionEventKind};

    #[test]
    fn app_state_round_trips_jobs_reports_workers_session_history_output_and_batches() {
        let mut state = AppState::default();
        state.next_batch_id = 7;
        state.next_job_number = 2;
        state.next_report_number = 4;
        state.next_report_subscription_number = 2;
        state.next_report_delivery_number = 3;
        state.next_session_automation_number = 2;
        state.jobs.insert(
            "JOB-001".to_owned(),
            JobRecord {
                id: "JOB-001".to_owned(),
                source_channel: "cli".to_owned(),
                requester: Some("operator".to_owned()),
                title: "Payment API refactor".to_owned(),
                objective: "Refactor the payment API with worker decomposition".to_owned(),
                context: Some("Focus on low-risk rollout".to_owned()),
                status: JobStatus::Running,
                priority: "high".to_owned(),
                policy: JobPolicy {
                    pattern: "planner_executor_reviewer".to_owned(),
                    approval_required: true,
                    auto_approve: true,
                    delegate_to_master_loop: true,
                    continue_for_secs: Some(3600),
                    continue_max_iterations: Some(8),
                },
                created_at: 119,
                updated_at: 126,
                batch_ids: vec![6],
                worker_ids: vec!["backend-001".to_owned()],
                latest_summary: Some("Planner assigned backend worker".to_owned()),
                latest_report_at: Some(124),
                next_report_due_at: Some(180),
                escalation_state: Some("none".to_owned()),
                final_outcome: None,
                automation_started_at: Some(121),
                last_continue_at: Some(126),
                continue_iterations: 2,
            },
        );
        state.reports.insert(
            3,
            JobReportRecord {
                id: 3,
                job_id: "JOB-001".to_owned(),
                kind: JobReportKind::Progress,
                job_status: JobStatus::Running,
                summary: "Planner assigned backend worker".to_owned(),
                body: "Job is running with one backend worker.".to_owned(),
                created_at: 125,
            },
        );
        state.report_subscriptions.insert(
            1,
            ReportSubscriptionRecord {
                id: 1,
                job_id: "JOB-001".to_owned(),
                channel: ReportChannel::Console,
                target: "stdout".to_owned(),
                notify_on_accepted: true,
                notify_on_progress: true,
                notify_on_blocker: true,
                notify_on_completion: true,
                notify_on_failure: true,
                notify_on_digest: true,
                created_at: 119,
                updated_at: 119,
            },
        );
        state.report_deliveries.insert(
            2,
            ReportDeliveryRecord {
                id: 2,
                report_id: 3,
                job_id: "JOB-001".to_owned(),
                channel: ReportChannel::Console,
                target: "stdout".to_owned(),
                status: ReportDeliveryStatus::Delivered,
                attempts: 1,
                created_at: 125,
                updated_at: 126,
                last_error: None,
            },
        );
        state.workers.insert(
            "backend-001".to_owned(),
            super::WorkerRecord {
                id: "backend-001".to_owned(),
                group: "backend".to_owned(),
                task: "Investigate API".to_owned(),
                job_id: Some("JOB-001".to_owned()),
                summary: Some("Investigating".to_owned()),
                lifecycle_note: Some("Blocked on approval for migration".to_owned()),
                task_file: ".codeclaw/tasks/TASK-001.md".to_owned(),
                thread_id: "thread-123".to_owned(),
                status: WorkerStatus::Blocked,
                created_at: 121,
                updated_at: 126,
                last_turn_id: Some("turn-123".to_owned()),
                last_message: Some("Blocked: need approval".to_owned()),
            },
        );
        state.session_history.insert(
            "master".to_owned(),
            vec![SessionEvent {
                ts: 123,
                batch_id: Some(6),
                kind: SessionEventKind::User,
                text: "started prompt".to_owned(),
            }],
        );
        state
            .session_output
            .insert("master".to_owned(), vec!["assistant> hello".to_owned()]);
        state
            .session_live_buffers
            .insert("master".to_owned(), "partial reply".to_owned());
        state.batches.insert(
            6,
            OrchestrationBatchRecord {
                id: 6,
                root_session_id: "master".to_owned(),
                root_prompt: "inspect api".to_owned(),
                job_id: Some("JOB-001".to_owned()),
                status: BatchStatus::Completed,
                created_at: 120,
                updated_at: 125,
                sessions: vec!["master".to_owned(), "backend-001".to_owned()],
                last_event: Some("worker done".to_owned()),
            },
        );
        state.session_automations.insert(
            "AUTO-001".to_owned(),
            SessionAutomationRecord {
                id: "AUTO-001".to_owned(),
                target_session_id: "master".to_owned(),
                prompt: "Review blocked jobs and continue".to_owned(),
                interval_secs: 300,
                max_runs: Some(10),
                run_for_secs: Some(3600),
                status: SessionAutomationStatus::Armed,
                created_at: 126,
                updated_at: 126,
                started_at: Some(126),
                next_run_at: Some(126),
                last_run_at: Some(126),
                run_count: 1,
                last_error: None,
                last_batch_id: Some(7),
            },
        );

        let raw = serde_json::to_string(&state).expect("state should encode");
        let decoded: AppState = serde_json::from_str(&raw).expect("state should decode");

        assert_eq!(decoded.next_batch_id, 7);
        assert_eq!(decoded.next_job_number, 2);
        assert_eq!(decoded.next_report_number, 4);
        assert_eq!(decoded.next_report_subscription_number, 2);
        assert_eq!(decoded.next_report_delivery_number, 3);
        assert_eq!(decoded.next_session_automation_number, 2);
        assert_eq!(decoded.jobs["JOB-001"].status, JobStatus::Running);
        assert_eq!(
            decoded.jobs["JOB-001"].policy.pattern,
            "planner_executor_reviewer"
        );
        assert!(decoded.jobs["JOB-001"].policy.auto_approve);
        assert!(decoded.jobs["JOB-001"].policy.delegate_to_master_loop);
        assert_eq!(decoded.jobs["JOB-001"].continue_iterations, 2);
        assert_eq!(decoded.reports[&3].kind, JobReportKind::Progress);
        assert_eq!(decoded.reports[&3].job_id, "JOB-001");
        assert_eq!(
            decoded.report_subscriptions[&1].channel,
            ReportChannel::Console
        );
        assert_eq!(
            decoded.report_deliveries[&2].status,
            ReportDeliveryStatus::Delivered
        );
        assert_eq!(
            decoded.workers["backend-001"].job_id.as_deref(),
            Some("JOB-001")
        );
        assert_eq!(
            decoded.workers["backend-001"].lifecycle_note.as_deref(),
            Some("Blocked on approval for migration")
        );
        assert_eq!(decoded.session_history["master"][0].batch_id, Some(6));
        assert_eq!(decoded.session_output["master"][0], "assistant> hello");
        assert_eq!(decoded.session_live_buffers["master"], "partial reply");
        assert_eq!(decoded.batches[&6].job_id.as_deref(), Some("JOB-001"));
        assert_eq!(decoded.batches[&6].status, BatchStatus::Completed);
        assert_eq!(
            decoded.session_automations["AUTO-001"].status,
            SessionAutomationStatus::Armed
        );
        assert_eq!(
            decoded.session_automations["AUTO-001"].target_session_id,
            "master"
        );
    }

    #[test]
    fn worker_status_round_trips_lifecycle_states() {
        let statuses = [
            WorkerStatus::SpawnRequested,
            WorkerStatus::Bootstrapping,
            WorkerStatus::Bootstrapped,
            WorkerStatus::Blocked,
            WorkerStatus::HandedBack,
        ];

        for status in statuses {
            let raw = serde_json::to_string(&status).expect("status should encode");
            let decoded: WorkerStatus = serde_json::from_str(&raw).expect("status should decode");
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn job_status_round_trips_lifecycle_states() {
        let statuses = [
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Blocked,
            JobStatus::Completed,
            JobStatus::Failed,
        ];

        for status in statuses {
            let raw = serde_json::to_string(&status).expect("status should encode");
            let decoded: JobStatus = serde_json::from_str(&raw).expect("status should decode");
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn session_automation_status_round_trips_lifecycle_states() {
        let statuses = [
            SessionAutomationStatus::Armed,
            SessionAutomationStatus::Paused,
            SessionAutomationStatus::Completed,
            SessionAutomationStatus::Cancelled,
            SessionAutomationStatus::Failed,
        ];

        for status in statuses {
            let raw = serde_json::to_string(&status).expect("status should encode");
            let decoded: SessionAutomationStatus =
                serde_json::from_str(&raw).expect("status should decode");
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn job_report_kind_round_trips_lifecycle_kinds() {
        let kinds = [
            JobReportKind::Accepted,
            JobReportKind::Progress,
            JobReportKind::Blocker,
            JobReportKind::Completion,
            JobReportKind::Failure,
            JobReportKind::Digest,
        ];

        for kind in kinds {
            let raw = serde_json::to_string(&kind).expect("kind should encode");
            let decoded: JobReportKind = serde_json::from_str(&raw).expect("kind should decode");
            assert_eq!(decoded, kind);
        }
    }

    #[test]
    fn report_delivery_status_round_trips_lifecycle_states() {
        let statuses = [
            ReportDeliveryStatus::Queued,
            ReportDeliveryStatus::Delivered,
            ReportDeliveryStatus::Failed,
        ];

        for status in statuses {
            let raw = serde_json::to_string(&status).expect("status should encode");
            let decoded: ReportDeliveryStatus =
                serde_json::from_str(&raw).expect("status should decode");
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn report_channel_from_str_accepts_cli_friendly_aliases() {
        assert_eq!(
            "console"
                .parse::<ReportChannel>()
                .expect("console should parse"),
            ReportChannel::Console
        );
        assert_eq!(
            "mock-file"
                .parse::<ReportChannel>()
                .expect("mock-file should parse"),
            ReportChannel::MockFile
        );
        assert_eq!(
            "mock_file"
                .parse::<ReportChannel>()
                .expect("mock_file should parse"),
            ReportChannel::MockFile
        );
    }
}
