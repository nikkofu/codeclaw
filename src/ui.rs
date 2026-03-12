use crate::{
    controller::{
        BatchEventSnapshot, BatchSessionSnapshot, BatchSnapshot, Controller, PromptTarget,
    },
    session::{SessionEvent, SessionEventKind, SessionKind, SessionSnapshot},
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
    selected_batch_id: Option<u64>,
    detail_mode: DetailMode,
    focus_filter: FocusFilter,
    animation_tick: u64,
    input_mode: InputMode,
    input_buffer: String,
    status_message: String,
    last_title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailMode {
    Session,
    Batch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusFilter {
    All,
    Summary,
    Commands,
    Errors,
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
            selected_batch_id: None,
            detail_mode: DetailMode::Session,
            focus_filter: FocusFilter::All,
            animation_tick: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            status_message:
                "Press `i` to talk to master, `n` to spawn a worker, `f` to focus panels, `b` to inspect batches.".to_owned(),
            last_title: String::new(),
        }
    }

    async fn run(mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            let sessions = self.controller.sessions_snapshot();
            self.sync_selection(&sessions);
            self.sync_batch_selection(&sessions);
            self.sync_title(&sessions)?;

            terminal
                .draw(|frame| self.draw(frame, &sessions))
                .context("failed to draw TUI frame")?;
            self.animation_tick = self.animation_tick.wrapping_add(1);

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
        let selected = sessions
            .iter()
            .find(|session| session.id == self.selected_id);
        let selected_accent = selected
            .map(session_accent_color)
            .unwrap_or(Color::Rgb(112, 122, 140));
        let running_count = sessions
            .iter()
            .filter(|session| is_busy_status(&session.status))
            .count();

        let items = sessions
            .iter()
            .map(|session| {
                let accent = session_accent_color(session);
                let title = truncate(&session.title, 17);
                let subtitle = truncate(&session_list_subtitle(session), 27);
                ListItem::new(Text::from(vec![
                    Line::from(vec![
                        Span::styled("|", Style::default().fg(accent)),
                        Span::raw(" "),
                        Span::styled(
                            animated_status_badge(&session.status, self.animation_tick),
                            status_badge_style(&session.status, self.animation_tick),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("[{}]", session_kind_badge(session)),
                            Style::default().fg(accent).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(title, session_title_style(session)),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            status_caption(&session.status, self.animation_tick),
                            status_secondary_style(&session.status),
                        ),
                        Span::raw("  "),
                        Span::styled(subtitle, Style::default().fg(Color::DarkGray)),
                    ]),
                ]))
            })
            .collect::<Vec<_>>();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled(
                            "Sessions",
                            Style::default()
                                .fg(selected_accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{} active", running_count),
                            Style::default().fg(status_color("running", self.animation_tick)),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(animated_border_color(
                        "running",
                        selected_accent,
                        self.animation_tick,
                    ))),
            )
            .highlight_style(
                Style::default()
                    .bg(selected_highlight_bg(selected_accent))
                    .fg(Color::White),
            )
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn draw_selected_session(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        sessions: &[SessionSnapshot],
    ) {
        match self.detail_mode {
            DetailMode::Session => self.draw_selected_session_view(frame, area, sessions),
            DetailMode::Batch => self.draw_batch_view(frame, area, sessions),
        }
    }

    fn draw_selected_session_view(
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
        let accent = session_accent_color(session);
        let border_style = panel_border_style(&session.status, accent, self.animation_tick);
        let visible_timeline = session
            .timeline_events
            .iter()
            .filter(|event| event_matches_filter(&event.kind, self.focus_filter))
            .count();
        let visible_logs = session
            .log_lines
            .iter()
            .filter(|line| log_line_matches_filter(line, self.focus_filter))
            .count();

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12),
                Constraint::Length(9),
                Constraint::Min(7),
            ])
            .split(area);

        let meta = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    animated_status_badge(&session.status, self.animation_tick),
                    status_badge_style(&session.status, self.animation_tick),
                ),
                Span::raw(" "),
                Span::styled(
                    &session.title,
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", status_caption(&session.status, self.animation_tick)),
                    status_style(&session.status, self.animation_tick),
                ),
            ]),
            Line::from(format!("id: {}", session.id)),
            Line::from(session_identity_line(session)),
            Line::from(session_queue_line(session)),
            Line::from(session_batch_line(session)),
            Line::from(session_summary_line(session)),
            Line::from(session_lifecycle_note_line(session)),
            Line::from(session_last_message_line(session)),
            Line::from(format!("thread: {}", session.thread_id)),
            Line::from(session_location_line(session)),
        ]))
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(
                        "Session",
                        Style::default().fg(accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        detail_mode_chip(self.detail_mode),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(meta, sections[0]);

        let timeline = timeline_text(
            &session.timeline_events,
            self.focus_filter,
            sections[1].width.saturating_sub(4) as usize,
            sections[1].height.saturating_sub(2) as usize,
        );
        let timeline = Paragraph::new(timeline)
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled(
                            format!(
                                "Timeline {}",
                                activity_glyph(&session.status, self.animation_tick)
                            ),
                            Style::default().fg(status_color(&session.status, self.animation_tick)),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            focus_filter_chip(self.focus_filter),
                            focus_filter_style(self.focus_filter, accent),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{visible_timeline}/{}", session.timeline_events.len()),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            timeline_status_hint(&session.status),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(secondary_panel_border_style(
                        accent,
                        &session.status,
                        self.animation_tick,
                    )),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(timeline, sections[1]);

        let body = log_text(
            &session.log_lines,
            self.focus_filter,
            sections[2].width.saturating_sub(4) as usize,
            sections[2].height.saturating_sub(2) as usize,
            accent,
        );
        let detail = Paragraph::new(body)
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled(
                            format!(
                                "Live Output {}",
                                activity_glyph(&session.status, self.animation_tick)
                            ),
                            Style::default().fg(accent),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            focus_filter_chip(self.focus_filter),
                            focus_filter_style(self.focus_filter, accent),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{visible_logs}/{}", session.log_lines.len()),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            live_output_hint(&session.status),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(secondary_panel_border_style(
                        accent,
                        &session.status,
                        self.animation_tick,
                    )),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(detail, sections[2]);
    }

    fn draw_batch_view(&self, frame: &mut Frame<'_>, area: Rect, sessions: &[SessionSnapshot]) {
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            let empty = Paragraph::new("No session selected")
                .block(Block::default().title("Batch").borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        };
        let Some(batch_id) = self.selected_batch_id else {
            let empty = Paragraph::new("No batch selected for this session.")
                .block(Block::default().title("Batch").borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        };
        let Some(batch) = self.controller.batch_snapshot(batch_id) else {
            let empty = Paragraph::new(format!("Batch b{batch_id} is no longer available."))
                .block(Block::default().title("Batch").borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        };
        let accent = session_accent_color(session);
        let border_style = panel_border_style(&batch.status, accent, self.animation_tick);
        let visible_events = batch
            .events
            .iter()
            .filter(|event| event_matches_filter(&event.kind, self.focus_filter))
            .count();

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(11),
                Constraint::Length(6),
                Constraint::Min(7),
            ])
            .split(area);

        let meta = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    format!(
                        "{} b{:03}",
                        activity_glyph(&batch.status, self.animation_tick),
                        batch.id
                    ),
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", status_caption(&batch.status, self.animation_tick)),
                    status_style(&batch.status, self.animation_tick),
                ),
            ]),
            Line::from(format!("focus session: {}", session.title)),
            Line::from(format!(
                "root: {} ({})",
                truncate(&batch.root_session_title, 28),
                batch.root_session_id
            )),
            Line::from(format!("prompt: {}", truncate(&batch.root_prompt, 44))),
            Line::from(format!("sessions: {}", batch.sessions.len())),
            Line::from(format!("events: {}", batch.events.len())),
            Line::from(format!("created: {}", batch.created_at)),
            Line::from(format!("updated: {}", batch.updated_at)),
            Line::from(format!(
                "last: {}",
                truncate(batch.last_event.as_deref().unwrap_or("-"), 44)
            )),
        ]))
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(
                        "Batch",
                        Style::default().fg(accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        detail_mode_chip(self.detail_mode),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(meta, sections[0]);

        let members = if batch.sessions.is_empty() {
            "No batch members.".to_owned()
        } else {
            batch
                .sessions
                .iter()
                .map(batch_member_line)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let members = Paragraph::new(members)
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("Batch Sessions", Style::default().fg(accent)),
                        Span::raw(" "),
                        Span::styled(
                            format!("{}", batch.sessions.len()),
                            Style::default().fg(status_color(&batch.status, self.animation_tick)),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(secondary_panel_border_style(
                        accent,
                        &batch.status,
                        self.animation_tick,
                    )),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(members, sections[1]);

        let timeline = batch_timeline_text(
            &batch,
            self.focus_filter,
            sections[2].width.saturating_sub(4) as usize,
            sections[2].height.saturating_sub(2) as usize,
        );
        let timeline = Paragraph::new(timeline)
            .block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled(
                            format!(
                                "Batch Timeline {}",
                                activity_glyph(&batch.status, self.animation_tick)
                            ),
                            Style::default().fg(accent),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            focus_filter_chip(self.focus_filter),
                            focus_filter_style(self.focus_filter, accent),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{visible_events}/{}", batch.events.len()),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(" :: "),
                        Span::styled(
                            truncate(&batch.root_prompt, 24),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(secondary_panel_border_style(
                        accent,
                        &batch.status,
                        self.animation_tick,
                    )),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(timeline, sections[2]);
    }

    fn draw_status_bar(&self, frame: &mut Frame<'_>, area: Rect, sessions: &[SessionSnapshot]) {
        let selected = sessions
            .iter()
            .find(|session| session.id == self.selected_id);
        let status = selected
            .map(|session| {
                let detail = match self.detail_mode {
                    DetailMode::Session => "session".to_owned(),
                    DetailMode::Batch => self
                        .selected_batch_id
                        .map(|batch_id| format!("batch=b{batch_id}"))
                        .unwrap_or_else(|| "batch=-".to_owned()),
                };
                format!(
                    "selected={} | status={} | queued={} | batch={} | view={} | focus={} | target={} | keys: ↑↓ switch  i master  e worker  n spawn  f focus  b batch  [ ] cycle  g master  q quit",
                    session.title,
                    status_caption(&session.status, self.animation_tick),
                    session.pending_turns,
                    session
                        .latest_batch_id
                        .map(|batch_id| format!("b{batch_id}"))
                        .unwrap_or_else(|| "-".to_owned()),
                    detail,
                    focus_filter_label(self.focus_filter),
                    input_target_label(&self.input_mode, session),
                )
            })
            .unwrap_or_else(|| "No session selected".to_owned());

        let paragraph = if let Some(session) = selected {
            Paragraph::new(status).style(status_bar_style(
                &session.status,
                session_accent_color(session),
                self.animation_tick,
            ))
        } else {
            Paragraph::new(status)
                .style(Style::default().bg(Color::Rgb(28, 32, 38)).fg(Color::White))
        };
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
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(input_border_style(
                    &self.input_mode,
                    selected,
                    self.animation_tick,
                )),
        )
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
                self.focus_session("master".to_owned(), sessions);
                self.status_message = "focused master".to_owned();
            }
            KeyCode::Char('f') => {
                self.focus_filter = self.focus_filter.next();
                self.status_message = format!(
                    "panel focus set to {}",
                    focus_filter_label(self.focus_filter)
                );
            }
            KeyCode::Char('b') => self.toggle_batch_view(sessions),
            KeyCode::Char('[') => self.select_previous_batch(sessions),
            KeyCode::Char(']') => self.select_next_batch(sessions),
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
                        self.detail_mode = DetailMode::Session;
                        self.selected_batch_id = None;
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
            self.selected_batch_id = None;
            return;
        }

        if !sessions
            .iter()
            .any(|session| session.id == self.selected_id)
        {
            self.selected_id = sessions[0].id.clone();
        }
    }

    fn sync_batch_selection(&mut self, sessions: &[SessionSnapshot]) {
        if self.detail_mode != DetailMode::Batch {
            return;
        }
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            self.selected_batch_id = None;
            return;
        };
        let batch_ids = session_batch_ids(session);
        if batch_ids.is_empty() {
            self.selected_batch_id = None;
            self.detail_mode = DetailMode::Session;
            return;
        }
        if !self
            .selected_batch_id
            .is_some_and(|batch_id| batch_ids.contains(&batch_id))
        {
            self.selected_batch_id = batch_ids.last().copied();
        }
    }

    fn sync_title(&mut self, sessions: &[SessionSnapshot]) -> Result<()> {
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            return Ok(());
        };
        let title = match self.detail_mode {
            DetailMode::Session => format!(
                "CodeClaw :: {} {} [{}]",
                activity_glyph(&session.status, self.animation_tick),
                session.title,
                status_caption(&session.status, self.animation_tick)
            ),
            DetailMode::Batch => self
                .selected_batch_id
                .map(|batch_id| {
                    format!(
                        "CodeClaw :: {} {} :: b{batch_id} [{}]",
                        activity_glyph(&session.status, self.animation_tick),
                        session.title,
                        status_caption(&session.status, self.animation_tick)
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "CodeClaw :: {} {} [{}]",
                        activity_glyph(&session.status, self.animation_tick),
                        session.title,
                        status_caption(&session.status, self.animation_tick)
                    )
                }),
        };
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
        self.focus_session(sessions[next].id.clone(), sessions);
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
        self.focus_session(sessions[next].id.clone(), sessions);
    }

    fn focus_session(&mut self, session_id: String, sessions: &[SessionSnapshot]) {
        self.selected_id = session_id.clone();
        if self.detail_mode == DetailMode::Batch {
            self.selected_batch_id = sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| session_batch_ids(session).last().copied());
        } else {
            self.selected_batch_id = None;
        }
    }

    fn toggle_batch_view(&mut self, sessions: &[SessionSnapshot]) {
        match self.detail_mode {
            DetailMode::Session => {
                let Some(session) = sessions
                    .iter()
                    .find(|session| session.id == self.selected_id)
                else {
                    self.status_message = "no session selected".to_owned();
                    return;
                };
                let batch_ids = session_batch_ids(session);
                if let Some(batch_id) = batch_ids.last().copied() {
                    self.detail_mode = DetailMode::Batch;
                    self.selected_batch_id = Some(batch_id);
                    self.status_message = format!("inspecting batch b{batch_id}");
                } else {
                    self.status_message = "selected session has no batch history".to_owned();
                }
            }
            DetailMode::Batch => {
                self.detail_mode = DetailMode::Session;
                self.status_message = "returned to session view".to_owned();
            }
        }
    }

    fn select_previous_batch(&mut self, sessions: &[SessionSnapshot]) {
        if self.detail_mode != DetailMode::Batch {
            return;
        }
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            return;
        };
        let batch_ids = session_batch_ids(session);
        if batch_ids.len() < 2 {
            return;
        }
        let current = self
            .selected_batch_id
            .and_then(|batch_id| {
                batch_ids
                    .iter()
                    .position(|candidate| *candidate == batch_id)
            })
            .unwrap_or(batch_ids.len() - 1);
        let next = current.saturating_sub(1);
        self.selected_batch_id = Some(batch_ids[next]);
        self.status_message = format!("inspecting batch b{}", batch_ids[next]);
    }

    fn select_next_batch(&mut self, sessions: &[SessionSnapshot]) {
        if self.detail_mode != DetailMode::Batch {
            return;
        }
        let Some(session) = sessions
            .iter()
            .find(|session| session.id == self.selected_id)
        else {
            return;
        };
        let batch_ids = session_batch_ids(session);
        if batch_ids.len() < 2 {
            return;
        }
        let current = self
            .selected_batch_id
            .and_then(|batch_id| {
                batch_ids
                    .iter()
                    .position(|candidate| *candidate == batch_id)
            })
            .unwrap_or(batch_ids.len() - 1);
        let next = min(current + 1, batch_ids.len() - 1);
        self.selected_batch_id = Some(batch_ids[next]);
        self.status_message = format!("inspecting batch b{}", batch_ids[next]);
    }
}

