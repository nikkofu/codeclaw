mod app_server;
mod config;
mod controller;
mod gateway;
mod orchestration;
mod service;
mod session;
mod state;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use controller::{BatchSnapshot, Controller, CreateJobRequest, JobSnapshot, PromptTarget};
use gateway::{capabilities_for_channel, sample_inbound_event, sample_outbound_envelope};
use serde_json::to_string_pretty;
use service::{ServiceLifecycle, ServiceSnapshot};
use session::{SessionEventKind, SessionKind, SessionSnapshot};
use state::ReportChannel;
use std::{
    collections::BTreeSet,
    env,
    io::{self, IsTerminal, Write},
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
    Serve {
        #[arg(long, default_value_t = 1500)]
        interval_ms: u64,
        #[arg(long, default_value_t = 900)]
        stall_after_secs: u64,
        #[arg(long, default_value_t = false)]
        once: bool,
    },
    Spawn {
        #[arg(long)]
        group: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        job: Option<String>,
    },
    Send {
        #[arg(long, default_value = "master")]
        to: String,
        #[arg(long)]
        job: Option<String>,
        prompt: String,
    },
    Inspect {
        #[arg(long, conflicts_with_all = ["batch", "service"])]
        session: Option<String>,
        #[arg(long, conflicts_with_all = ["session", "service"])]
        batch: Option<u64>,
        #[arg(long, conflicts_with_all = ["session", "batch"])]
        service: bool,
        #[arg(long, default_value_t = 10)]
        events: usize,
        #[arg(long, default_value_t = 8)]
        output: usize,
    },
    List,
    Jobs,
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },
    Gateway {
        #[command(subcommand)]
        command: GatewayCommand,
    },
}

#[derive(Debug, Subcommand)]
enum JobCommand {
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        objective: Option<String>,
        #[arg(long, default_value = "cli")]
        channel: String,
        #[arg(long)]
        requester: Option<String>,
        #[arg(long, default_value = "normal")]
        priority: String,
        #[arg(long, default_value = "supervisor_worker")]
        pattern: String,
        #[arg(long, default_value_t = false)]
        approval_required: bool,
        #[arg(long)]
        context: Option<String>,
    },
    Inspect {
        job_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum GatewayCommand {
    Capabilities {
        #[arg(long, default_value = "console")]
        channel: String,
    },
    Schema,
    Subscribe {
        #[arg(long)]
        job: String,
        #[arg(long)]
        channel: String,
        #[arg(long)]
        target: Option<String>,
    },
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
        Command::Serve {
            interval_ms,
            stall_after_secs,
            once,
        } => run_serve(workspace_root, interval_ms, stall_after_secs, once).await,
        Command::Spawn { group, task, job } => {
            run_spawn(workspace_root, &group, &task, job.as_deref()).await
        }
        Command::Send { to, job, prompt } => {
            run_send(workspace_root, &to, job.as_deref(), &prompt).await
        }
        Command::Inspect {
            session,
            batch,
            service,
            events,
            output,
        } => run_inspect(workspace_root, session, batch, service, events, output).await,
        Command::List => run_list(workspace_root).await,
        Command::Jobs => run_jobs(workspace_root).await,
        Command::Job { command } => run_job(workspace_root, command).await,
        Command::Gateway { command } => run_gateway(workspace_root, command).await,
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

async fn run_serve(
    workspace_root: PathBuf,
    interval_ms: u64,
    stall_after_secs: u64,
    once: bool,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;

    let started_at = state::now_unix_ts();
    let interval = Duration::from_millis(interval_ms);
    let mut tick = 0u64;

    println!(
        "service starting | interval_ms={} stall_after_secs={}",
        interval_ms, stall_after_secs
    );
    controller.write_service_lifecycle(
        ServiceLifecycle::Starting,
        started_at,
        tick,
        Vec::new(),
        None,
    )?;

    loop {
        tick += 1;
        match controller
            .service_tick(started_at, tick, stall_after_secs)
            .await
        {
            Ok(snapshot) => {
                print_service_tick(&snapshot);
            }
            Err(error) => {
                controller.write_service_lifecycle(
                    ServiceLifecycle::Failed,
                    started_at,
                    tick,
                    Vec::new(),
                    Some(error.to_string()),
                )?;
                return Err(error);
            }
        }

        if once {
            controller.write_service_lifecycle(
                ServiceLifecycle::Stopped,
                started_at,
                tick,
                Vec::new(),
                None,
            )?;
            println!("service stopped | mode=once");
            return Ok(());
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to listen for ctrl-c")?;
                controller.write_service_lifecycle(
                    ServiceLifecycle::Stopped,
                    started_at,
                    tick,
                    Vec::new(),
                    None,
                )?;
                println!("service stopped | reason=ctrl-c");
                return Ok(());
            }
        }
    }
}

async fn run_spawn(
    workspace_root: PathBuf,
    group: &str,
    task: &str,
    job_id: Option<&str>,
) -> Result<()> {
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
    let result = if let Some(job_id) = job_id {
        controller
            .spawn_worker_and_wait_for_job(group, task, Some(job_id))
            .await
    } else {
        controller.spawn_worker_and_wait(group, task).await
    };
    let _ = stop_tx.send(true);
    let _ = progress.await;
    let worker = result?;
    println!("worker: {}", worker.id);
    println!(
        "job: {}",
        worker.job_id.clone().unwrap_or_else(|| "-".to_owned())
    );
    println!("thread: {}", worker.thread_id);
    println!("task file: {}", worker.task_file);
    println!("status: {}", worker.status);
    Ok(())
}

async fn run_send(
    workspace_root: PathBuf,
    to: &str,
    job_id: Option<&str>,
    prompt: &str,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    let target = if to == "master" {
        PromptTarget::Master
    } else {
        PromptTarget::Worker(to.to_owned())
    };
    if let Some(job_id) = job_id {
        controller
            .submit_prompt_and_wait_for_job(target, prompt, Some(job_id))
            .await?;
    } else {
        controller.submit_prompt_and_wait(target, prompt).await?;
    }
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
            "{} [{}] {} :: {} | job={}",
            worker.id,
            worker.group,
            worker.status,
            worker.task,
            worker.job_id.unwrap_or_else(|| "-".to_owned())
        );
    }

    Ok(())
}

