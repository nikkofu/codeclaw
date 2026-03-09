use crate::{
    controller::{Controller, PromptTarget},
    session::{SessionKind, SessionSnapshot},
};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
    },
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{cmp::min, io, time::Duration};

pub async fn run(controller: Controller) -> Result<()> {
    controller.init_workspace()?;
    controller.ensure_master_thread().await?;

    enable_raw_mode().context("failed to enable raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen, SetTitle("CodeClaw"))?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal backend")?;

    let result = App::new(controller).run(&mut terminal).await;

    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen, SetTitle("CodeClaw")).ok();
    terminal.show_cursor().ok();

    result
}

struct App {
    controller: Controller,
    selected_id: String,
    input_mode: InputMode,
    input_buffer: String,
    status_message: String,
    last_title: String,
}

#[derive(Debug, Clone)]
enum InputMode {
    Normal,
    MasterPrompt,
    WorkerPrompt(String),
    SpawnWorker,
}

impl App {
    fn new(controller: Controller) -> Self {
        Self {
            controller,
            selected_id: "master".to_owned(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            status_message: "Press `i` to talk to master, `n` to spawn a worker.".to_owned(),
            last_title: String::new(),
        }
    }

    async fn run(mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            let sessions = self.controller.sessions_snapshot();
            self.sync_selection(&sessions);
            self.sync_title(&sessions)?;

            terminal
                .draw(|frame| self.draw(frame, &sessions))
                .context("failed to draw TUI frame")?;

            if event::poll(Duration::from_millis(120)).context("failed to poll terminal event")? {
                let event = event::read().context("failed to read terminal event")?;
                if let Event::Key(key) = event {
                    if key.kind == KeyEventKind::Press && self.handle_key(key, &sessions).await? {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn draw(&self, frame: &mut Frame<'_>, sessions: &[SessionSnapshot]) {
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),
                Constraint::Length(2),
                Constraint::Length(3),
            ])
            .split(frame.size());

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(34), Constraint::Min(40)])
            .split(areas[0]);

        let list_state = self.list_state(sessions);
        self.draw_sessions(frame, main[0], sessions, list_state);
        self.draw_selected_session(frame, main[1], sessions);
        self.draw_status_bar(frame, areas[1], sessions);
        self.draw_input_bar(frame, areas[2], sessions);
    }

