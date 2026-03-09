use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const DEFAULT_CONFIG: &str = include_str!("../codeclaw.example.toml");
pub const CONFIG_FILE_NAME: &str = "codeclaw.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub master: MasterConfig,
    pub workers: WorkerConfig,
    pub git: GitConfig,
    pub ui: UiConfig,
    pub coordination: CoordinationConfig,
    #[serde(default)]
    pub groups: Vec<GroupConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterConfig {
    pub mode: String,
    pub model: String,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    pub sandbox: String,
    pub approval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub default_mode: String,
    pub attach_mode: String,
    pub max_parallel: usize,
    pub inherit_model: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    pub main_branch: String,
    pub integration_branch: String,
    pub worktree_root: String,
    pub task_branch_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub sidebar_width: usize,
    pub show_branch: bool,
    pub show_cwd: bool,
    pub show_locks: bool,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationConfig {
    pub root: String,
    pub task_dir: String,
    pub status_dir: String,
    pub lock_file: String,
    pub decision_dir: String,
    pub log_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupConfig {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub lease_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CoordinationPaths {
    pub root: PathBuf,
    pub task_dir: PathBuf,
    pub status_dir: PathBuf,
    pub lock_file: PathBuf,
    pub decision_dir: PathBuf,
    pub log_dir: PathBuf,
    pub worktree_root: PathBuf,
    pub state_file: PathBuf,
}

impl Config {
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let config_path = workspace_root.join(CONFIG_FILE_NAME);
        let (source_name, raw) = if config_path.exists() {
            (
                config_path.display().to_string(),
                fs::read_to_string(&config_path)
                    .with_context(|| format!("failed to read {}", config_path.display()))?,
            )
        } else {
            (
                format!("embedded default from {}", CONFIG_FILE_NAME),
                DEFAULT_CONFIG.to_owned(),
            )
        };

        toml::from_str(&raw).with_context(|| format!("failed to parse config from {source_name}"))
    }

    pub fn write_default_config_if_missing(workspace_root: &Path) -> Result<Option<PathBuf>> {
        let config_path = workspace_root.join(CONFIG_FILE_NAME);
        if config_path.exists() {
            return Ok(None);
        }

        fs::write(&config_path, DEFAULT_CONFIG)
            .with_context(|| format!("failed to write {}", config_path.display()))?;
        Ok(Some(config_path))
    }

    pub fn coordination_paths(&self, workspace_root: &Path) -> CoordinationPaths {
        let root = resolve_path(workspace_root, &self.coordination.root);
        CoordinationPaths {
            task_dir: resolve_path(workspace_root, &self.coordination.task_dir),
            status_dir: resolve_path(workspace_root, &self.coordination.status_dir),
            lock_file: resolve_path(workspace_root, &self.coordination.lock_file),
            decision_dir: resolve_path(workspace_root, &self.coordination.decision_dir),
            log_dir: resolve_path(workspace_root, &self.coordination.log_dir),
            worktree_root: resolve_path(workspace_root, &self.git.worktree_root),
            state_file: root.join("state.json"),
            root,
        }
    }

    pub fn group(&self, id: &str) -> Option<&GroupConfig> {
        self.groups.iter().find(|group| group.id == id)
    }
}

fn default_reasoning_effort() -> String {
    "high".to_owned()
}

impl CoordinationPaths {
    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            &self.root,
            &self.task_dir,
            &self.status_dir,
            &self.decision_dir,
            &self.log_dir,
            &self.worktree_root,
        ] {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }

        if let Some(parent) = self.lock_file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        if !self.lock_file.exists() {
            fs::write(&self.lock_file, "{}\n")
                .with_context(|| format!("failed to initialize {}", self.lock_file.display()))?;
        }

        Ok(())
    }
}

fn resolve_path(workspace_root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}