async fn run_jobs(workspace_root: PathBuf) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    let jobs = controller.list_jobs();

    if jobs.is_empty() {
        println!("no jobs registered");
        return Ok(());
    }

    for job in jobs {
        println!(
            "{} [{}] {} :: batches={} workers={} | pattern={}",
            job.id,
            job.status,
            job.title,
            job.batch_ids.len(),
            job.worker_ids.len(),
            job.policy.pattern
        );
    }

    Ok(())
}

async fn run_job(workspace_root: PathBuf, command: JobCommand) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;

    match command {
        JobCommand::Create {
            title,
            objective,
            channel,
            requester,
            priority,
            pattern,
            approval_required,
            context,
        } => {
            controller.init_workspace()?;
            let objective = objective.unwrap_or_else(|| title.clone());
            let job = controller.create_job(CreateJobRequest {
                title,
                objective,
                source_channel: channel,
                requester,
                priority,
                pattern,
                approval_required,
                context,
            })?;
            println!("job: {}", job.id);
            println!("status: {}", job.status);
            println!("title: {}", job.title);
            println!("objective: {}", job.objective);
            println!("pattern: {}", job.policy.pattern);
            println!(
                "approval required: {}",
                if job.policy.approval_required {
                    "yes"
                } else {
                    "no"
                }
            );
            println!("next step: use --job {} with `send` or `spawn`", job.id);
            Ok(())
        }
        JobCommand::Inspect { job_id } => {
            let job = controller
                .job_snapshot(&job_id)
                .with_context(|| format!("unknown job `{job_id}`"))?;
            print_job_snapshot(&job);
            Ok(())
        }
    }
}