#[derive(Debug, Clone)]
enum PromptMode {
    Master,
    Worker(String),
    Spawn,
}

impl FocusFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Summary,
            Self::Summary => Self::Commands,
            Self::Commands => Self::Errors,
            Self::Errors => Self::All,
        }
    }
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

fn animated_status_badge(status: &str, tick: u64) -> String {
    match status {
        "completed" => "[OK]".to_owned(),
        "failed" => "[!!]".to_owned(),
        "spawn_requested" => "[RQ]".to_owned(),
        "bootstrapped" => "[UP]".to_owned(),
        "blocked" => "[BL]".to_owned(),
        "handed_back" => "[HB]".to_owned(),
        "queued" => format!("[{}]", queue_glyph(tick)),
        "bootstrapping" => format!("[{}]", spinner_glyph(tick)),
        "running" | "active" | "inProgress" => format!("[{}]", spinner_glyph(tick)),
        _ => "[--]".to_owned(),
    }
}

fn status_caption(status: &str, tick: u64) -> String {
    match status {
        "completed" => "completed".to_owned(),
        "failed" => "failed".to_owned(),
        "spawn_requested" => format!("spawn req {}", queue_glyph(tick)),
        "bootstrapping" => format!("bootstrapping {}", spinner_glyph(tick)),
        "bootstrapped" => "bootstrapped".to_owned(),
        "blocked" => "blocked".to_owned(),
        "handed_back" => "handed back".to_owned(),
        "queued" => format!("queued {}", queue_glyph(tick)),
        "running" | "active" | "inProgress" => format!("running {}", spinner_glyph(tick)),
        "idle" => "idle".to_owned(),
        other => other.to_owned(),
    }
}

