use crate::session::SessionEvent;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
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
    pub next_task_number: u64,
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
    pub status: BatchStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub sessions: Vec<String>,
    #[serde(default)]
    pub last_event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub id: String,
    pub group: String,
    pub task: String,
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
            next_task_number: 1,
            workers: BTreeMap::new(),
            session_history: BTreeMap::new(),
            session_output: BTreeMap::new(),
            session_live_buffers: BTreeMap::new(),
            batches: BTreeMap::new(),
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

pub fn now_unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn default_next_batch_id() -> u64 {
    1
}

#[cfg(test)]
mod tests {
    use super::{AppState, BatchStatus, OrchestrationBatchRecord, WorkerStatus};
    use crate::session::{SessionEvent, SessionEventKind};

    #[test]
    fn app_state_round_trips_session_history_output_workers_and_batches() {
        let mut state = AppState::default();
        state.next_batch_id = 7;
        state.workers.insert(
            "backend-001".to_owned(),
            super::WorkerRecord {
                id: "backend-001".to_owned(),
                group: "backend".to_owned(),
                task: "Investigate API".to_owned(),
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
                status: BatchStatus::Completed,
                created_at: 120,
                updated_at: 125,
                sessions: vec!["master".to_owned(), "backend-001".to_owned()],
                last_event: Some("worker done".to_owned()),
            },
        );

        let raw = serde_json::to_string(&state).expect("state should encode");
        let decoded: AppState = serde_json::from_str(&raw).expect("state should decode");

        assert_eq!(decoded.next_batch_id, 7);
        assert_eq!(
            decoded.workers["backend-001"].lifecycle_note.as_deref(),
            Some("Blocked on approval for migration")
        );
        assert_eq!(decoded.session_history["master"][0].batch_id, Some(6));
        assert_eq!(decoded.session_output["master"][0], "assistant> hello");
        assert_eq!(decoded.session_live_buffers["master"], "partial reply");
        assert_eq!(decoded.batches[&6].status, BatchStatus::Completed);
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
}
