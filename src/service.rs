use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceLifecycle {
    Starting,
    Running,
    Stopped,
    Failed,
}

impl fmt::Display for ServiceLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    pub status: ServiceLifecycle,
    pub pid: u32,
    pub started_at: u64,
    pub updated_at: u64,
    pub tick: u64,
    #[serde(default)]
    pub master_thread_id: Option<String>,
    #[serde(default)]
    pub pending_jobs: Vec<String>,
    #[serde(default)]
    pub running_jobs: Vec<String>,
    #[serde(default)]
    pub blocked_jobs: Vec<String>,
    #[serde(default)]
    pub completed_jobs: Vec<String>,
    #[serde(default)]
    pub failed_jobs: Vec<String>,
    #[serde(default)]
    pub stalled_jobs: Vec<String>,
    #[serde(default)]
    pub running_workers: Vec<String>,
    #[serde(default)]
    pub dispatched_jobs: Vec<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl ServiceSnapshot {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let snapshot =
            serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(Some(snapshot))
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let raw = serde_json::to_string_pretty(self).context("failed to encode service snapshot")?;
        fs::write(path, format!("{raw}\n"))
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::{ServiceLifecycle, ServiceSnapshot};

    #[test]
    fn service_snapshot_round_trips_json() {
        let snapshot = ServiceSnapshot {
            status: ServiceLifecycle::Running,
            pid: 1234,
            started_at: 10,
            updated_at: 20,
            tick: 3,
            master_thread_id: Some("thread-1".to_owned()),
            pending_jobs: vec!["JOB-001".to_owned()],
            running_jobs: vec!["JOB-002".to_owned()],
            blocked_jobs: vec![],
            completed_jobs: vec!["JOB-003".to_owned()],
            failed_jobs: vec![],
            stalled_jobs: vec!["JOB-004".to_owned()],
            running_workers: vec!["backend-001".to_owned()],
            dispatched_jobs: vec!["JOB-001".to_owned()],
            last_error: None,
        };

        let raw = serde_json::to_string(&snapshot).expect("snapshot should encode");
        let decoded: ServiceSnapshot =
            serde_json::from_str(&raw).expect("snapshot should decode");

        assert_eq!(decoded.status, ServiceLifecycle::Running);
        assert_eq!(decoded.pending_jobs, vec!["JOB-001"]);
        assert_eq!(decoded.running_workers, vec!["backend-001"]);
        assert_eq!(decoded.tick, 3);
    }
}