async fn run_gateway(workspace_root: PathBuf, command: GatewayCommand) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;

    match command {
        GatewayCommand::Capabilities { channel } => {
            let channel = parse_report_channel(&channel)?;
            let capabilities = capabilities_for_channel(&channel);

            println!("channel: {}", channel);
            println!(
                "default target: {}",
                gateway::default_target_for_channel(&channel, &controller.paths.root)
            );
            println!("platform: {}", capabilities.platform);
            println!("supports text: {}", yes_no(capabilities.supports_text));
            println!(
                "supports markdown: {}",
                yes_no(capabilities.supports_markdown)
            );
            println!("supports links: {}", yes_no(capabilities.supports_links));
            println!("supports images: {}", yes_no(capabilities.supports_images));
            println!("supports audio: {}", yes_no(capabilities.supports_audio));
            println!("supports video: {}", yes_no(capabilities.supports_video));
            println!("supports files: {}", yes_no(capabilities.supports_files));
            println!("supports typing: {}", yes_no(capabilities.supports_typing));
            println!(
                "supports raw type: {}",
                yes_no(capabilities.supports_raw_type)
            );
            println!(
                "supports raw event: {}",
                yes_no(capabilities.supports_raw_event)
            );
            println!(
                "supports raw hook: {}",
                yes_no(capabilities.supports_raw_hook)
            );
            println!();
            println!(
                "inbound event kinds: {}",
                capabilities
                    .inbound_event_kinds
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "outbound content kinds: {}",
                capabilities
                    .outbound_content_kinds
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Ok(())
        }
        GatewayCommand::Schema => {
            let inbound = sample_inbound_event();
            let outbound = sample_outbound_envelope();

            println!("normalized gateway compatibility contract");
            println!("supports: text, markdown, links, image, audio, video, file, typing");
            println!("raw fields: type, event, hook");
            println!();
            println!("sample inbound event:");
            println!("{}", to_string_pretty(&inbound)?);
            println!();
            println!("sample outbound envelope:");
            println!("{}", to_string_pretty(&outbound)?);
            Ok(())
        }
        GatewayCommand::Subscribe {
            job,
            channel,
            target,
        } => {
            controller.init_workspace()?;
            let channel = parse_report_channel(&channel)?;
            let subscription = controller.add_report_subscription(&job, channel, target)?;

            println!("subscription: SUB-{:03}", subscription.id);
            println!("job: {}", subscription.job_id);
            println!("channel: {}", subscription.channel);
            println!("target: {}", subscription.target);
            println!("notify kinds: accepted, progress, blocker, completion, failure, digest");
            Ok(())
        }
    }
}

