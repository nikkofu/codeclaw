mod app_server;
mod config;
mod controller;
mod orchestration;
mod session;
mod state;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use controller::{BatchSnapshot, Controller, PromptTarget};
use session::{SessionEventKind, SessionKind, SessionSnapshot};
use std::{env, path::PathBuf};

#[derive(Debug, Parser)]
#[command(name = "codeclaw")]
#[command(about = "Terminal-first control plane for Codex sessions")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    Doctor,
    Up,
    Spawn {
        #[arg(long)]
        group: String,
        #[arg(long)]
        task: String,
    },
    Send {
        #[arg(long, default_value = "master")]
        to: String,
        prompt: String,
    },
    Inspect {
        #[arg(long, conflicts_with = "batch")]
        session: Option<String>,
        #[arg(long, conflicts_with = "session")]
        batch: Option<u64>,
        #[arg(long, default_value_t = 10)]
        events: usize,
        #[arg(long, default_value_t = 8)]
        output: usize,
    },
    List,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let workspace_root = env::current_dir().context("failed to resolve current directory")?;

    match cli.command {
        Command::Init => run_init(workspace_root).await,
        Command::Doctor => run_doctor(workspace_root).await,
        Command::Up => run_up(workspace_root).await,
        Command::Spawn { group, task } => run_spawn(workspace_root, &group, &task).await,
        Command::Send { to, prompt } => run_send(workspace_root, &to, &prompt).await,
        Command::Inspect {
            session,
            batch,
            events,
            output,
        } => run_inspect(workspace_root, session, batch, events, output).await,
        Command::List => run_list(workspace_root).await,
    }
}

async fn run_init(workspace_root: PathBuf) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    let config_path = controller.init_workspace()?;

    if let Some(path) = config_path {
        println!("wrote {}", path.display());
    } else {
        println!("config already present");
    }
    println!("initialized {}", controller.paths.root.display());
    Ok(())
}

async fn run_doctor(workspace_root: PathBuf) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    let report = controller.doctor().await?;

    println!("config: {}", report.config_source);
    println!("coordination root: {}", report.coordination_root.display());
    println!("app-server: {}", ok(report.codex_app_server_ok));
    println!("thread/start probe: {}", ok(report.thread_start_ok));
    Ok(())
}

async fn run_up(workspace_root: PathBuf) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    ui::run(controller).await
}

async fn run_spawn(workspace_root: PathBuf, group: &str, task: &str) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    let worker = controller.spawn_worker_and_wait(group, task).await?;
    println!("worker: {}", worker.id);
    println!("thread: {}", worker.thread_id);
    println!("task file: {}", worker.task_file);
    println!("status: {}", worker.status);
    Ok(())
}

async fn run_send(workspace_root: PathBuf, to: &str, prompt: &str) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    let target = if to == "master" {
        PromptTarget::Master
    } else {
        PromptTarget::Worker(to.to_owned())
    };
    controller.submit_prompt_and_wait(target, prompt).await?;
    println!("submitted");
    Ok(())
}

async fn run_list(workspace_root: PathBuf) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    let workers = controller.list_workers();

    if workers.is_empty() {
        println!("no workers registered");
        return Ok(());
    }

    for worker in workers {
        println!(
            "{} [{}] {} :: {}",
            worker.id, worker.group, worker.status, worker.task
        );
    }

    Ok(())
}

async fn run_inspect(
    workspace_root: PathBuf,
    session_id: Option<String>,
    batch_id: Option<u64>,
    events: usize,
    output: usize,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;

    if let Some(batch_id) = batch_id {
        let batch = controller
            .batch_snapshot(batch_id)
            .with_context(|| format!("unknown batch `b{batch_id}`"))?;
        print_batch_snapshot(&batch, events);
        return Ok(());
    }

    let session_id = session_id.unwrap_or_else(|| "master".to_owned());
    let session = controller
        .session_snapshot(&session_id)
        .with_context(|| format!("unknown session `{session_id}`"))?;
    print_session_snapshot(&session, events, output);
    Ok(())
}

fn ok(value: bool) -> &'static str {
    if value {
        "ok"
    } else {
        "failed"
    }
}

