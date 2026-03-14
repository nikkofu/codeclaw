mod app_server;
mod config;
mod controller;
mod gateway;
mod logging;
mod orchestration;
mod service;
mod session;
mod state;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use controller::{
    job_intake_prompt, BatchSnapshot, Controller, CreateJobRequest, CreateSessionAutomationRequest,
    JobSnapshot, PromptTarget, SessionAutomationSnapshot,
};
use gateway::{capabilities_for_channel, sample_inbound_event, sample_outbound_envelope};
use serde_json::to_string_pretty;
use service::{RuntimeSnapshot, ServiceLifecycle, ServiceSnapshot};
use session::{SessionEventKind, SessionKind, SessionSnapshot};
use state::{JobRecord, ReportChannel};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    future::Future,
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
    Automation {
        #[command(subcommand)]
        command: AutomationCommand,
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
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        #[arg(long, default_value_t = false)]
        delegate_master_loop: bool,
        #[arg(long)]
        continue_for_secs: Option<u64>,
        #[arg(long)]
        continue_max_iterations: Option<u32>,
        #[arg(long)]
        context: Option<String>,
        #[arg(long, default_value_t = false)]
        defer: bool,
        #[arg(long, default_value_t = false)]
        follow: bool,
        #[arg(long, conflicts_with = "start_group")]
        start_session: Option<String>,
        #[arg(long, conflicts_with = "start_session")]
        start_group: Option<String>,
    },
    Inspect {
        job_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    Create {
        #[arg(long)]
        to: String,
        #[arg(long)]
        every_secs: u64,
        #[arg(long)]
        max_runs: Option<u32>,
        #[arg(long)]
        for_secs: Option<u64>,
        prompt: String,
    },
    List,
    Pause {
        automation_id: String,
    },
    Resume {
        automation_id: String,
    },
    Cancel {
        automation_id: String,
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

#[derive(Debug, Clone)]
enum JobStartTarget {
    Master,
    ExistingSession(String),
    NewWorkerGroup(String),
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
        Command::Automation { command } => run_automation(workspace_root, command).await,
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
    with_runtime_session(
        controller.clone(),
        "up",
        "cargo run -- up".to_owned(),
        ui::run(controller),
    )
    .await
}

async fn run_serve(
    workspace_root: PathBuf,
    interval_ms: u64,
    stall_after_secs: u64,
    once: bool,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    with_runtime_session(
        controller.clone(),
        "serve",
        if once {
            "cargo run -- serve --once".to_owned()
        } else {
            "cargo run -- serve".to_owned()
        },
        async move {
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
        },
    )
    .await
}

async fn run_spawn(
    workspace_root: PathBuf,
    group: &str,
    task: &str,
    job_id: Option<&str>,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    with_runtime_session(
        controller.clone(),
        "spawn",
        format!("cargo run -- spawn --group {group} --task {task}"),
        async move {
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
        },
    )
    .await
}

async fn run_send(
    workspace_root: PathBuf,
    to: &str,
    job_id: Option<&str>,
    prompt: &str,
) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;
    with_runtime_session(
        controller.clone(),
        "send",
        format!("cargo run -- send --to {to}"),
        async move {
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
        },
    )
    .await
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
            auto_approve,
            delegate_master_loop,
            continue_for_secs,
            continue_max_iterations,
            context,
            defer,
            follow,
            start_session,
            start_group,
        } => {
            controller.init_workspace()?;
            with_runtime_session(
                controller.clone(),
                "job_create",
                format!("cargo run -- job create --title {}", title),
                async move {
                    let objective = objective.unwrap_or_else(|| title.clone());
                    let job = controller.create_job(CreateJobRequest {
                        title,
                        objective,
                        source_channel: channel,
                        requester,
                        priority,
                        pattern,
                        approval_required,
                        auto_approve,
                        delegate_to_master_loop: delegate_master_loop,
                        continue_for_secs,
                        continue_max_iterations,
                        context,
                    })?;
                    let start_target = resolve_job_start_target(
                        &controller,
                        start_session.as_deref(),
                        start_group.as_deref(),
                    )?;

                    println!("job: {}", job.id);
                    println!("received: yes");
                    println!("start target: {}", job_start_target_label(&start_target));
                    println!("policy: {}", job_policy_summary(&job));
                    if defer {
                        println!("start now: no");
                        println!("status: queued");
                        if job.policy.delegate_to_master_loop {
                            println!(
                                "next step: run `cargo run -- serve` to intake and continue delegated jobs"
                            );
                        } else {
                            println!("next step: use --job {} with `send` or `spawn`", job.id);
                        }
                        return Ok(());
                    }

                    let service_running = controller
                        .service_snapshot()?
                        .map(|snapshot| {
                            matches!(
                                snapshot.status,
                                ServiceLifecycle::Running | ServiceLifecycle::Starting
                            )
                        })
                        .unwrap_or(false);

                    match start_target {
                        JobStartTarget::Master => {
                            println!("status: starting");
                            let batch_id = start_job_on_existing_session(
                                &controller,
                                &job,
                                PromptTarget::Master,
                                follow,
                            )
                            .await?;
                            let snapshot = controller
                                .job_snapshot(&job.id)
                                .with_context(|| format!("unknown job `{}` after intake", job.id))?;
                            print_job_start_result(&snapshot, Some(batch_id), None, service_running);
                        }
                        JobStartTarget::ExistingSession(session_id) => {
                            println!("status: starting");
                            let batch_id = start_job_on_existing_session(
                                &controller,
                                &job,
                                PromptTarget::Worker(session_id.clone()),
                                follow,
                            )
                            .await?;
                            let snapshot = controller
                                .job_snapshot(&job.id)
                                .with_context(|| format!("unknown job `{}` after dispatch", job.id))?;
                            print_job_start_result(
                                &snapshot,
                                Some(batch_id),
                                Some(session_id),
                                service_running,
                            );
                        }
                        JobStartTarget::NewWorkerGroup(group) => {
                            println!("status: starting");
                            let worker =
                                start_job_in_new_worker_group(&controller, &job, &group, follow)
                                    .await?;
                            let snapshot = controller.job_snapshot(&job.id).with_context(|| {
                                format!("unknown job `{}` after worker bootstrap", job.id)
                            })?;
                            print_job_start_result(
                                &snapshot,
                                worker.latest_batch_id,
                                Some(worker.id),
                                service_running,
                            );
                        }
                    }

                    Ok(())
                },
            )
            .await
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

async fn run_automation(workspace_root: PathBuf, command: AutomationCommand) -> Result<()> {
    let controller = Controller::start(workspace_root).await?;
    controller.init_workspace()?;

    match command {
        AutomationCommand::Create {
            to,
            every_secs,
            max_runs,
            for_secs,
            prompt,
        } => {
            let automation =
                controller.create_session_automation(CreateSessionAutomationRequest {
                    target_session_id: to,
                    prompt,
                    interval_secs: every_secs,
                    max_runs,
                    run_for_secs: for_secs,
                })?;
            println!("automation: {}", automation.id);
            println!("target: {}", automation.target_session_id);
            println!("interval secs: {}", automation.interval_secs);
            println!(
                "max runs: {}",
                automation
                    .max_runs
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_owned())
            );
            println!(
                "run for secs: {}",
                automation
                    .run_for_secs
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_owned())
            );
            println!("status: {}", automation.status);
            let scheduler_running = controller
                .service_snapshot()?
                .map(|snapshot| matches!(snapshot.status, ServiceLifecycle::Running))
                .unwrap_or(false);
            if scheduler_running {
                println!("scheduler: running");
            } else {
                println!("scheduler: idle");
                println!("next step: keep `cargo run -- up` open or run `cargo run -- serve`");
            }
        }
        AutomationCommand::List => {
            let automations = controller.list_session_automations();
            if automations.is_empty() {
                println!("no automations registered");
            } else {
                for automation in &automations {
                    print_session_automation_snapshot(automation);
                }
            }
        }
        AutomationCommand::Pause { automation_id } => {
            let automation = controller.pause_session_automation(&automation_id)?;
            println!(
                "paused: {} -> {}",
                automation.id, automation.target_session_id
            );
        }
        AutomationCommand::Resume { automation_id } => {
            let automation = controller.resume_session_automation(&automation_id)?;
            println!(
                "resumed: {} -> {}",
                automation.id, automation.target_session_id
            );
        }
        AutomationCommand::Cancel { automation_id } => {
            let automation = controller.cancel_session_automation(&automation_id)?;
            println!(
                "cancelled: {} -> {}",
                automation.id, automation.target_session_id
            );
        }
    }

    Ok(())
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
        let service = controller.service_snapshot()?;
        let runtime = controller.runtime_snapshot()?;
        if service.is_none() && runtime.is_none() {
            anyhow::bail!("service and runtime snapshots not found");
        }
        if let Some(snapshot) = service {
            print_service_snapshot(&snapshot);
        } else {
            println!("scheduler snapshot: not found");
        }
        if let Some(snapshot) = runtime {
            println!();
            print_runtime_snapshot(&snapshot);
        } else {
            println!();
            println!("runtime snapshot: not found");
        }
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

async fn with_runtime_session<T, Fut>(
    controller: Controller,
    mode: &str,
    command_label: String,
    future: Fut,
) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    controller.begin_runtime_session(mode, command_label)?;
    let result = future.await;
    match &result {
        Ok(_) => controller.finish_runtime_session(ServiceLifecycle::Stopped, None)?,
        Err(error) => {
            controller.finish_runtime_session(ServiceLifecycle::Failed, Some(error.to_string()))?
        }
    }
    result
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

fn resolve_job_start_target(
    controller: &Controller,
    start_session: Option<&str>,
    start_group: Option<&str>,
) -> Result<JobStartTarget> {
    if let Some(group) = start_group.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(JobStartTarget::NewWorkerGroup(group.to_owned()));
    }

    let Some(session_id) = start_session
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(JobStartTarget::Master);
    };

    if session_id == "master" {
        return Ok(JobStartTarget::Master);
    }
    if session_id == "onboard" {
        anyhow::bail!("`onboard` is a virtual supervisor session and cannot execute a job turn");
    }

    let session = controller
        .session_snapshot(session_id)
        .with_context(|| format!("unknown session `{session_id}`"))?;
    match session.kind {
        SessionKind::Worker { .. } => Ok(JobStartTarget::ExistingSession(session_id.to_owned())),
        SessionKind::Master => Ok(JobStartTarget::Master),
        SessionKind::Onboard => {
            anyhow::bail!("`onboard` is a virtual supervisor session and cannot execute a job turn")
        }
    }
}