fn status_style(status: &str, tick: u64) -> Style {
    Style::default()
        .fg(status_color(status, tick))
        .add_modifier(Modifier::BOLD)
}

fn status_badge_style(status: &str, tick: u64) -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(status_color(status, tick))
        .add_modifier(Modifier::BOLD)
}

fn status_secondary_style(status: &str) -> Style {
    Style::default().fg(match status {
        "completed" => Color::Rgb(124, 218, 146),
        "failed" => Color::Rgb(255, 133, 133),
        "spawn_requested" => Color::Rgb(201, 182, 255),
        "bootstrapping" => Color::Rgb(144, 235, 231),
        "bootstrapped" => Color::Rgb(136, 226, 197),
        "blocked" => Color::Rgb(255, 195, 120),
        "handed_back" => Color::Rgb(160, 222, 255),
        "queued" => Color::Rgb(129, 173, 255),
        "running" | "active" | "inProgress" => Color::Rgb(255, 217, 102),
        _ => Color::DarkGray,
    })
}

fn status_color(status: &str, tick: u64) -> Color {
    let pulse = (tick / 3) % 2 == 0;
    match status {
        "completed" => Color::Rgb(76, 201, 126),
        "failed" => Color::Rgb(232, 93, 93),
        "spawn_requested" => {
            if pulse {
                Color::Rgb(182, 153, 255)
            } else {
                Color::Rgb(154, 123, 235)
            }
        }
        "bootstrapping" => {
            if pulse {
                Color::Rgb(92, 219, 215)
            } else {
                Color::Rgb(67, 194, 190)
            }
        }
        "bootstrapped" => Color::Rgb(90, 210, 184),
        "blocked" => {
            if pulse {
                Color::Rgb(255, 176, 94)
            } else {
                Color::Rgb(232, 150, 63)
            }
        }
        "handed_back" => Color::Rgb(111, 203, 255),
        "queued" => {
            if pulse {
                Color::Rgb(108, 142, 255)
            } else {
                Color::Rgb(86, 119, 231)
            }
        }
        "running" | "active" | "inProgress" => {
            if pulse {
                Color::Rgb(255, 201, 79)
            } else {
                Color::Rgb(242, 176, 52)
            }
        }
        _ => Color::Rgb(132, 141, 156),
    }
}

