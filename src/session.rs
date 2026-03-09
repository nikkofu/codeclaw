use crate::state::WorkerRecord;
use std::collections::VecDeque;

const MAX_LOG_LINES: usize = 512;

#[derive(Debug, Clone)]
pub enum SessionKind {
    Master,
    Worker {
        group: String,
        task: String,
        task_file: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub id: String,
    pub thread_id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: SessionKind,
    pub status: String,
    pub cwd: String,
    pub last_turn_id: Option<String>,
    pub last_message: Option<String>,
    pub log_lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionView {
    id: String,
    thread_id: String,
    title: String,
    subtitle: String,
    kind: SessionKind,
    status: String,
    cwd: String,
    last_turn_id: Option<String>,
    last_message: Option<String>,
    lines: VecDeque<String>,
    live_buffer: String,
}

impl SessionView {
    pub fn master(thread_id: String, cwd: String) -> Self {
        Self {
            id: "master".to_owned(),
            thread_id,
            title: "master".to_owned(),
            subtitle: "Primary planner and dispatcher".to_owned(),
            kind: SessionKind::Master,
            status: "idle".to_owned(),
            cwd,
            last_turn_id: None,
            last_message: None,
            lines: VecDeque::new(),
            live_buffer: String::new(),
        }
    }

    pub fn from_worker(worker: &WorkerRecord, cwd: String) -> Self {
        let subtitle = worker
            .last_message
            .clone()
            .unwrap_or_else(|| worker.task.clone());
        Self {
            id: worker.id.clone(),
            thread_id: worker.thread_id.clone(),
            title: format!("[{}] {}", worker.group, worker.task),
            subtitle,
            kind: SessionKind::Worker {
                group: worker.group.clone(),
                task: worker.task.clone(),
                task_file: worker.task_file.clone(),
            },
            status: worker.status.to_string(),
            cwd,
            last_turn_id: worker.last_turn_id.clone(),
            last_message: worker.last_message.clone(),
            lines: VecDeque::new(),
            live_buffer: String::new(),
        }
    }

    pub fn set_thread_id(&mut self, thread_id: String) {
        self.thread_id = thread_id;
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub fn set_last_turn_id(&mut self, turn_id: Option<String>) {
        self.last_turn_id = turn_id;
    }

    pub fn set_last_message(&mut self, message: Option<String>) {
        if let Some(message) = message {
            self.subtitle = message.clone();
            self.last_message = Some(message);
        }
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push_back(line.into());
        trim_lines(&mut self.lines);
    }

    pub fn append_live_chunk(&mut self, chunk: &str) {
        self.live_buffer.push_str(chunk);
    }

    pub fn commit_live_buffer(&mut self) -> Option<String> {
        if self.live_buffer.trim().is_empty() {
            self.live_buffer.clear();
            return None;
        }

        let committed = std::mem::take(&mut self.live_buffer);
        self.push_line(format!("assistant> {committed}"));
        Some(committed)
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let mut log_lines = self.lines.iter().cloned().collect::<Vec<_>>();
        if !self.live_buffer.is_empty() {
            log_lines.push(format!("assistant> {}", self.live_buffer));
        }

        SessionSnapshot {
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            kind: self.kind.clone(),
            status: self.status.clone(),
            cwd: self.cwd.clone(),
            last_turn_id: self.last_turn_id.clone(),
            last_message: self.last_message.clone(),
            log_lines,
        }
    }
}

fn trim_lines(lines: &mut VecDeque<String>) {
    while lines.len() > MAX_LOG_LINES {
        lines.pop_front();
    }
}