async fn run_inspect(
    workspace_root: PathBuf,
    session_id: Option<String>,
    batch_id: Option<u64>,
    inspect_service: bool,
    events: usize,
    output: usize,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;

    if inspect_service {
        let snapshot = controller
            .service_snapshot()?
            .context("service snapshot not found")?;
        print_service_snapshot(&snapshot);
        return Ok(());
    }

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

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn parse_report_channel(value: &str) -> Result<ReportChannel> {
    value.parse::<ReportChannel>().map_err(anyhow::Error::msg)
}

async fn monitor_spawn_progress(
    controller: Controller,
    existing_workers: BTreeSet<String>,
    group: String,
    task: String,
    mut stop_rx: watch::Receiver<bool>,
) {
    let interactive = io::stderr().is_terminal();
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
                    clear_progress_line(interactive, &mut last_status_len);
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
            render_progress_line(&status_line, interactive, &mut last_status_len);
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

    clear_progress_line(interactive, &mut last_status_len);
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

fn render_progress_line(line: &str, interactive: bool, last_status_len: &mut usize) {
    if !interactive {
        eprintln!("{line}");
        return;
    }
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

fn clear_progress_line(interactive: bool, last_status_len: &mut usize) {
    if !interactive {
        return;
    }
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
    println!(
        "job: {}",
        session.job_id.clone().unwrap_or_else(|| "-".to_owned())
    );
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
    println!(
        "job: {}",
        batch.job_id.clone().unwrap_or_else(|| "-".to_owned())
    );
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

fn print_job_snapshot(job: &JobSnapshot) {
    println!("job: {}", job.id);
    println!("status: {}", job.status);
    println!("title: {}", job.title);
    println!("objective: {}", job.objective);
    println!("source channel: {}", job.source_channel);
    println!(
        "requester: {}",
        job.requester.clone().unwrap_or_else(|| "-".to_owned())
    );
    println!("priority: {}", job.priority);
    println!("pattern: {}", job.pattern);
    println!(
        "approval required: {}",
        if job.approval_required { "yes" } else { "no" }
    );
    println!("created: {}", job.created_at);
    println!("updated: {}", job.updated_at);
    println!(
        "latest summary: {}",
        job.latest_summary.clone().unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "latest report: {}",
        job.latest_report_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "next report due: {}",
        job.next_report_due_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "escalation state: {}",
        job.escalation_state
            .clone()
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "final outcome: {}",
        job.final_outcome.clone().unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "context: {}",
        job.context.clone().unwrap_or_else(|| "-".to_owned())
    );

    println!();
    println!("batches ({}):", job.batch_ids.len());
    for batch in &job.batch_ids {
        println!(
            "- b{:03} [{}] {} :: {} | updated={}",
            batch.id, batch.status, batch.root_session_id, batch.root_prompt, batch.updated_at
        );
    }
    if job.batch_ids.is_empty() {
        println!("- no batches");
    }

    println!();
    println!("workers ({}):", job.workers.len());
    for worker in &job.workers {
        println!(
            "- {} [{}] {} :: {}",
            worker.id, worker.status, worker.group, worker.task
        );
        println!(
            "  summary: {}",
            worker.summary.clone().unwrap_or_else(|| "-".to_owned())
        );
    }
    if job.workers.is_empty() {
        println!("- no workers");
    }

    println!();
    println!("reports ({}):", job.reports.len());
    for report in &job.reports {
        println!(
            "- RPT-{:03} [{}] [{}] {}",
            report.id, report.kind, report.status, report.summary
        );
        println!("  created: {}", report.created_at);
        println!("  body: {}", report.body);
    }
    if job.reports.is_empty() {
        println!("- no reports");
    }

    println!();
    println!("subscriptions ({}):", job.subscriptions.len());
    for subscription in &job.subscriptions {
        println!(
            "- SUB-{:03} [{}] {}",
            subscription.id, subscription.channel, subscription.target
        );
    }
    if job.subscriptions.is_empty() {
        println!("- no subscriptions");
    }

    println!();
    println!("deliveries ({}):", job.deliveries.len());
    for delivery in &job.deliveries {
        println!(
            "- DLY-{:03} -> RPT-{:03} [{}:{}] [{}] attempts={} updated={}",
            delivery.id,
            delivery.report_id,
            delivery.channel,
            delivery.target,
            delivery.status,
            delivery.attempts,
            delivery.updated_at
        );
        println!(
            "  last error: {}",
            delivery
                .last_error
                .clone()
                .unwrap_or_else(|| "-".to_owned())
        );
    }
    if job.deliveries.is_empty() {
        println!("- no deliveries");
    }
}

fn print_service_tick(snapshot: &ServiceSnapshot) {
    println!(
        "tick={} status={} pending={} running={} blocked={} completed={} failed={} workers={} dispatched={} reports={} queued_deliveries={} delivered={}",
        snapshot.tick,
        snapshot.status,
        snapshot.pending_jobs.len(),
        snapshot.running_jobs.len(),
        snapshot.blocked_jobs.len(),
        snapshot.completed_jobs.len(),
        snapshot.failed_jobs.len(),
        snapshot.running_workers.len(),
        if snapshot.dispatched_jobs.is_empty() {
            "-".to_owned()
        } else {
            snapshot.dispatched_jobs.join(",")
        },
        if snapshot.generated_reports.is_empty() {
            "-".to_owned()
        } else {
            snapshot.generated_reports.join(",")
        },
        snapshot.queued_deliveries.len(),
        if snapshot.delivered_notifications.is_empty() {
            "-".to_owned()
        } else {
            snapshot.delivered_notifications.join(" | ")
        }
    );
}

fn print_service_snapshot(snapshot: &ServiceSnapshot) {
    println!("status: {}", snapshot.status);
    println!("pid: {}", snapshot.pid);
    println!("started: {}", snapshot.started_at);
    println!("updated: {}", snapshot.updated_at);
    println!("tick: {}", snapshot.tick);
    println!(
        "master thread: {}",
        snapshot
            .master_thread_id
            .clone()
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "last error: {}",
        snapshot
            .last_error
            .clone()
            .unwrap_or_else(|| "-".to_owned())
    );

    print_named_ids("pending jobs", &snapshot.pending_jobs);
    print_named_ids("running jobs", &snapshot.running_jobs);
    print_named_ids("blocked jobs", &snapshot.blocked_jobs);
    print_named_ids("completed jobs", &snapshot.completed_jobs);
    print_named_ids("failed jobs", &snapshot.failed_jobs);
    print_named_ids("stalled jobs", &snapshot.stalled_jobs);
    print_named_ids("running workers", &snapshot.running_workers);
    print_named_ids("last dispatched jobs", &snapshot.dispatched_jobs);
    print_named_ids("last generated reports", &snapshot.generated_reports);
    print_named_ids("queued deliveries", &snapshot.queued_deliveries);
    print_named_ids(
        "last delivered notifications",
        &snapshot.delivered_notifications,
    );
}

fn print_named_ids(label: &str, ids: &[String]) {
    println!();
    println!("{label} ({}):", ids.len());
    if ids.is_empty() {
        println!("- none");
        return;
    }
    for id in ids {
        println!("- {id}");
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