fn spinner_glyph(tick: u64) -> &'static str {
    const FRAMES: [&str; 4] = ["-", "\\", "|", "/"];
    FRAMES[((tick / 2) as usize) % FRAMES.len()]
}

fn queue_glyph(tick: u64) -> &'static str {
    const FRAMES: [&str; 4] = [".", "o", "O", "o"];
    FRAMES[((tick / 2) as usize) % FRAMES.len()]
}

fn activity_glyph(status: &str, tick: u64) -> &'static str {
    match status {
        "completed" => "+",
        "failed" => "x",
        "spawn_requested" => "+",
        "bootstrapping" => spinner_glyph(tick),
        "bootstrapped" => "^",
        "blocked" => "!",
        "handed_back" => "<",
        "queued" => queue_glyph(tick),
        "running" | "active" | "inProgress" => spinner_glyph(tick),
        _ => "-",
    }
}

fn is_busy_status(status: &str) -> bool {
    matches!(
        status,
        "spawn_requested" | "bootstrapping" | "queued" | "running" | "active" | "inProgress"
    )
}

fn session_accent_color(session: &SessionSnapshot) -> Color {
    match &session.kind {
        SessionKind::Master => Color::Rgb(232, 190, 92),
        SessionKind::Worker { group, .. } => match group.as_str() {
            "backend" => Color::Rgb(96, 165, 250),
            "frontend" => Color::Rgb(72, 187, 158),
            "infra" => Color::Rgb(244, 162, 97),
            _ => Color::Rgb(167, 139, 250),
        },
    }
}

