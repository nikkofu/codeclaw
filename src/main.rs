mod app_server;
mod config;
mod controller;
mod orchestration;
mod session;
mod state;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use controller::{Controller, PromptTarget};
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

fn ok(value: bool) -> &'static str {
    if value {
        "ok"
    } else {
        "failed"
    }
}