fn job_start_target_label(target: &JobStartTarget) -> String {
    match target {
        JobStartTarget::Master => "master".to_owned(),
        JobStartTarget::ExistingSession(session_id) => format!("session:{session_id}"),
        JobStartTarget::NewWorkerGroup(group) => format!("new-worker:{group}"),
    }
}

fn job_policy_summary(job: &JobRecord) -> String {
    let mut fields = Vec::new();
    if job.policy.delegate_to_master_loop {
        fields.push("delegate-master-loop".to_owned());
    }
    if job.policy.auto_approve {
        fields.push("auto-approve".to_owned());
    }
    if let Some(value) = job.policy.continue_for_secs {
        fields.push(format!("continue-for-secs={value}"));
    }
    if let Some(value) = job.policy.continue_max_iterations {
        fields.push(format!("continue-max-iterations={value}"));
    }
    if fields.is_empty() {
        "manual".to_owned()
    } else {
        fields.join(", ")
    }
}

fn job_execution_prompt(job: &JobRecord, target: &PromptTarget) -> String {
    match target {
        PromptTarget::Master => job_intake_prompt(job),
        PromptTarget::Worker(worker_id) => {
            let context = job.context.clone().unwrap_or_else(|| "none".to_owned());
            format!(
                "Job execution request for an existing CodeClaw session.\n\nJob id: {}\nWorker session: {}\nTitle: {}\nObjective: {}\nPriority: {}\nApproval required: {}\nContext: {}\n\nExecute the job from this session if it fits your current role and workspace. Keep the summary concise, report blockers clearly, and hand work back when a different session should continue.",
                job.id,
                worker_id,
                job.title,
                job.objective,
                job.priority,
                if job.policy.approval_required { "yes" } else { "no" },
                context
            )
        }
    }
}