fn session_title_style(session: &SessionSnapshot) -> Style {
    Style::default()
        .fg(session_accent_color(session))
        .add_modifier(Modifier::BOLD)
}

fn session_kind_badge(session: &SessionSnapshot) -> &'static str {
    match &session.kind {
        SessionKind::Master => "MSTR",
        SessionKind::Worker { group, .. } => match group.as_str() {
            "backend" => "BACK",
            "frontend" => "FRNT",
            "infra" => "INFR",
            _ => "WORK",
        },
    }
}

fn animated_border_color(status: &str, accent: Color, tick: u64) -> Color {
    if matches!(
        status,
        "failed"
            | "completed"
            | "spawn_requested"
            | "bootstrapping"
            | "bootstrapped"
            | "blocked"
            | "handed_back"
            | "queued"
            | "running"
            | "active"
            | "inProgress"
    ) {
        status_color(status, tick)
    } else {
        accent
    }
}

fn panel_border_style(status: &str, accent: Color, tick: u64) -> Style {
    Style::default()
        .fg(animated_border_color(status, accent, tick))
        .add_modifier(
            if matches!(
                status,
                "bootstrapping" | "running" | "active" | "inProgress" | "blocked"
            ) {
                Modifier::BOLD
            } else {
                Modifier::empty()
            },
        )
}

fn secondary_panel_border_style(accent: Color, status: &str, tick: u64) -> Style {
    let color = if matches!(
        status,
        "spawn_requested"
            | "bootstrapping"
            | "running"
            | "active"
            | "inProgress"
            | "queued"
            | "blocked"
            | "handed_back"
            | "bootstrapped"
    ) {
        animated_border_color(status, accent, tick)
    } else {
        accent
    };
    Style::default().fg(color)
}