    fn draw_sessions(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        sessions: &[SessionSnapshot],
        mut list_state: ListState,
    ) {
        let items = sessions
            .iter()
            .map(|session| {
                let status = status_badge(&session.status);
                let subtitle = truncate(&session_list_subtitle(session), 28);
                ListItem::new(Text::from(vec![
                    Line::from(vec![
                        Span::styled(status, status_style(&session.status)),
                        Span::raw(" "),
                        Span::styled(
                            &session.title,
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(Span::styled(subtitle, Style::default().fg(Color::DarkGray))),
                ]))
            })
            .collect::<Vec<_>>();

        let list = List::new(items)
            .block(
                Block::default()
                    .title("Sessions")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .highlight_style(Style::default().bg(Color::Rgb(35, 44, 53)).fg(Color::White))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn draw_selected_session(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        sessions: &[SessionSnapshot],
    ) {
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            let empty = Paragraph::new("No session selected")
                .block(Block::default().title("Session").borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(9), Constraint::Min(8)])
            .split(area);

        let meta = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    &session.title,
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", session.status),
                    status_style(&session.status),
                ),
            ]),
            Line::from(format!("id: {}", session.id)),
            Line::from(session_identity_line(session)),
            Line::from(session_queue_line(session)),
            Line::from(session_summary_line(session)),
            Line::from(session_last_message_line(session)),
            Line::from(format!("thread: {}", session.thread_id)),
            Line::from(session_location_line(session)),
        ]))
        .block(
            Block::default()
                .title("Selected Session")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(meta, sections[0]);

        let lines = tail_lines(
            &session.log_lines,
            sections[1].height.saturating_sub(2) as usize,
        );
        let body = if lines.is_empty() {
            "No output yet.".to_owned()
        } else {
            lines.join("\n")
        };

        let detail = Paragraph::new(body)
            .block(Block::default().title("Live Output").borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        frame.render_widget(detail, sections[1]);
    }

    fn draw_status_bar(&self, frame: &mut Frame<'_>, area: Rect, sessions: &[SessionSnapshot]) {
        let selected = sessions
            .iter()
            .find(|session| session.id == self.selected_id);
        let status = selected
            .map(|session| {
                format!(
                    "selected={} | status={} | queued={} | target={} | keys: ↑↓ switch  i master  e worker  n spawn  g master  q quit",
                    session.title,
                    session.status,
                    session.pending_turns,
                    input_target_label(&self.input_mode, session),
                )
            })
            .unwrap_or_else(|| "No session selected".to_owned());

        let paragraph = Paragraph::new(status)
            .style(Style::default().bg(Color::Rgb(28, 32, 38)).fg(Color::White));
        frame.render_widget(paragraph, area);
    }

    fn draw_input_bar(&self, frame: &mut Frame<'_>, area: Rect, sessions: &[SessionSnapshot]) {
        let selected = sessions
            .iter()
            .find(|session| session.id == self.selected_id);
        let title = match &self.input_mode {
            InputMode::Normal => "Command",
            InputMode::MasterPrompt => "Prompt -> master",
            InputMode::WorkerPrompt(_) => "Prompt -> worker",
            InputMode::SpawnWorker => "Spawn Worker (group: task)",
        };

        let body = match &self.input_mode {
            InputMode::Normal => self.status_message.clone(),
            _ => self.input_buffer.clone(),
        };

        let help = match &self.input_mode {
            InputMode::Normal => {
                let groups = self
                    .controller
                    .groups()
                    .into_iter()
                    .map(|group| group.id)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("available groups: {groups}")
            }
            InputMode::MasterPrompt => "Enter to send. Esc to cancel.".to_owned(),
            InputMode::WorkerPrompt(worker_id) => {
                format!("worker target: {worker_id}. Enter to send. Esc to cancel.")
            }
            InputMode::SpawnWorker => "Format: backend: Payment API refactor".to_owned(),
        };

        let paragraph = Paragraph::new(Text::from(vec![
            Line::from(body),
            Line::from(Span::styled(help, Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                selected
                    .map(|session| {
                        format!(
                            "window title: {} | last turn: {}",
                            session.title,
                            session
                                .last_turn_id
                                .clone()
                                .unwrap_or_else(|| "-".to_owned())
                        )
                    })
                    .unwrap_or_default(),
                Style::default().fg(Color::DarkGray),
            )),
        ]))
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }

    async fn handle_key(&mut self, key: KeyEvent, sessions: &[SessionSnapshot]) -> Result<bool> {
        match self.input_mode.clone() {
            InputMode::Normal => self.handle_normal_key(key, sessions).await,
            InputMode::MasterPrompt => self.handle_input_key(key, PromptMode::Master).await,
            InputMode::WorkerPrompt(worker_id) => {
                self.handle_input_key(key, PromptMode::Worker(worker_id))
                    .await
            }
            InputMode::SpawnWorker => self.handle_input_key(key, PromptMode::Spawn).await,
        }
    }

    async fn handle_normal_key(
        &mut self,
        key: KeyEvent,
        sessions: &[SessionSnapshot],
    ) -> Result<bool> {
        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(sessions),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(sessions),
            KeyCode::Char('g') => {
                self.selected_id = "master".to_owned();
                self.status_message = "focused master".to_owned();
            }
            KeyCode::Char('i') => {
                self.input_mode = InputMode::MasterPrompt;
                self.input_buffer.clear();
                self.status_message = "composing prompt to master".to_owned();
            }
            KeyCode::Char('e') => {
                if self.selected_id == "master" {
                    self.status_message =
                        "selected session is master; use `i` to send input".to_owned();
                } else {
                    self.input_mode = InputMode::WorkerPrompt(self.selected_id.clone());
                    self.input_buffer.clear();
                    self.status_message = format!("composing prompt to {}", self.selected_id);
                }
            }
            KeyCode::Char('n') => {
                self.input_mode = InputMode::SpawnWorker;
                self.input_buffer.clear();
                self.status_message = "spawn worker using `group: task`".to_owned();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            _ => {}
        }

        Ok(false)
    }

    async fn handle_input_key(&mut self, key: KeyEvent, mode: PromptMode) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.status_message = "cancelled".to_owned();
            }
            KeyCode::Enter => {
                let buffer = self.input_buffer.trim().to_owned();
                if buffer.is_empty() {
                    self.status_message = "input is empty".to_owned();
                    self.input_mode = InputMode::Normal;
                    self.input_buffer.clear();
                    return Ok(false);
                }

                match mode {
                    PromptMode::Master => {
                        self.controller
                            .submit_prompt(PromptTarget::Master, &buffer)
                            .await?;
                        self.status_message = "submitted prompt to master".to_owned();
                    }
                    PromptMode::Worker(worker_id) => {
                        self.controller
                            .submit_prompt(PromptTarget::Worker(worker_id.clone()), &buffer)
                            .await?;
                        self.status_message = format!("submitted prompt to {worker_id}");
                    }
                    PromptMode::Spawn => {
                        let (group, task) = parse_spawn_input(&buffer)?;
                        let worker = self.controller.spawn_worker(&group, &task).await?;
                        self.selected_id = worker.id.clone();
                        self.status_message = format!("spawned {}", worker.id);
                    }
                }

                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(ch) => {
                self.input_buffer.push(ch);
            }
            _ => {}
        }

        Ok(false)
    }

    fn list_state(&self, sessions: &[SessionSnapshot]) -> ListState {
        let mut state = ListState::default();
        let selected = sessions
            .iter()
            .position(|session| session.id == self.selected_id)
            .or(Some(0));
        state.select(selected);
        state
    }

    fn sync_selection(&mut self, sessions: &[SessionSnapshot]) {
        if sessions.is_empty() {
            self.selected_id.clear();
            return;
        }

        if !sessions
            .iter()
            .any(|session| session.id == self.selected_id)
        {
            self.selected_id = sessions[0].id.clone();
        }
    }

    fn sync_title(&mut self, sessions: &[SessionSnapshot]) -> Result<()> {
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            return Ok(());
        };
        let title = format!("CodeClaw :: {} [{}]", session.title, session.status);
        if self.last_title != title {
            execute!(io::stdout(), SetTitle(title.clone())).context("failed to set title")?;
            self.last_title = title;
        }
        Ok(())
    }

    fn select_previous(&mut self, sessions: &[SessionSnapshot]) {
        if sessions.is_empty() {
            return;
        }

        let current = sessions
            .iter()
            .position(|session| session.id == self.selected_id)
            .unwrap_or(0);
        let next = current.saturating_sub(1);
        self.selected_id = sessions[next].id.clone();
    }

    fn select_next(&mut self, sessions: &[SessionSnapshot]) {
        if sessions.is_empty() {
            return;
        }

        let current = sessions
            .iter()
            .position(|session| session.id == self.selected_id)
            .unwrap_or(0);
        let next = min(current + 1, sessions.len() - 1);
        self.selected_id = sessions[next].id.clone();
    }
}

