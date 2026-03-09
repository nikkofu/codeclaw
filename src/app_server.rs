use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{broadcast, oneshot, Mutex},
};

#[derive(Debug, Clone)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

pub struct AppServerClient {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: PendingMap,
    notifications: broadcast::Sender<Notification>,
    next_id: AtomicU64,
}

impl AppServerClient {
    pub async fn spawn(client_name: &str, client_version: &str) -> Result<Self> {
        let mut command = Command::new("codex");
        command
            .arg("app-server")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .context("failed to start `codex app-server`")?;
        let stdin = child
            .stdin
            .take()
            .context("failed to capture app-server stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture app-server stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture app-server stderr")?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (notifications, _) = broadcast::channel(256);

        spawn_stdout_reader(stdout, Arc::clone(&pending), notifications.clone());
        spawn_stderr_reader(stderr);

        let client = Self {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            pending,
            notifications,
            next_id: AtomicU64::new(1),
        };

        let _: Value = client
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": client_name,
                        "version": client_version,
                    },
                    "capabilities": {
                        "experimentalApi": true,
                    }
                }),
            )
            .await
            .context("failed to initialize app-server session")?;

        Ok(client)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.notifications.subscribe()
    }

    pub async fn request<T>(&self, method: &str, params: Value) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut stdin = self.stdin.lock().await;
        let payload = serde_json::to_vec(&request).context("failed to encode JSON-RPC request")?;
        stdin
            .write_all(&payload)
            .await
            .context("failed to write request to app-server stdin")?;
        stdin
            .write_all(b"\n")
            .await
            .context("failed to write request delimiter to app-server stdin")?;
        stdin
            .flush()
            .await
            .context("failed to flush app-server stdin")?;
        drop(stdin);

        let response = receiver
            .await
            .map_err(|_| anyhow!("app-server closed before responding to `{method}`"))??;

        serde_json::from_value(response)
            .with_context(|| format!("failed to decode response for `{method}`"))
    }

    pub async fn is_running(&self) -> Result<bool> {
        let mut child = self.child.lock().await;
        Ok(child.try_wait()?.is_none())
    }
}

fn spawn_stdout_reader(
    stdout: tokio::process::ChildStdout,
    pending: PendingMap,
    notifications: broadcast::Sender<Notification>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let message = match serde_json::from_str::<Value>(line) {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("[codeclaw] failed to parse app-server message: {error}");
                    continue;
                }
            };

            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                if message.get("result").is_some() {
                    if let Some(sender) = pending.lock().await.remove(&id) {
                        let result = message
                            .get("result")
                            .cloned()
                            .ok_or_else(|| anyhow!("missing result payload in response"));
                        let _ = sender.send(result);
                    }
                    continue;
                }

                if let Some(error) = message.get("error") {
                    if let Some(sender) = pending.lock().await.remove(&id) {
                        let message_text = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown JSON-RPC error");
                        let _ = sender.send(Err(anyhow!("JSON-RPC error: {message_text}")));
                    }
                    continue;
                }
            }

            if let Some(method) = message.get("method").and_then(Value::as_str) {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let _ = notifications.send(Notification {
                    method: method.to_owned(),
                    params,
                });
            }
        }

        let mut pending = pending.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(anyhow!("app-server stdout closed")));
        }
    });
}

fn spawn_stderr_reader(stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            eprintln!("[codex] {line}");
        }
    });
}