fn selected_highlight_bg(accent: Color) -> Color {
    match accent {
        Color::Rgb(232, 190, 92) => Color::Rgb(60, 52, 28),
        Color::Rgb(96, 165, 250) => Color::Rgb(27, 45, 74),
        Color::Rgb(72, 187, 158) => Color::Rgb(22, 57, 52),
        Color::Rgb(244, 162, 97) => Color::Rgb(70, 44, 27),
        _ => Color::Rgb(43, 36, 71),
    }
}

fn status_bar_style(status: &str, accent: Color, tick: u64) -> Style {
    let bg = match status {
        "completed" => Color::Rgb(20, 58, 35),
        "failed" => Color::Rgb(72, 25, 25),
        "spawn_requested" => {
            if (tick / 3) % 2 == 0 {
                Color::Rgb(49, 33, 83)
            } else {
                Color::Rgb(42, 28, 70)
            }
        }
        "bootstrapping" => {
            if (tick / 3) % 2 == 0 {
                Color::Rgb(18, 68, 71)
            } else {
                Color::Rgb(15, 57, 60)
            }
        }
        "bootstrapped" => Color::Rgb(16, 63, 53),
        "blocked" => {
            if (tick / 3) % 2 == 0 {
                Color::Rgb(84, 50, 10)
            } else {
                Color::Rgb(73, 43, 8)
            }
        }
        "handed_back" => Color::Rgb(18, 52, 74),
        "queued" => {
            if (tick / 3) % 2 == 0 {
                Color::Rgb(24, 40, 82)
            } else {
                Color::Rgb(20, 33, 67)
            }
        }
        "running" | "active" | "inProgress" => {
            if (tick / 3) % 2 == 0 {
                Color::Rgb(72, 51, 14)
            } else {
                Color::Rgb(86, 58, 12)
            }
        }
        _ => selected_highlight_bg(accent),
    };
    Style::default().bg(bg).fg(Color::White)
}

fn input_border_style(mode: &InputMode, selected: Option<&SessionSnapshot>, tick: u64) -> Style {
    match mode {
        InputMode::Normal => selected
            .map(|session| Style::default().fg(session_accent_color(session)))
            .unwrap_or_else(|| Style::default().fg(Color::Gray)),
        InputMode::MasterPrompt | InputMode::WorkerPrompt(_) => {
            Style::default().fg(status_color("running", tick))
        }
        InputMode::SpawnWorker => Style::default().fg(status_color("queued", tick)),
    }
}

fn detail_mode_chip(mode: DetailMode) -> &'static str {
    match mode {
        DetailMode::Session => "[session]",
        DetailMode::Batch => "[batch]",
    }
}

fn timeline_status_hint(status: &str) -> &'static str {
    match status {
        "failed" => "faults highlighted",
        "completed" => "settled",
        "spawn_requested" => "launching worker",
        "bootstrapping" => "initializing worker",
        "bootstrapped" => "initial handoff ready",
        "blocked" => "attention needed",
        "handed_back" => "returned to master",
        "queued" => "dispatch queued",
        "running" | "active" | "inProgress" => "live orchestration",
        _ => "recent events",
    }
}

fn live_output_hint(status: &str) -> &'static str {
    match status {
        "failed" => "check latest errors",
        "completed" => "final transcript",
        "spawn_requested" => "preparing worker thread",
        "bootstrapping" => "bootstrap stream",
        "bootstrapped" => "bootstrap complete",
        "blocked" => "review blocker details",
        "handed_back" => "ready for next dispatch",
        "queued" => "awaiting worker turn",
        "running" | "active" | "inProgress" => "streaming",
        _ => "latest stream",
    }
}

fn timeline_text(
    events: &[SessionEvent],
    filter: FocusFilter,
    width: usize,
    max_lines: usize,
) -> Text<'static> {
    if max_lines == 0 {
        return Text::default();
    }

    let filtered = filtered_session_events(events, filter, max_lines);
    if filtered.is_empty() {
        return Text::from(Line::from(empty_timeline_message(filter)));
    }

    let max_body = width.saturating_sub(7).max(12);
    let lines = filtered
        .into_iter()
        .map(|event| {
            Line::from(vec![
                Span::styled(
                    event
                        .batch_id
                        .map(|batch_id| format!("b{batch_id:03} "))
                        .unwrap_or_else(|| "     ".to_owned()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{}]", event_kind_label(&event.kind)),
                    event_kind_style(&event.kind),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate(&compact_inline(&event.text), max_body),
                    event_text_style(&event.kind),
                ),
            ])
        })
        .collect::<Vec<_>>();

    Text::from(lines)
}