#[derive(Debug, Clone)]
enum PromptMode {
    Master,
    Worker(String),
    Spawn,
}

fn parse_spawn_input(input: &str) -> Result<(String, String)> {
    let Some((group, task)) = input.split_once(':') else {
        anyhow::bail!("spawn input must use `group: task`");
    };
    let group = group.trim();
    let task = task.trim();
    if group.is_empty() || task.is_empty() {
        anyhow::bail!("spawn input must include both group and task");
    }
    Ok((group.to_owned(), task.to_owned()))
}

fn truncate(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_owned()
    } else {
        let prefix = trimmed
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>();
        format!("{prefix}…")
    }
}

fn status_badge(status: &str) -> &'static str {
    match status {
        "completed" => "OK",
        "failed" => "ER",
        "running" | "queued" | "active" | "inProgress" => "RN",
        _ => "ID",
    }
}

fn status_style(status: &str) -> Style {
    match status {
        "completed" => Style::default().fg(Color::Green),
        "failed" => Style::default().fg(Color::Red),
        "running" | "queued" | "active" | "inProgress" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Gray),
    }
}

fn tail_lines(lines: &[String], max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].to_vec()
}

fn input_target_label(mode: &InputMode, session: &SessionSnapshot) -> String {
    match mode {
        InputMode::MasterPrompt => "master".to_owned(),
        InputMode::WorkerPrompt(worker_id) => worker_id.clone(),
        InputMode::SpawnWorker => "spawn".to_owned(),
        InputMode::Normal => match &session.kind {
            SessionKind::Master => "master".to_owned(),
            SessionKind::Worker { .. } => "master".to_owned(),
        },
    }
}

fn session_list_subtitle(session: &SessionSnapshot) -> String {
    if session.pending_turns == 0 {
        session.subtitle.clone()
    } else {
        format!("q{} | {}", session.pending_turns, session.subtitle)
    }
}

fn session_identity_line(session: &SessionSnapshot) -> String {
    match &session.kind {
        SessionKind::Master => "role: master".to_owned(),
        SessionKind::Worker { group, task, .. } => {
            format!("group: {group} | task: {}", truncate(task, 28))
        }
    }
}

fn session_queue_line(session: &SessionSnapshot) -> String {
    format!("queue: {} pending turn(s)", session.pending_turns)
}

fn session_summary_line(session: &SessionSnapshot) -> String {
    format!(
        "summary: {}",
        truncate(
            &session
                .summary
                .clone()
                .unwrap_or_else(|| "not set".to_owned()),
            44,
        )
    )
}

fn session_last_message_line(session: &SessionSnapshot) -> String {
    format!(
        "last: {}",
        truncate(
            &session
                .last_message
                .clone()
                .unwrap_or_else(|| "-".to_owned()),
            47,
        )
    )
}

fn session_location_line(session: &SessionSnapshot) -> String {
    match &session.kind {
        SessionKind::Master => format!("workspace: {}", truncate(&session.cwd, 39)),
        SessionKind::Worker { task_file, .. } => {
            format!("task file: {}", truncate(task_file, 39))
        }
    }
}
