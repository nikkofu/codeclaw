use crate::state::WorkerRecord;
use std::collections::VecDeque;

const MAX_LOG_LINES: usize = 512;
const MAX_TIMELINE_EVENTS: usize = 128;
const DEFAULT_MASTER_SUMMARY: &str = "Primary planner and dispatcher";

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
    pub pending_turns: usize,
    pub summary: Option<String>,
    pub kind: SessionKind,
    pub status: String,
    pub cwd: String,
    pub last_turn_id: Option<String>,
    pub last_message: Option<String>,
    pub timeline_events: Vec<SessionEvent>,
    pub log_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEventKind {
    User,
    Bootstrap,
    Orchestrator,
    Runtime,
    System,
    Command,
    Status,
    Error,
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub kind: SessionEventKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionView {
    id: String,
    thread_id: String,
    title: String,
    summary: Option<String>,
    kind: SessionKind,
    status: String,
    pending_turns: usize,
    cwd: String,
    last_turn_id: Option<String>,
    last_message: Option<String>,
    timeline_events: VecDeque<SessionEvent>,
    lines: VecDeque<String>,
    live_buffer: String,
}

impl SessionView {
    pub fn master(
        thread_id: String,
        cwd: String,
        summary: Option<String>,
        last_message: Option<String>,
    ) -> Self {
        Self {
            id: "master".to_owned(),
            thread_id,
            title: "master".to_owned(),
            summary: summary.or_else(|| Some(DEFAULT_MASTER_SUMMARY.to_owned())),
            kind: SessionKind::Master,
            status: "idle".to_owned(),
            pending_turns: 0,
            cwd,
            last_turn_id: None,
            last_message,
            timeline_events: VecDeque::new(),
            lines: VecDeque::new(),
            live_buffer: String::new(),
        }
    }

    pub fn from_worker(worker: &WorkerRecord, cwd: String) -> Self {
        Self {
            id: worker.id.clone(),
            thread_id: worker.thread_id.clone(),
            title: format!("[{}] {}", worker.group, worker.task),
            summary: worker.summary.clone(),
            kind: SessionKind::Worker {
                group: worker.group.clone(),
                task: worker.task.clone(),
                task_file: worker.task_file.clone(),
            },
            status: worker.status.to_string(),
            pending_turns: 0,
            cwd,
            last_turn_id: worker.last_turn_id.clone(),
            last_message: worker.last_message.clone(),
            timeline_events: VecDeque::new(),
            lines: VecDeque::new(),
            live_buffer: String::new(),
        }
    }

    pub fn set_thread_id(&mut self, thread_id: String) {
        self.thread_id = thread_id;
    }

    pub fn set_status(&mut self, status: impl Into<String>) -> bool {
        let next_status = status.into();
        if self.status == next_status {
            return false;
        }
        self.status = next_status;
        true
    }

    pub fn set_pending_turns(&mut self, pending_turns: usize) {
        self.pending_turns = pending_turns;
    }

    pub fn set_last_turn_id(&mut self, turn_id: Option<String>) {
        self.last_turn_id = turn_id;
    }

    pub fn set_last_message(&mut self, message: Option<String>) {
        if let Some(message) = message {
            self.last_message = Some(message);
        }
    }

    pub fn set_summary(&mut self, summary: Option<String>) {
        if summary.is_some() {
            self.summary = summary;
        }
    }

    pub fn push_timeline_event(&mut self, kind: SessionEventKind, text: impl Into<String>) {
        self.timeline_events.push_back(SessionEvent {
            kind,
            text: text.into(),
        });
        trim_timeline(&mut self.timeline_events);
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push_back(line.into());
        trim_lines(&mut self.lines);
    }

    pub fn append_live_chunk(&mut self, chunk: &str) {
        self.live_buffer.push_str(chunk);
    }

    pub fn set_live_buffer(&mut self, content: &str) {
        self.live_buffer.clear();
        self.live_buffer.push_str(content);
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

    pub fn replace_last_assistant_line(&mut self, text: &str) {
        let replacement = format!("assistant> {text}");
        if let Some(last) = self.lines.back_mut() {
            if last.starts_with("assistant> ") {
                *last = replacement;
            }
        }
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let mut log_lines = self.lines.iter().cloned().collect::<Vec<_>>();
        if !self.live_buffer.is_empty() {
            log_lines.push(format!("assistant> {}", self.live_buffer));
        }

        let subtitle = self
            .summary
            .clone()
            .or_else(|| self.last_message.clone())
            .unwrap_or_else(|| self.title.clone());

        SessionSnapshot {
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            title: self.title.clone(),
            subtitle,
            pending_turns: self.pending_turns,
            summary: self.summary.clone(),
            kind: self.kind.clone(),
            status: self.status.clone(),
            cwd: self.cwd.clone(),
            last_turn_id: self.last_turn_id.clone(),
            last_message: self.last_message.clone(),
            timeline_events: self.timeline_events.iter().cloned().collect(),
            log_lines,
        }
    }
}

fn trim_lines(lines: &mut VecDeque<String>) {
    while lines.len() > MAX_LOG_LINES {
        lines.pop_front();
    }
}

fn trim_timeline(events: &mut VecDeque<SessionEvent>) {
    while events.len() > MAX_TIMELINE_EVENTS {
        events.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionEventKind, SessionKind, SessionView};

    #[test]
    fn timeline_is_trimmed_to_recent_events() {
        let mut session = SessionView::master("thread-1".to_owned(), "/tmp".to_owned(), None, None);

        for index in 0..140 {
            session.push_timeline_event(SessionEventKind::System, format!("event-{index}"));
        }

        let snapshot = session.snapshot();
        assert_eq!(snapshot.timeline_events.len(), 128);
        assert_eq!(snapshot.timeline_events[0].text, "event-12");
        assert_eq!(snapshot.timeline_events[127].text, "event-139");
        assert!(matches!(snapshot.kind, SessionKind::Master));
    }
}