fn batch_timeline_text(
    batch: &BatchSnapshot,
    filter: FocusFilter,
    width: usize,
    max_lines: usize,
) -> Text<'static> {
    if max_lines == 0 {
        return Text::default();
    }

    let filtered = filtered_batch_events(&batch.events, filter, max_lines);
    if filtered.is_empty() {
        return Text::from(Line::from(empty_timeline_message(filter)));
    }

    let max_session = width.saturating_sub(18).clamp(10, 18);
    let max_body = width.saturating_sub(max_session + 8).max(12);
    let lines = filtered
        .into_iter()
        .map(|event| {
            Line::from(vec![
                Span::styled(
                    truncate(&event.session_title, max_session),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("[{}]", event_kind_label(&event.kind)),
                    event_kind_style(&event.kind),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate(&compact_inline(&event.text), max_body),
                    event_text_style(&event.kind),
                ),
            ])
        })
        .collect::<Vec<_>>();

    Text::from(lines)
}

fn log_text(
    lines: &[String],
    filter: FocusFilter,
    width: usize,
    max_lines: usize,
    accent: Color,
) -> Text<'static> {
    if max_lines == 0 {
        return Text::default();
    }

    let filtered = filtered_log_lines(lines, filter, max_lines);
    if filtered.is_empty() {
        return Text::from(Line::from(empty_output_message(filter)));
    }

    Text::from(
        filtered
            .into_iter()
            .map(|line| styled_log_line(line, width, accent))
            .collect::<Vec<_>>(),
    )
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

fn session_batch_line(session: &SessionSnapshot) -> String {
    format!(
        "batch: {}",
        session
            .latest_batch_id
            .map(|batch_id| format!("b{batch_id}"))
            .unwrap_or_else(|| "-".to_owned())
    )
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

fn session_lifecycle_note_line(session: &SessionSnapshot) -> String {
    format!(
        "note: {}",
        truncate(
            &session
                .lifecycle_note
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

fn batch_member_line(session: &BatchSessionSnapshot) -> String {
    format!(
        "{} [{}]",
        truncate(&session.title, 28),
        truncate(&session.status, 10)
    )
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLineKind {
    Prompt,
    Assistant,
    Command,
    Output,
    Error,
    Other,
}

fn focus_filter_label(filter: FocusFilter) -> &'static str {
    match filter {
        FocusFilter::All => "all",
        FocusFilter::Summary => "summary",
        FocusFilter::Commands => "commands",
        FocusFilter::Errors => "errors",
    }
}

fn focus_filter_chip(filter: FocusFilter) -> &'static str {
    match filter {
        FocusFilter::All => "[all]",
        FocusFilter::Summary => "[sum]",
        FocusFilter::Commands => "[cmd]",
        FocusFilter::Errors => "[err]",
    }
}

fn focus_filter_style(filter: FocusFilter, accent: Color) -> Style {
    let color = match filter {
        FocusFilter::All => accent,
        FocusFilter::Summary => Color::Rgb(114, 194, 255),
        FocusFilter::Commands => Color::Rgb(124, 218, 146),
        FocusFilter::Errors => Color::Rgb(255, 133, 133),
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn empty_timeline_message(filter: FocusFilter) -> &'static str {
    match filter {
        FocusFilter::All => "No orchestration events yet.",
        FocusFilter::Summary => "No summary events matched this view.",
        FocusFilter::Commands => "No command events matched this view.",
        FocusFilter::Errors => "No error events matched this view.",
    }
}

fn empty_output_message(filter: FocusFilter) -> &'static str {
    match filter {
        FocusFilter::All => "No output yet.",
        FocusFilter::Summary => "No summary output matched this view.",
        FocusFilter::Commands => "No command/output lines matched this view.",
        FocusFilter::Errors => "No error output matched this view.",
    }
}

fn event_matches_filter(kind: &SessionEventKind, filter: FocusFilter) -> bool {
    match filter {
        FocusFilter::All => true,
        FocusFilter::Summary => !matches!(kind, SessionEventKind::Command),
        FocusFilter::Commands => matches!(kind, SessionEventKind::Command),
        FocusFilter::Errors => matches!(kind, SessionEventKind::Error),
    }
}

fn filtered_session_events<'a>(
    events: &'a [SessionEvent],
    filter: FocusFilter,
    max_events: usize,
) -> Vec<&'a SessionEvent> {
    if max_events == 0 {
        return Vec::new();
    }

    let filtered = events
        .iter()
        .filter(|event| event_matches_filter(&event.kind, filter))
        .collect::<Vec<_>>();
    let start = filtered.len().saturating_sub(max_events);
    filtered[start..].to_vec()
}

fn filtered_batch_events<'a>(
    events: &'a [BatchEventSnapshot],
    filter: FocusFilter,
    max_events: usize,
) -> Vec<&'a BatchEventSnapshot> {
    if max_events == 0 {
        return Vec::new();
    }

    let filtered = events
        .iter()
        .filter(|event| event_matches_filter(&event.kind, filter))
        .collect::<Vec<_>>();
    let start = filtered.len().saturating_sub(max_events);
    filtered[start..].to_vec()
}

