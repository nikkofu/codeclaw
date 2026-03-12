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
use std::{
    collections::BTreeSet,
    env,
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};
use tokio::sync::watch;

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
    let existing_workers = controller
        .list_workers()
        .into_iter()
        .map(|worker| worker.id)
        .collect::<BTreeSet<_>>();
    eprintln!("spawning worker [{group}] {task}");
    let (stop_tx, stop_rx) = watch::channel(false);
    let progress = tokio::spawn(monitor_spawn_progress(
        controller.clone(),
        existing_workers,
        group.to_owned(),
        task.to_owned(),
        stop_rx,
    ));
    let result = controller.spawn_worker_and_wait(group, task).await;
    let _ = stop_tx.send(true);
    let _ = progress.await;
    let worker = result?;
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

async fn monitor_spawn_progress(
    controller: Controller,
    existing_workers: BTreeSet<String>,
    group: String,
    task: String,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut tick = 0usize;
    let mut worker_id: Option<String> = None;
    let mut printed_log_lines = 0usize;
    let mut last_status_line = String::new();
    let mut last_status_len = 0usize;

    loop {
        if *stop_rx.borrow() {
            break;
        }

        if worker_id.is_none() {
            if let Some(worker) = controller
                .list_workers()
                .into_iter()
                .filter(|worker| {
                    !existing_workers.contains(&worker.id)
                        && worker.group == group
                        && worker.task == task
                })
                .max_by(|left, right| {
                    left.created_at
                        .cmp(&right.created_at)
                        .then_with(|| left.id.cmp(&right.id))
                })
            {
                worker_id = Some(worker.id.clone());
                printed_log_lines = 0;
                eprintln!("\nworker created: {}", worker.id);
                eprintln!("task file: {}", worker.task_file);
            }
        }

        let status_line = if let Some(worker_id) = worker_id.as_deref() {
            if let Some(session) = controller.session_snapshot(worker_id) {
                if session.log_lines.len() > printed_log_lines {
                    clear_progress_line(&mut last_status_len);
                    for line in session.log_lines.iter().skip(printed_log_lines) {
                        eprintln!("  {line}");
                    }
                    printed_log_lines = session.log_lines.len();
                }

                let detail = cli_status_detail(&session);
                format!(
                    "{} {} [{}] {}",
                    cli_spinner_frame(tick),
                    worker_id,
                    session.status,
                    detail
                )
            } else {
                format!(
                    "{} {} [waiting] refreshing session state",
                    cli_spinner_frame(tick),
                    worker_id
                )
            }
        } else {
            format!(
                "{} creating worker record for [{}] {}",
                cli_spinner_frame(tick),
                group,
                task
            )
        };

        if status_line != last_status_line {
            render_progress_line(&status_line, &mut last_status_len);
            last_status_line = status_line;
        }
        tick = tick.wrapping_add(1);

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(120)) => {}
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    break;
                }
            }
        }
    }

    clear_progress_line(&mut last_status_len);
}

fn cli_spinner_frame(tick: usize) -> &'static str {
    const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
    FRAMES[tick % FRAMES.len()]
}

fn cli_status_detail(session: &SessionSnapshot) -> String {
    truncate_cli(
        &session
            .lifecycle_note
            .clone()
            .or_else(|| session.last_message.clone())
            .or_else(|| session.summary.clone())
            .unwrap_or_else(|| "waiting for bootstrap".to_owned()),
        72,
    )
}

fn render_progress_line(line: &str, last_status_len: &mut usize) {
    let mut stderr = io::stderr();
    let padded = if line.len() < *last_status_len {
        format!("{line}{}", " ".repeat(*last_status_len - line.len()))
    } else {
        line.to_owned()
    };
    let _ = write!(stderr, "\r{padded}");
    let _ = stderr.flush();
    *last_status_len = padded.len();
}

fn clear_progress_line(last_status_len: &mut usize) {
    if *last_status_len == 0 {
        return;
    }
    let mut stderr = io::stderr();
    let _ = write!(stderr, "\r{}\r", " ".repeat(*last_status_len));
    let _ = stderr.flush();
    *last_status_len = 0;
}

fn truncate_cli(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_owned();
    }
    let keep = max_len.saturating_sub(3);
    format!("{}...", &text[..keep])
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
        "lifecycle note: {}",
        session
            .lifecycle_note
            .clone()
            .unwrap_or_else(|| "-".to_owned())
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