async fn start_job_on_existing_session(
    controller: &Controller,
    job: &JobRecord,
    target: PromptTarget,
    follow: bool,
) -> Result<u64> {
    let existing_log_lines = if follow {
        controller
            .sessions_snapshot()
            .into_iter()
            .map(|session| (session.id, session.log_lines.len()))
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    let prompt = job_execution_prompt(job, &target);
    let batch_id = controller
        .submit_prompt_for_job_with_batch(target, &prompt, Some(&job.id))
        .await?;
    if follow {
        let (stop_tx, stop_rx) = watch::channel(false);
        let progress = tokio::spawn(monitor_job_progress(
            controller.clone(),
            job.id.clone(),
            batch_id,
            existing_log_lines,
            stop_rx,
        ));
        let result = controller.wait_for_batch_completion(batch_id).await;
        let _ = stop_tx.send(true);
        let _ = progress.await;
        result?;
    } else {
        controller.wait_for_batch_completion(batch_id).await?;
    }
    Ok(batch_id)
}

async fn start_job_in_new_worker_group(
    controller: &Controller,
    job: &JobRecord,
    group: &str,
    follow: bool,
) -> Result<SessionSnapshot> {
    let existing_workers = if follow {
        controller
            .list_workers()
            .into_iter()
            .map(|worker| worker.id)
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let monitor = if follow {
        let (stop_tx, stop_rx) = watch::channel(false);
        let progress = tokio::spawn(monitor_spawn_progress(
            controller.clone(),
            existing_workers,
            group.to_owned(),
            job.title.clone(),
            stop_rx,
        ));
        Some((stop_tx, progress))
    } else {
        None
    };
    let (_, worker) = controller
        .spawn_worker_and_wait_for_job_with_batch(group, &job.title, Some(&job.id))
        .await?;
    if let Some((stop_tx, progress)) = monitor {
        let _ = stop_tx.send(true);
        let _ = progress.await;
    }
    controller
        .session_snapshot(&worker.id)
        .with_context(|| format!("missing worker session `{}` after bootstrap", worker.id))
}

fn print_job_start_result(
    snapshot: &JobSnapshot,
    batch_id: Option<u64>,
    session_id: Option<String>,
    service_running: bool,
) {
    println!("start now: yes");
    if let Some(batch_id) = batch_id {
        println!("batch: b{batch_id:03}");
    }
    if let Some(session_id) = session_id {
        println!("session: {session_id}");
    }
    println!("status: {}", snapshot.status);
    if let Some(summary) = snapshot
        .latest_summary
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        println!("summary: {summary}");
    }
    if snapshot.delegate_to_master_loop {
        if service_running {
            println!("automation: delegated loop armed; service running");
        } else {
            println!(
                "automation: delegated loop armed; run `cargo run -- serve` to keep it continuing"
            );
        }
    }
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
                progress_log_line(interactive, &format!("\nworker created: {}", worker.id));
                progress_log_line(interactive, &format!("task file: {}", worker.task_file));
            }
        }

        let status_body = if let Some(worker_id) = worker_id.as_deref() {
            if let Some(session) = controller.session_snapshot(worker_id) {
                if session.log_lines.len() > printed_log_lines {
                    clear_progress_line(interactive, &mut last_status_len);
                    for line in session.log_lines.iter().skip(printed_log_lines) {
                        progress_log_line(interactive, &format!("  {line}"));
                    }
                    printed_log_lines = session.log_lines.len();
                }

                let detail = cli_status_detail(&session);
                format!("{} [{}] {}", worker_id, session.status, detail)
            } else {
                format!("{} [waiting] refreshing session state", worker_id)
            }
        } else {
            format!("creating worker record for [{}] {}", group, task)
        };
        let status_line = if interactive {
            format!("{} {status_body}", cli_spinner_frame(tick))
        } else {
            status_body
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

async fn monitor_job_progress(
    controller: Controller,
    job_id: String,
    batch_id: u64,
    mut printed_log_lines: BTreeMap<String, usize>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let interactive = io::stderr().is_terminal();
    let mut tick = 0usize;
    let mut last_status_line = String::new();
    let mut last_status_len = 0usize;

    loop {
        if *stop_rx.borrow() {
            break;
        }

        let batch = controller.batch_snapshot(batch_id);
        let job = controller.job_snapshot(&job_id);

        if let Some(batch) = &batch {
            for session in &batch.sessions {
                if let Some(snapshot) = controller.session_snapshot(&session.id) {
                    let printed = printed_log_lines
                        .entry(session.id.clone())
                        .or_insert(0usize);
                    if snapshot.log_lines.len() > *printed {
                        clear_progress_line(interactive, &mut last_status_len);
                        for line in snapshot.log_lines.iter().skip(*printed) {
                            progress_log_line(interactive, &format!("  [{}] {line}", snapshot.id));
                        }
                        *printed = snapshot.log_lines.len();
                    }
                }
            }
        }

        let status_body = if let Some(batch) = &batch {
            let job_status = job
                .as_ref()
                .map(|snapshot| snapshot.status.clone())
                .unwrap_or_else(|| batch.status.clone());
            let detail = job
                .as_ref()
                .and_then(|snapshot| snapshot.latest_summary.clone())
                .or_else(|| batch.last_event.clone())
                .unwrap_or_else(|| "waiting for Codex planning".to_owned());
            format!(
                "{} [job={} batch=b{:03} sessions={}] {}",
                job_id,
                job_status,
                batch_id,
                batch.sessions.len(),
                truncate_cli(&detail, 72)
            )
        } else {
            format!("{} [starting] creating initial master batch", job_id)
        };
        let status_line = if interactive {
            format!("{} {status_body}", cli_spinner_frame(tick))
        } else {
            status_body
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
        println!("{line}");
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

fn progress_log_line(interactive: bool, line: &str) {
    if interactive {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
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
    println!(
        "auto approve: {}",
        if job.auto_approve { "yes" } else { "no" }
    );
    println!(
        "delegate master loop: {}",
        if job.delegate_to_master_loop {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "continue budget secs: {}",
        job.continue_for_secs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "continue max iterations: {}",
        job.continue_max_iterations
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
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
    println!("automation state: {}", job.automation.state);
    println!(
        "automation started: {}",
        job.automation
            .automation_started_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "last continue: {}",
        job.automation
            .last_continue_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "continue iterations used: {}",
        job.automation.continue_iterations
    );
    println!(
        "remaining budget secs: {}",
        job.automation
            .remaining_secs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "remaining budget iterations: {}",
        job.automation
            .remaining_iterations
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
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

fn print_session_automation_snapshot(automation: &SessionAutomationSnapshot) {
    println!(
        "{} [{}] -> {} | every={}s runs={} next={} last={} batch={}",
        automation.id,
        automation.status,
        automation.target_session_id,
        automation.interval_secs,
        automation.run_count,
        automation
            .next_run_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        automation
            .last_run_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        automation
            .last_batch_id
            .map(|value| format!("b{value}"))
            .unwrap_or_else(|| "-".to_owned())
    );
    println!("  prompt: {}", automation.prompt_preview);
    println!(
        "  remaining runs: {} | remaining secs: {} | max runs: {} | window secs: {}",
        automation
            .remaining_runs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        automation
            .remaining_secs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        automation
            .max_runs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        automation
            .run_for_secs
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    if let Some(error) = &automation.last_error {
        println!("  last error: {}", error);
    }
}

fn print_service_tick(snapshot: &ServiceSnapshot) {
    println!(
        "tick={} status={} pending={} running={} blocked={} completed={} failed={} workers={} dispatched={} continued={} reports={} queued_deliveries={} delivered={} auto_approve={} delegated={} exhausted={}",
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
        if snapshot.continued_jobs.is_empty() {
            "-".to_owned()
        } else {
            snapshot.continued_jobs.join(",")
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
        },
        snapshot.auto_approve_jobs.len(),
        snapshot.delegated_jobs.len(),
        snapshot.budget_exhausted_jobs.len()
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
    print_named_ids("last continued jobs", &snapshot.continued_jobs);
    print_named_ids("last generated reports", &snapshot.generated_reports);
    print_named_ids("queued deliveries", &snapshot.queued_deliveries);
    print_named_ids("delegated jobs", &snapshot.delegated_jobs);
    print_named_ids("auto approve jobs", &snapshot.auto_approve_jobs);
    print_named_ids("budget exhausted jobs", &snapshot.budget_exhausted_jobs);
    print_named_ids(
        "last delivered notifications",
        &snapshot.delivered_notifications,
    );
}

fn print_runtime_snapshot(snapshot: &RuntimeSnapshot) {
    println!("runtime status: {}", snapshot.status);
    println!("mode: {}", snapshot.mode);
    println!("pid: {}", snapshot.pid);
    println!(
        "app-server pid: {}",
        snapshot
            .app_server_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    println!(
        "app-server connected: {}",
        yes_no(snapshot.app_server_connected)
    );
    println!("started: {}", snapshot.started_at);
    println!("updated: {}", snapshot.updated_at);
    println!("command: {}", snapshot.command_label);
    println!(
        "master thread: {}",
        snapshot
            .master_thread_id
            .clone()
            .unwrap_or_else(|| "-".to_owned())
    );
    println!("active turns: {}", snapshot.active_turns);
    println!("queued turns: {}", snapshot.queued_turns);
    println!(
        "last error: {}",
        snapshot
            .last_error
            .clone()
            .unwrap_or_else(|| "-".to_owned())
    );

    print_named_ids("active sessions", &snapshot.active_sessions);
    print_named_ids("queued sessions", &snapshot.queued_sessions);
    print_named_ids("running workers", &snapshot.running_workers);
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
        SessionKind::Onboard => "onboard".to_owned(),
        SessionKind::Master => "master".to_owned(),
        SessionKind::Worker { group, task, .. } => format!("worker:{group} :: {task}"),
    }
}

fn session_location(session: &SessionSnapshot) -> String {
    match &session.kind {
        SessionKind::Onboard => session.cwd.clone(),
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