fn classify_log_line(line: &str) -> LogLineKind {
    if line.starts_with("user> ")
        || line.starts_with("system> ")
        || line.starts_with("orchestrator> ")
        || line.starts_with("runtime> ")
    {
        LogLineKind::Prompt
    } else if line.starts_with("assistant> ") {
        LogLineKind::Assistant
    } else if line.starts_with("command> ") {
        LogLineKind::Command
    } else if line.starts_with("output> ") {
        LogLineKind::Output
    } else if line.starts_with("error> ") {
        LogLineKind::Error
    } else {
        LogLineKind::Other
    }
}

fn log_line_matches_filter(line: &str, filter: FocusFilter) -> bool {
    let kind = classify_log_line(line);
    match filter {
        FocusFilter::All => true,
        FocusFilter::Summary => matches!(
            kind,
            LogLineKind::Prompt | LogLineKind::Assistant | LogLineKind::Error | LogLineKind::Other
        ),
        FocusFilter::Commands => matches!(kind, LogLineKind::Command | LogLineKind::Output),
        FocusFilter::Errors => matches!(kind, LogLineKind::Error),
    }
}

fn filtered_log_lines<'a>(
    lines: &'a [String],
    filter: FocusFilter,
    max_lines: usize,
) -> Vec<&'a str> {
    if max_lines == 0 {
        return Vec::new();
    }

    let filtered = lines
        .iter()
        .map(String::as_str)
        .filter(|line| log_line_matches_filter(line, filter))
        .collect::<Vec<_>>();
    let start = filtered.len().saturating_sub(max_lines);
    filtered[start..].to_vec()
}

fn styled_log_line(line: &str, width: usize, accent: Color) -> Line<'static> {
    let compact = compact_inline(line);
    let max_body = width.saturating_sub(13).max(10);
    let kind = classify_log_line(&compact);

    if let Some((prefix, body)) = compact.split_once("> ") {
        let prefix = format!("{prefix}>");
        return Line::from(vec![
            Span::styled(prefix, log_prefix_style(kind, accent)),
            Span::raw(" "),
            Span::styled(truncate(body, max_body), log_body_style(kind, accent)),
        ]);
    }

    Line::from(Span::styled(
        truncate(&compact, width.max(12)),
        log_body_style(kind, accent),
    ))
}

fn log_prefix_style(kind: LogLineKind, accent: Color) -> Style {
    let color = match kind {
        LogLineKind::Prompt => Color::Rgb(114, 194, 255),
        LogLineKind::Assistant => accent,
        LogLineKind::Command => Color::Rgb(124, 218, 146),
        LogLineKind::Output => Color::Rgb(168, 176, 190),
        LogLineKind::Error => Color::Rgb(255, 133, 133),
        LogLineKind::Other => Color::Gray,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn log_body_style(kind: LogLineKind, accent: Color) -> Style {
    let color = match kind {
        LogLineKind::Prompt => Color::Rgb(198, 224, 255),
        LogLineKind::Assistant => brighten_color(accent),
        LogLineKind::Command => Color::Rgb(198, 244, 205),
        LogLineKind::Output => Color::Rgb(202, 208, 218),
        LogLineKind::Error => Color::Rgb(255, 204, 204),
        LogLineKind::Other => Color::White,
    };
    Style::default().fg(color)
}

fn event_text_style(kind: &SessionEventKind) -> Style {
    let color = match kind {
        SessionEventKind::User => Color::Rgb(183, 234, 255),
        SessionEventKind::Bootstrap => Color::Rgb(188, 210, 255),
        SessionEventKind::Orchestrator => Color::Rgb(255, 228, 160),
        SessionEventKind::Runtime => Color::Rgb(227, 197, 255),
        SessionEventKind::System => Color::Rgb(198, 203, 214),
        SessionEventKind::Command => Color::Rgb(196, 243, 205),
        SessionEventKind::Status => Color::Rgb(184, 232, 188),
        SessionEventKind::Error => Color::Rgb(255, 204, 204),
    };
    Style::default().fg(color)
}

fn compact_inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn brighten_color(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            r.saturating_add(28),
            g.saturating_add(28),
            b.saturating_add(28),
        ),
        _ => color,
    }
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

fn event_kind_style(kind: &SessionEventKind) -> Style {
    match kind {
        SessionEventKind::User => Style::default().fg(Color::Cyan),
        SessionEventKind::Bootstrap => Style::default().fg(Color::LightBlue),
        SessionEventKind::Orchestrator => Style::default().fg(Color::Yellow),
        SessionEventKind::Runtime => Style::default().fg(Color::LightMagenta),
        SessionEventKind::System => Style::default().fg(Color::Gray),
        SessionEventKind::Command => Style::default().fg(Color::LightGreen),
        SessionEventKind::Status => Style::default().fg(Color::Green),
        SessionEventKind::Error => Style::default().fg(Color::Red),
    }
}
