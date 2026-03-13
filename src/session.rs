use crate::state::{now_unix_ts, WorkerRecord};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MAX_LOG_LINES: usize = 512;
pub const MAX_TIMELINE_EVENTS: usize = 128;
const DEFAULT_MASTER_SUMMARY: &str = "Primary planner and dispatcher";
const DEFAULT_ONBOARD_SUMMARY: &str = "Supervises jobs, loop budgets, and session health";

#[derive(Debug, Clone)]
pub enum SessionKind {
    Onboard,
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
    pub job_id: Option<String>,
    pub thread_id: String,
    pub title: String,
    pub subtitle: String,
    pub pending_turns: usize,
    pub latest_batch_id: Option<u64>,
    pub summary: Option<String>,
    pub lifecycle_note: Option<String>,
    pub kind: SessionKind,
    pub status: String,
    pub cwd: String,
    pub last_turn_id: Option<String>,
    pub last_message: Option<String>,
    pub timeline_events: Vec<SessionEvent>,
    pub log_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub ts: u64,
    #[serde(default)]
    pub batch_id: Option<u64>,
    pub kind: SessionEventKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SessionView {
    id: String,
    job_id: Option<String>,
    thread_id: String,
    title: String,
    summary: Option<String>,
    lifecycle_note: Option<String>,
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
    pub fn onboard(cwd: String, summary: Option<String>, last_message: Option<String>) -> Self {
        Self {
            id: "onboard".to_owned(),
            job_id: None,
            thread_id: "virtual".to_owned(),
            title: "onboard".to_owned(),
            summary: summary.or_else(|| Some(DEFAULT_ONBOARD_SUMMARY.to_owned())),
            lifecycle_note: None,
            kind: SessionKind::Onboard,
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

    pub fn master(
        thread_id: String,
        cwd: String,
        summary: Option<String>,
        last_message: Option<String>,
    ) -> Self {
        Self {
            id: "master".to_owned(),
            job_id: None,
            thread_id,
            title: "master".to_owned(),
            summary: summary.or_else(|| Some(DEFAULT_MASTER_SUMMARY.to_owned())),
            lifecycle_note: None,
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
            job_id: worker.job_id.clone(),
            thread_id: worker.thread_id.clone(),
            title: format!("[{}] {}", worker.group, worker.task),
            summary: worker.summary.clone(),
            lifecycle_note: worker.lifecycle_note.clone(),
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

    pub fn set_lifecycle_note(&mut self, note: Option<String>) {
        self.lifecycle_note = note;
    }

    pub fn restore_timeline(&mut self, events: &[SessionEvent]) {
        self.timeline_events.clear();
        for event in events.iter().cloned() {
            self.push_timeline_event(event);
        }
    }

    pub fn restore_output(&mut self, lines: &[String]) {
        self.lines.clear();
        for line in lines {
            self.push_line(line.clone());
        }
    }

    pub fn restore_live_buffer(&mut self, content: &str) {
        self.live_buffer.clear();
        self.live_buffer.push_str(content);
    }

    pub fn output_is_empty(&self) -> bool {
        self.lines.is_empty() && self.live_buffer.is_empty()
    }

    pub fn push_timeline_event(&mut self, event: SessionEvent) {
        self.timeline_events.push_back(event);
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

    pub fn latest_batch_id(&self) -> Option<u64> {
        self.timeline_events
            .iter()
            .rev()
            .find_map(|event| event.batch_id)
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let mut log_lines = self.lines.iter().cloned().collect::<Vec<_>>();
        if !self.live_buffer.is_empty() {
            log_lines.push(format!("assistant> {}", self.live_buffer));
        }

        let subtitle = match self.status.as_str() {
            "bootstrapped" | "blocked" | "handed_back" | "failed" => self
                .lifecycle_note
                .clone()
                .or_else(|| self.summary.clone())
                .or_else(|| self.last_message.clone())
                .unwrap_or_else(|| self.title.clone()),
            _ => self
                .summary
                .clone()
                .or_else(|| self.lifecycle_note.clone())
                .or_else(|| self.last_message.clone())
                .unwrap_or_else(|| self.title.clone()),
        };

        SessionSnapshot {
            id: self.id.clone(),
            job_id: self.job_id.clone(),
            thread_id: self.thread_id.clone(),
            title: self.title.clone(),
            subtitle,
            pending_turns: self.pending_turns,
            latest_batch_id: self.latest_batch_id(),
            summary: self.summary.clone(),
            lifecycle_note: self.lifecycle_note.clone(),
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

impl SessionEvent {
    pub fn new(kind: SessionEventKind, text: impl Into<String>, batch_id: Option<u64>) -> Self {
        Self {
            ts: now_unix_ts(),
            batch_id,
            kind,
            text: text.into(),
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
    use super::{SessionEvent, SessionEventKind, SessionKind, SessionView};

    #[test]
    fn timeline_is_trimmed_to_recent_events() {
        let mut session = SessionView::master("thread-1".to_owned(), "/tmp".to_owned(), None, None);

        for index in 0..140 {
            session.push_timeline_event(SessionEvent {
                ts: index,
                batch_id: None,
                kind: SessionEventKind::System,
                text: format!("event-{index}"),
            });
        }

        let snapshot = session.snapshot();
        assert_eq!(snapshot.timeline_events.len(), 128);
        assert_eq!(snapshot.timeline_events[0].text, "event-12");
        assert_eq!(snapshot.timeline_events[127].text, "event-139");
        assert!(matches!(snapshot.kind, SessionKind::Master));
    }

    #[test]
    fn onboard_sessions_use_virtual_runtime_identity() {
        let session = SessionView::onboard("/tmp".to_owned(), None, None).snapshot();
        assert_eq!(session.id, "onboard");
        assert_eq!(session.thread_id, "virtual");
        assert!(matches!(session.kind, SessionKind::Onboard));
    }

    #[test]
    fn latest_batch_id_tracks_most_recent_batch_event() {
        let mut session = SessionView::master("thread-1".to_owned(), "/tmp".to_owned(), None, None);
        session.push_timeline_event(SessionEvent::new(SessionEventKind::System, "seed", None));
        session.push_timeline_event(SessionEvent::new(SessionEventKind::User, "batch", Some(42)));

        assert_eq!(session.latest_batch_id(), Some(42));
    }

    #[test]
    fn snapshot_includes_restored_live_buffer() {
        let mut session = SessionView::master("thread-1".to_owned(), "/tmp".to_owned(), None, None);
        session.push_line("assistant> committed");
        session.restore_live_buffer("streaming");

        let snapshot = session.snapshot();

        assert_eq!(snapshot.log_lines.len(), 2);
        assert_eq!(snapshot.log_lines[0], "assistant> committed");
        assert_eq!(snapshot.log_lines[1], "assistant> streaming");
    }

    #[test]
    fn blocked_sessions_prefer_lifecycle_note_in_subtitle() {
        let mut session = SessionView::master("thread-1".to_owned(), "/tmp".to_owned(), None, None);
        session.set_summary(Some("steady summary".to_owned()));
        session.set_lifecycle_note(Some("waiting on schema approval".to_owned()));
        session.set_status("blocked");

        let snapshot = session.snapshot();

        assert_eq!(
            snapshot.lifecycle_note.as_deref(),
            Some("waiting on schema approval")
        );
        assert_eq!(snapshot.subtitle, "waiting on schema approval");
    }
}