fn print_session_snapshot(session: &SessionSnapshot, max_events: usize, max_output: usize) {
    println!("session: {}", session.id);
    println!("title: {}", session.title);
    println!("status: {}", session.status);
    println!("role: {}", session_role_label(session));
    println!("thread: {}", session.thread_id);
    println!("pending turns: {}", session.pending_turns);
    println!(
        "latest batch: {}",
        session
            .latest_batch_id
            .map(|batch_id| format!("b{batch_id}"))
            .unwrap_or_else(|| "-".to_owned())
    );
    let batch_ids = session_batch_ids(session);
    println!(
        "known batches: {}",
        if batch_ids.is_empty() {
            "-".to_owned()
        } else {
            batch_ids
                .into_iter()
                .map(|batch_id| format!("b{batch_id}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "summary: {}",
        session.summary.clone().unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "last message: {}",
        session
            .last_message
            .clone()
            .unwrap_or_else(|| "-".to_owned())
    );
    println!("location: {}", session_location(session));

    println!();
    println!(
        "recent timeline (showing {}/{}):",
        tail_len(session.timeline_events.len(), max_events),
        session.timeline_events.len()
    );
    for event in tail_refs(&session.timeline_events, max_events) {
        println!(
            "- {} [{}] {}",
            event
                .batch_id
                .map(|batch_id| format!("b{batch_id:03}"))
                .unwrap_or_else(|| "-----".to_owned()),
            event_kind_label(&event.kind),
            event.text
        );
    }
    if session.timeline_events.is_empty() {
        println!("- no events");
    }

    println!();
    println!(
        "recent output (showing {}/{}):",
        tail_len(session.log_lines.len(), max_output),
        session.log_lines.len()
    );
    for line in tail_refs(&session.log_lines, max_output) {
        println!("- {line}");
    }
    if session.log_lines.is_empty() {
        println!("- no output");
    }
}

fn print_batch_snapshot(batch: &BatchSnapshot, max_events: usize) {
    println!("batch: b{:03}", batch.id);
    println!("status: {}", batch.status);
    println!(
        "root session: {} ({})",
        batch.root_session_title, batch.root_session_id
    );
    println!("root prompt: {}", batch.root_prompt);
    println!("created: {}", batch.created_at);
    println!("updated: {}", batch.updated_at);
    println!(
        "last event: {}",
        batch.last_event.clone().unwrap_or_else(|| "-".to_owned())
    );

    println!();
    println!("sessions ({}):", batch.sessions.len());
    for session in &batch.sessions {
        println!("- {} [{}] {}", session.id, session.status, session.title);
    }
    if batch.sessions.is_empty() {
        println!("- no sessions");
    }

    println!();
    println!(
        "recent events (showing {}/{}):",
        tail_len(batch.events.len(), max_events),
        batch.events.len()
    );
    for event in tail_refs(&batch.events, max_events) {
        println!(
            "- {} [{}] {}",
            event.session_title,
            event_kind_label(&event.kind),
            event.text
        );
    }
    if batch.events.is_empty() {
        println!("- no events");
    }
}

fn session_role_label(session: &SessionSnapshot) -> String {
    match &session.kind {
        SessionKind::Master => "master".to_owned(),
        SessionKind::Worker { group, task, .. } => format!("worker:{group} :: {task}"),
    }
}

fn session_location(session: &SessionSnapshot) -> String {
    match &session.kind {
        SessionKind::Master => session.cwd.clone(),
        SessionKind::Worker { task_file, .. } => task_file.clone(),
    }
}

fn session_batch_ids(session: &SessionSnapshot) -> Vec<u64> {
    let mut batch_ids = Vec::new();
    for event in &session.timeline_events {
        if let Some(batch_id) = event.batch_id {
            if !batch_ids.contains(&batch_id) {
                batch_ids.push(batch_id);
            }
        }
    }
    batch_ids
}

fn event_kind_label(kind: &SessionEventKind) -> &'static str {
    match kind {
        SessionEventKind::User => "USR",
        SessionEventKind::Bootstrap => "BOT",
        SessionEventKind::Orchestrator => "ORC",
        SessionEventKind::Runtime => "RUN",
        SessionEventKind::System => "SYS",
        SessionEventKind::Command => "CMD",
        SessionEventKind::Status => "STS",
        SessionEventKind::Error => "ERR",
    }
}

fn tail_refs<T>(items: &[T], max_items: usize) -> &[T] {
    let start = items.len().saturating_sub(max_items);
    &items[start..]
}

fn tail_len(total: usize, max_items: usize) -> usize {
    total.min(max_items)
}
