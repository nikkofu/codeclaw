use crate::controller::{Controller, PromptTarget};
use anyhow::{Context, Result};
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn run(controller: &mut Controller) -> Result<()> {
    let master_thread_id = controller.ensure_master_thread().await?;
    println!("CodeClaw interactive controller");
    println!("master thread: {master_thread_id}");
    println!("type `/help` for commands; plain text is sent to the master thread");

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    loop {
        print!("codeclaw> ");
        io::stdout().flush().ok();

        let Some(line) = lines.next_line().await.context("failed to read stdin")? else {
            println!();
            return Ok(());
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(command) = line.strip_prefix('/') {
            if handle_command(controller, command).await? {
                return Ok(());
            }
            continue;
        }

        controller.send_prompt(PromptTarget::Master, line).await?;
    }
}

async fn handle_command(controller: &mut Controller, command: &str) -> Result<bool> {
    let parts = shell_words::split(command).context("failed to parse command")?;
    if parts.is_empty() {
        return Ok(false);
    }

    match parts[0].as_str() {
        "help" => {
            println!("/help");
            println!("/workers");
            println!("/spawn <group> <task>");
            println!("/send <worker-id> <prompt>");
            println!("/master <prompt>");
            println!("/quit");
        }
        "workers" => {
            let workers = controller.list_workers();
            if workers.is_empty() {
                println!("no workers registered");
            } else {
                for worker in workers {
                    println!(
                        "{} [{}] {} :: {}",
                        worker.id, worker.group, worker.status, worker.task
                    );
                }
            }
        }
        "spawn" => {
            if parts.len() < 3 {
                println!("usage: /spawn <group> <task>");
            } else {
                let group = &parts[1];
                let task = parts[2..].join(" ");
                let worker = controller.spawn_worker(group, &task).await?;
                println!("spawned worker {} -> {}", worker.id, worker.thread_id);
            }
        }
        "send" => {
            if parts.len() < 3 {
                println!("usage: /send <worker-id> <prompt>");
            } else {
                let worker_id = parts[1].clone();
                let prompt = parts[2..].join(" ");
                controller
                    .send_prompt(PromptTarget::Worker(worker_id), &prompt)
                    .await?;
            }
        }
        "master" => {
            if parts.len() < 2 {
                println!("usage: /master <prompt>");
            } else {
                let prompt = parts[1..].join(" ");
                controller
                    .send_prompt(PromptTarget::Master, &prompt)
                    .await?;
            }
        }
        "quit" | "exit" => return Ok(true),
        other => println!("unknown command `{other}`"),
    }

    Ok(false)
}
