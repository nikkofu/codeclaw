use crate::{
    app_server::{AppServerClient, Notification},
    config::{Config, CoordinationPaths},
    state::{now_unix_ts, AppState, SessionStatus, WorkerRecord, WorkerStatus},
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use tokio::sync::broadcast;

pub struct Controller {
    workspace_root: PathBuf,
    pub config: Config,
    pub paths: CoordinationPaths,
    pub state: AppState,
    client: AppServerClient,
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
struct ThreadReadResponse {
    thread: ThreadSummary,
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

impl Controller {
    pub async fn start(workspace_root: PathBuf) -> Result<Self> {
        let config = Config::load(&workspace_root)?;
        let paths = config.coordination_paths(&workspace_root);
        let state = AppState::load(&paths.state_file)?;
        let client =
            AppServerClient::spawn(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await?;

        Ok(Self {
            workspace_root,
            config,
            paths,
            state,
            client,
        })
    }

    pub fn init_workspace(&mut self) -> Result<Option<PathBuf>> {
        let config_path = Config::write_default_config_if_missing(&self.workspace_root)?;
        self.paths.ensure_layout()?;
        self.state.save(&self.paths.state_file)?;
        self.write_master_status("idle", None, None)?;
        Ok(config_path)
    }

    pub async fn doctor(&self) -> Result<DoctorReport> {
        let mut client =
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
        let thread_ok = !response.thread.id.is_empty();
        let app_server_ok = client.is_running().await.unwrap_or(false);
        Ok(DoctorReport {
            config_source: self
                .workspace_root
                .join("codeclaw.toml")
                .display()
                .to_string(),
            coordination_root: self.paths.root.clone(),
            codex_app_server_ok: app_server_ok,
            thread_start_ok: thread_ok,
        })
    }

    pub async fn ensure_master_thread(&mut self) -> Result<String> {
        self.paths.ensure_layout()?;

        if let Some(thread_id) = self.state.master_thread_id.clone() {
            if self.thread_exists(&thread_id).await {
                self.write_master_status("idle", self.state.master_last_turn_id.clone(), None)?;
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
        self.state.master_thread_id = Some(thread_id.clone());
        self.state.master_last_turn_id = None;
        self.save_state()?;
        self.write_master_status("idle", None, None)?;
        Ok(thread_id)
    }

    pub async fn send_prompt(&mut self, target: PromptTarget, prompt: &str) -> Result<()> {
        match target {
            PromptTarget::Master => {
                let thread_id = self.ensure_master_thread().await?;
                let turn_id = self
                    .run_turn(&thread_id, "master", prompt, SessionRole::Master)
                    .await?;
                self.state.master_last_turn_id = Some(turn_id);
                self.save_state()?;
            }
            PromptTarget::Worker(worker_id) => {
                let worker = self
                    .state
                    .workers
                    .get(&worker_id)
                    .cloned()
                    .with_context(|| format!("unknown worker `{worker_id}`"))?;
                let turn_id = self
                    .run_turn(
                        &worker.thread_id,
                        &worker.id,
                        prompt,
                        SessionRole::Worker(worker.id.clone()),
                    )
                    .await?;
                self.update_worker_after_turn(
                    &worker.id,
                    WorkerStatus::Completed,
                    Some(turn_id),
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub async fn spawn_worker(&mut self, group: &str, task: &str) -> Result<WorkerRecord> {
        let group_config = self
            .config
            .group(group)
            .with_context(|| format!("unknown group `{group}`"))?
            .clone();
        self.paths.ensure_layout()?;

        let task_number = self.state.next_task_number;
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
        let mut record = WorkerRecord {
            id: worker_id.clone(),
            group: group.to_owned(),
            task: task.to_owned(),
            task_file: task_file.display().to_string(),
            thread_id: response.thread.id,
            status: WorkerStatus::Idle,
            created_at: now,
            updated_at: now,
            last_turn_id: None,
            last_message: None,
        };

        self.state.next_task_number += 1;
        self.state.workers.insert(worker_id.clone(), record.clone());
        self.save_state()?;
        self.write_worker_status(&record)?;

        match self
            .run_turn(
                &record.thread_id,
                &record.id,
                &worker_bootstrap_prompt(&record),
                SessionRole::Worker(record.id.clone()),
            )
            .await
        {
            Ok(turn_id) => {
                self.update_worker_after_turn(
                    &record.id,
                    WorkerStatus::Completed,
                    Some(turn_id),
                    None,
                )?;
            }
            Err(error) => {
                let message = error.to_string();
                self.update_worker_after_turn(
                    &record.id,
                    WorkerStatus::Failed,
                    None,
                    Some(message.clone()),
                )?;
                return Err(error);
            }
        }

        record = self
            .state
            .workers
            .get(&worker_id)
            .cloned()
            .with_context(|| format!("worker `{worker_id}` disappeared from state"))?;
        Ok(record)
    }

    pub fn list_workers(&self) -> Vec<&WorkerRecord> {
        self.state.workers.values().collect()
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

    async fn run_turn(
        &mut self,
        thread_id: &str,
        log_label: &str,
        prompt: &str,
        role: SessionRole,
    ) -> Result<String> {
        let mut receiver = self.client.subscribe();
        self.write_role_status(&role, "running", None, None)?;

        let response: TurnStartResponse = self
            .client
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [
                        {
                            "type": "text",
                            "text": prompt,
                            "text_elements": [],
                        }
                    ]
                }),
            )
            .await?;
        let turn_id = response.turn.id.clone();
        self.write_role_status(&role, "running", Some(turn_id.clone()), None)?;

        let mut printed_text = String::new();
        let mut streamed_delta = false;
        let mut final_error: Option<TurnError> = None;

        loop {
            let notification = receiver.recv().await.map_err(map_broadcast_error)?;
            self.log_notification(log_label, &notification)?;

            match notification.method.as_str() {
                "item/agentMessage/delta" => {
                    let event: AgentMessageDeltaNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn_id == turn_id {
                        print!("{}", event.delta);
                        io::stdout().flush().ok();
                        printed_text.push_str(&event.delta);
                        streamed_delta = true;
                    }
                }
                "item/completed" => {
                    let event: ItemLifecycleNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn_id == turn_id {
                        match event.item {
                            ThreadItem::AgentMessage { text } if !streamed_delta => {
                                print!("{text}");
                                io::stdout().flush().ok();
                                printed_text.push_str(&text);
                            }
                            ThreadItem::CommandExecution {
                                command,
                                status,
                                exit_code,
                                aggregated_output,
                            } => {
                                eprintln!(
                                    "\n[codeclaw][cmd][{log_label}] status={status} exit={:?} :: {}",
                                    exit_code, command
                                );
                                if let Some(output) = aggregated_output {
                                    let trimmed = output.trim();
                                    if !trimmed.is_empty() {
                                        eprintln!("{trimmed}");
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
                        self.write_role_status(
                            &role,
                            &thread_state_text(&event.status),
                            Some(turn_id.clone()),
                            None,
                        )?;
                    }
                }
                "turn/started" => {
                    let event: TurnStartedNotification =
                        serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn.id == turn_id {
                        self.write_role_status(
                            &role,
                            &event.turn.status,
                            Some(turn_id.clone()),
                            None,
                        )?;
                    }
                }
                "error" => {
                    let event: ErrorNotification = serde_json::from_value(notification.params)?;
                    if event.thread_id == thread_id && event.turn_id == turn_id {
                        eprintln!("\n[codeclaw][error][{log_label}] {}", event.error.message);
                        if let Some(details) = &event.error.additional_details {
                            eprintln!("{details}");
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
                        if !printed_text.is_empty() {
                            println!();
                        }

                        if let Some(error) = event.turn.error {
                            self.write_role_status(
                                &role,
                                &event.turn.status,
                                Some(turn_id.clone()),
                                Some(error.message.clone()),
                            )?;
                            return Err(anyhow!(error.message));
                        }

                        if let Some(error) = final_error {
                            self.write_role_status(
                                &role,
                                &event.turn.status,
                                Some(turn_id.clone()),
                                Some(error.message.clone()),
                            )?;
                            return Err(anyhow!(error.message));
                        }

                        self.write_role_status(
                            &role,
                            &event.turn.status,
                            Some(turn_id.clone()),
                            if printed_text.is_empty() {
                                None
                            } else {
                                Some(compact_message(&printed_text))
                            },
                        )?;
                        return Ok(turn_id);
                    }
                }
                _ => {}
            }
        }
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

    fn update_worker_after_turn(
        &mut self,
        worker_id: &str,
        status: WorkerStatus,
        last_turn_id: Option<String>,
        last_message: Option<String>,
    ) -> Result<()> {
        let worker = self
            .state
            .workers
            .get_mut(worker_id)
            .with_context(|| format!("unknown worker `{worker_id}`"))?;
        worker.status = status;
        worker.updated_at = now_unix_ts();
        if last_turn_id.is_some() {
            worker.last_turn_id = last_turn_id;
        }
        if last_message.is_some() {
            worker.last_message = last_message;
        }
        let snapshot = worker.clone();
        self.save_state()?;
        self.write_worker_status(&snapshot)
    }

    fn save_state(&self) -> Result<()> {
        self.state.save(&self.paths.state_file)
    }

    fn write_master_status(
        &self,
        state: &str,
        last_turn_id: Option<String>,
        last_message: Option<String>,
    ) -> Result<()> {
        let thread_id = self.state.master_thread_id.clone().unwrap_or_default();
        let status = SessionStatus {
            role: "master".to_owned(),
            thread_id,
            state: state.to_owned(),
            updated_at: now_unix_ts(),
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
            last_turn_id: worker.last_turn_id.clone(),
            last_message: worker.last_message.clone(),
        };
        status.write(&worker.status_path(&self.paths.status_dir))
    }

    fn write_role_status(
        &mut self,
        role: &SessionRole,
        state: &str,
        last_turn_id: Option<String>,
        last_message: Option<String>,
    ) -> Result<()> {
        match role {
            SessionRole::Master => self.write_master_status(state, last_turn_id, last_message),
            SessionRole::Worker(worker_id) => {
                let status = match state {
                    "completed" => WorkerStatus::Completed,
                    "failed" => WorkerStatus::Failed,
                    "running" | "inProgress" | "active" => WorkerStatus::Running,
                    _ => WorkerStatus::Idle,
                };
                self.update_worker_after_turn(worker_id, status, last_turn_id, last_message)
            }
        }
    }

    fn master_instructions(&self) -> String {
        format!(
            "You are the master controller for CodeClaw in {}. Coordinate work across workers, keep plans concise, and prefer actionable task splits.",
            self.workspace_root.display()
        )
    }
}

#[derive(Debug, Clone)]
enum SessionRole {
    Master,
    Worker(String),
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

fn thread_state_text(value: &Value) -> String {
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        kind.to_owned()
    } else {
        value.to_string()
    }
}

fn map_broadcast_error(error: broadcast::error::RecvError) -> anyhow::Error {
    anyhow!("app-server notification channel error: {error}")
}
