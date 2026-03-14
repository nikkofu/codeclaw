use crate::{
    controller::{
        job_intake_prompt, BatchEventSnapshot, BatchSessionSnapshot, BatchSnapshot, Controller,
        CreateJobRequest, CreateSessionAutomationRequest, MonitorSessionSnapshot, OnboardLaneItem,
        OnboardSnapshot, PromptTarget, SessionAutomationSnapshot,
    },
    service::ServiceLifecycle,
    session::{
        SessionEvent, SessionEventKind, SessionKind, SessionOverviewSnapshot, SessionSnapshot,
    },
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
use shell_words::split as shell_split;
use std::{
    cmp::min,
    io,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

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
    available_groups: Vec<String>,
    service_started_at: u64,
    service_tick: u64,
    last_service_tick_at: Instant,
    selected_id: String,
    selected_batch_id: Option<u64>,
    detail_mode: DetailMode,
    focus_filter: FocusFilter,
    animation_tick: u64,
    input_mode: InputMode,
    input_buffer: String,
    input_cursor: usize,
    input_wrap_width: usize,
    input_history: InputHistory,
    history_cursor: Option<InputHistoryCursor>,
    completion_index: usize,
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
    SlashCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputHistoryScope {
    Master,
    Worker,
    Spawn,
    Slash,
}

#[derive(Debug, Default, Clone)]
struct InputHistory {
    master: Vec<String>,
    worker: Vec<String>,
    spawn: Vec<String>,
    slash: Vec<String>,
}

#[derive(Debug, Clone)]
struct InputHistoryCursor {
    scope: InputHistoryScope,
    index: usize,
    draft: String,
}

#[derive(Debug, Clone)]
struct InputCompletion {
    label: String,
    replacement: String,
    start: usize,
    end: usize,
}

impl App {
    fn new(controller: Controller) -> Self {
        let available_groups = controller
            .groups()
            .into_iter()
            .map(|group| group.id)
            .collect::<Vec<_>>();
        Self {
            controller,
            available_groups,
            service_started_at: crate::state::now_unix_ts(),
            service_tick: 0,
            last_service_tick_at: Instant::now() - Duration::from_millis(UI_SERVICE_TICK_INTERVAL_MS),
            selected_id: "onboard".to_owned(),
            selected_batch_id: None,
            detail_mode: DetailMode::Session,
            focus_filter: FocusFilter::All,
            animation_tick: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            input_wrap_width: 80,
            input_history: InputHistory::default(),
            history_cursor: None,
            completion_index: 0,
            status_message:
                "Press `/` for commands, `o` for onboard, `i` to talk to master, `n` to spawn a worker, `f` to focus panels, `b` to inspect batches.".to_owned(),
            last_title: String::new(),
        }
    }

    async fn run(mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let _ = self.controller.write_service_lifecycle(
            ServiceLifecycle::Starting,
            self.service_started_at,
            self.service_tick,
            Vec::new(),
            None,
        );
        loop {
            self.tick_service_if_due().await;
            let sessions = self.controller.sessions_overview_snapshot();
            self.sync_selection(&sessions);
            self.sync_batch_selection(&sessions);
            self.sync_title(&sessions)?;
            self.input_wrap_width = terminal
                .size()
                .map(|size| size.width.saturating_sub(2) as usize)
                .unwrap_or(80)
                .max(1);

            terminal
                .draw(|frame| self.draw(frame, &sessions))
                .context("failed to draw TUI frame")?;
            self.animation_tick = self.animation_tick.wrapping_add(1);

            if event::poll(Duration::from_millis(120)).context("failed to poll terminal event")? {
                let event = event::read().context("failed to read terminal event")?;
                if let Event::Key(key) = event {
                    if key.kind == KeyEventKind::Press && self.handle_key(key, &sessions).await? {
                        break;
                    }
                }
            }
        }

        let _ = self.controller.write_service_lifecycle(
            ServiceLifecycle::Stopped,
            self.service_started_at,
            self.service_tick,
            Vec::new(),
            None,
        );
        Ok(())
    }

    async fn tick_service_if_due(&mut self) {
        if self.last_service_tick_at.elapsed() < Duration::from_millis(UI_SERVICE_TICK_INTERVAL_MS)
        {
            return;
        }

        self.service_tick = self.service_tick.wrapping_add(1);
        if let Err(error) = self
            .controller
            .service_tick(
                self.service_started_at,
                self.service_tick,
                UI_SERVICE_STALL_AFTER_SECS,
            )
            .await
        {
            self.status_message = format!("scheduler tick failed: {error}");
        }
        self.last_service_tick_at = Instant::now();
    }

    fn input_wrap_width(&self) -> usize {
        self.input_wrap_width.max(1)
    }

    fn input_area_height(&self, total_width: u16, sessions: &[SessionOverviewSnapshot]) -> u16 {
        if matches!(self.input_mode, InputMode::Normal) {
            return 3;
        }
        let width = total_width.saturating_sub(2) as usize;
        let body_lines = editor_rows(&self.input_buffer, width.max(1)).len().max(1);
        let completion_count = self.current_input_completions(sessions).len();
        let wanted = body_lines
            + input_completion_line_count(&self.input_mode, completion_count)
            + input_help_line_count(&self.input_mode)
            + 2;
        wanted.clamp(MIN_INPUT_HEIGHT as usize, MAX_INPUT_HEIGHT as usize) as u16
    }

    fn current_input_completions(
        &self,
        sessions: &[SessionOverviewSnapshot],
    ) -> Vec<InputCompletion> {
        match &self.input_mode {
            InputMode::SlashCommand => self.input_completions(&PromptMode::Slash, sessions),
            InputMode::SpawnWorker => self.input_completions(&PromptMode::Spawn, sessions),
            _ => Vec::new(),
        }
    }

    fn clear_input(&mut self) {
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.history_cursor = None;
        self.completion_index = 0;
    }

    fn set_input(&mut self, text: impl Into<String>) {
        self.input_buffer = text.into();
        self.input_cursor = self.input_buffer.len();
        self.completion_index = 0;
    }

    fn replace_input_range(&mut self, start: usize, end: usize, replacement: &str) {
        self.history_cursor = None;
        self.completion_index = 0;
        self.input_buffer.replace_range(start..end, replacement);
        self.input_cursor = start + replacement.len();
    }

    fn insert_input_char(&mut self, ch: char) {
        self.history_cursor = None;
        self.completion_index = 0;
        self.input_buffer.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    fn insert_input_newline(&mut self) {
        self.insert_input_char('\n');
    }

    fn move_input_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        self.input_cursor = previous_char_boundary(&self.input_buffer, self.input_cursor);
    }

    fn move_input_right(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            return;
        }
        self.input_cursor = next_char_boundary(&self.input_buffer, self.input_cursor);
    }

    fn backspace_input(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        self.history_cursor = None;
        self.completion_index = 0;
        let start = previous_char_boundary(&self.input_buffer, self.input_cursor);
        self.input_buffer
            .replace_range(start..self.input_cursor, "");
        self.input_cursor = start;
    }

    fn delete_input(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            return;
        }
        self.history_cursor = None;
        self.completion_index = 0;
        let end = next_char_boundary(&self.input_buffer, self.input_cursor);
        self.input_buffer.replace_range(self.input_cursor..end, "");
    }

    fn history_entries(&self, scope: InputHistoryScope) -> &[String] {
        match scope {
            InputHistoryScope::Master => &self.input_history.master,
            InputHistoryScope::Worker => &self.input_history.worker,
            InputHistoryScope::Spawn => &self.input_history.spawn,
            InputHistoryScope::Slash => &self.input_history.slash,
        }
    }

    fn history_entries_mut(&mut self, scope: InputHistoryScope) -> &mut Vec<String> {
        match scope {
            InputHistoryScope::Master => &mut self.input_history.master,
            InputHistoryScope::Worker => &mut self.input_history.worker,
            InputHistoryScope::Spawn => &mut self.input_history.spawn,
            InputHistoryScope::Slash => &mut self.input_history.slash,
        }
    }

    fn record_input_history(&mut self, scope: InputHistoryScope, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        let entries = self.history_entries_mut(scope);
        if let Some(existing) = entries.iter().position(|entry| entry == trimmed) {
            entries.remove(existing);
        }
        entries.push(trimmed.to_owned());
        if entries.len() > INPUT_HISTORY_LIMIT {
            let overflow = entries.len() - INPUT_HISTORY_LIMIT;
            entries.drain(0..overflow);
        }
    }

    fn navigate_input_history(&mut self, scope: InputHistoryScope, previous: bool) -> bool {
        let len = self.history_entries(scope).len();
        if len == 0 {
            return false;
        }

        if previous {
            let next_index = match &self.history_cursor {
                Some(cursor) if cursor.scope == scope => cursor.index.saturating_sub(1),
                _ => len - 1,
            };
            let draft = match &self.history_cursor {
                Some(cursor) if cursor.scope == scope => cursor.draft.clone(),
                _ => self.input_buffer.clone(),
            };
            let value = self.history_entries(scope)[next_index].clone();
            self.history_cursor = Some(InputHistoryCursor {
                scope,
                index: next_index,
                draft,
            });
            self.set_input(value);
            return true;
        }

        let Some(cursor) = self
            .history_cursor
            .clone()
            .filter(|cursor| cursor.scope == scope)
        else {
            return false;
        };

        if cursor.index + 1 < len {
            let next_index = cursor.index + 1;
            let value = self.history_entries(scope)[next_index].clone();
            self.history_cursor = Some(InputHistoryCursor {
                scope,
                index: next_index,
                draft: cursor.draft,
            });
            self.set_input(value);
        } else {
            self.set_input(cursor.draft);
            self.history_cursor = None;
        }

        true
    }

    fn apply_input_completion(
        &mut self,
        mode: &PromptMode,
        sessions: &[SessionOverviewSnapshot],
    ) -> bool {
        let completions = self.input_completions(mode, sessions);
        let completion = completions
            .get(self.selected_completion_index(completions.len()))
            .cloned();

        let Some(completion) = completion else {
            return false;
        };

        self.replace_input_range(completion.start, completion.end, &completion.replacement);
        self.status_message = format!("completed {}", completion.label);
        true
    }

    fn input_completions(
        &self,
        mode: &PromptMode,
        sessions: &[SessionOverviewSnapshot],
    ) -> Vec<InputCompletion> {
        let session_ids = sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        let automation_ids = self
            .controller
            .list_session_automations()
            .into_iter()
            .map(|automation| automation.id)
            .collect::<Vec<_>>();
        match mode {
            PromptMode::Slash => slash_input_completions(
                &self.input_buffer,
                self.input_cursor,
                &self.available_groups,
                &session_ids,
                &automation_ids,
            ),
            PromptMode::Spawn => spawn_input_completions(
                &self.input_buffer,
                self.input_cursor,
                &self.available_groups,
            ),
            PromptMode::Master | PromptMode::Worker(_) => Vec::new(),
        }
    }

    fn selected_completion_index(&self, len: usize) -> usize {
        if len == 0 {
            0
        } else {
            self.completion_index.min(len - 1)
        }
    }

    fn cycle_input_completion(
        &mut self,
        mode: &PromptMode,
        sessions: &[SessionOverviewSnapshot],
        forward: bool,
    ) -> bool {
        let completions = self.input_completions(mode, sessions);
        if completions.is_empty() {
            return false;
        }
        let len = completions.len();
        let current = self.selected_completion_index(len);
        self.completion_index = if forward {
            (current + 1) % len
        } else if current == 0 {
            len - 1
        } else {
            current - 1
        };
        let selected = &completions[self.completion_index];
        self.status_message = format!(
            "suggestion {}/{}: {}",
            self.completion_index + 1,
            len,
            selected.label
        );
        true
    }

    fn move_input_home(&mut self) {
        let width = self.input_wrap_width();
        let rows = editor_rows(&self.input_buffer, width);
        let (row, _) = find_cursor_in_rows(&rows, self.input_cursor);
        self.input_cursor = rows[row].stops.first().map(|stop| stop.index).unwrap_or(0);
    }

    fn move_input_end(&mut self) {
        let width = self.input_wrap_width();
        let rows = editor_rows(&self.input_buffer, width);
        let (row, _) = find_cursor_in_rows(&rows, self.input_cursor);
        self.input_cursor = rows[row]
            .stops
            .last()
            .map(|stop| stop.index)
            .unwrap_or(self.input_cursor);
    }

    fn move_input_up(&mut self) {
        let width = self.input_wrap_width();
        let rows = editor_rows(&self.input_buffer, width);
        let (row, col) = find_cursor_in_rows(&rows, self.input_cursor);
        if row == 0 {
            return;
        }
        let target = &rows[row - 1];
        self.input_cursor = cursor_index_for_column(target, col);
    }

    fn move_input_down(&mut self) {
        let width = self.input_wrap_width();
        let rows = editor_rows(&self.input_buffer, width);
        let (row, col) = find_cursor_in_rows(&rows, self.input_cursor);
        if row + 1 >= rows.len() {
            return;
        }
        let target = &rows[row + 1];
        self.input_cursor = cursor_index_for_column(target, col);
    }

    fn draw(&self, frame: &mut Frame<'_>, sessions: &[SessionOverviewSnapshot]) {
        let input_height = self.input_area_height(frame.size().width, sessions);
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),
                Constraint::Length(2),
                Constraint::Length(input_height),
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
        sessions: &[SessionOverviewSnapshot],
        mut list_state: ListState,
    ) {
        let selected = sessions
            .iter()
            .find(|session| session.id == self.selected_id);
        let selected_accent = selected
            .map(|session| session_accent_color_for_kind(&session.kind))
            .unwrap_or(Color::Rgb(112, 122, 140));
        let running_count = sessions
            .iter()
            .filter(|session| is_busy_status(&session.status))
            .count();

        let items = sessions
            .iter()
            .map(|session| {
                let accent = session_accent_color_for_kind(&session.kind);
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
                            format!("[{}]", session_kind_badge_for_kind(&session.kind)),
                            Style::default().fg(accent).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(title, session_title_style(accent)),
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
        sessions: &[SessionOverviewSnapshot],
    ) {
        match self.detail_mode {
            DetailMode::Session => self.draw_selected_session_view(frame, area),
            DetailMode::Batch => self.draw_batch_view(frame, area, sessions),
        }
    }

    fn draw_selected_session_view(&self, frame: &mut Frame<'_>, area: Rect) {
        if self.selected_id == "onboard" {
            self.draw_onboard_view(frame, area);
            return;
        }
        let Some(selected) = self.controller.session_snapshot(&self.selected_id) else {
            let empty = Paragraph::new("No session selected")
                .block(Block::default().title("Session").borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        };
        let accent = session_accent_color_for_kind(&selected.kind);
        let border_style = panel_border_style(&selected.status, accent, self.animation_tick);
        let visible_timeline = selected
            .timeline_events
            .iter()
            .filter(|event| event_matches_filter(&event.kind, self.focus_filter))
            .count();
        let visible_logs = selected
            .log_lines
            .iter()
            .filter(|line| log_line_matches_filter(line, self.focus_filter))
            .count();

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(13),
                Constraint::Length(9),
                Constraint::Min(7),
            ])
            .split(area);

        let meta = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    animated_status_badge(&selected.status, self.animation_tick),
                    status_badge_style(&selected.status, self.animation_tick),
                ),
                Span::raw(" "),
                Span::styled(
                    &selected.title,
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!(
                        "[{}]",
                        status_caption(&selected.status, self.animation_tick)
                    ),
                    status_style(&selected.status, self.animation_tick),
                ),
            ]),
            Line::from(format!("id: {}", selected.id)),
            Line::from(session_identity_line(&selected)),
            Line::from(session_queue_line(&selected)),
            Line::from(session_batch_line(&selected)),
            Line::from(session_summary_line(&selected)),
            Line::from(session_lifecycle_note_line(&selected)),
            Line::from(session_latest_user_line(&selected)),
            Line::from(session_last_message_line(&selected)),
            Line::from(format!(
                "last turn: {}",
                selected
                    .last_turn_id
                    .clone()
                    .unwrap_or_else(|| "-".to_owned())
            )),
            Line::from(format!("thread: {}", selected.thread_id)),
            Line::from(session_location_line(&selected)),
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
            &selected.timeline_events,
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
                                activity_glyph(&selected.status, self.animation_tick)
                            ),
                            Style::default()
                                .fg(status_color(&selected.status, self.animation_tick)),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            focus_filter_chip(self.focus_filter),
                            focus_filter_style(self.focus_filter, accent),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{visible_timeline}/{}", selected.timeline_events.len()),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            timeline_status_hint(&selected.status),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(secondary_panel_border_style(
                        accent,
                        &selected.status,
                        self.animation_tick,
                    )),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(timeline, sections[1]);

        let body = log_text(
            &selected.log_lines,
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
                                activity_glyph(&selected.status, self.animation_tick)
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
                            format!("{visible_logs}/{}", selected.log_lines.len()),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            live_output_hint(&selected.status),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(secondary_panel_border_style(
                        accent,
                        &selected.status,
                        self.animation_tick,
                    )),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(detail, sections[2]);
    }

    fn draw_onboard_view(&self, frame: &mut Frame<'_>, area: Rect) {
        let onboard = self.controller.onboard_snapshot();
        let monitor = self.controller.monitor_snapshot();
        let accent = Color::Rgb(201, 216, 117);
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9),
                Constraint::Min(10),
                Constraint::Length(8),
            ])
            .split(area);

        let meta = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    animated_status_badge(&onboard.status, self.animation_tick),
                    status_badge_style(&onboard.status, self.animation_tick),
                ),
                Span::raw(" "),
                Span::styled(
                    "onboard",
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!(
                        "scheduler={} tick={}  runtime={} pid={}",
                        onboard
                            .service_status
                            .clone()
                            .unwrap_or_else(|| "unknown".to_owned()),
                        onboard
                            .service_tick
                            .map(|tick| tick.to_string())
                            .unwrap_or_else(|| "-".to_owned()),
                        if onboard.runtime_connected {
                            "connected"
                        } else {
                            "disconnected"
                        },
                        onboard
                            .runtime_pid
                            .map(|pid| pid.to_string())
                            .unwrap_or_else(|| "-".to_owned())
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            Line::from(format!("summary: {}", onboard.summary)),
            Line::from(format!(
                "active turns: {} | queued turns: {} | workers running: {}",
                onboard.active_turns, onboard.queued_turns, onboard.running_workers
            )),
            Line::from(format!(
                "codex sessions: {} total | {} active | {} queued | {} blocked",
                monitor.total_codex_sessions,
                monitor.active_codex_sessions,
                monitor.queued_codex_sessions,
                monitor.blocked_codex_sessions
            )),
            Line::from(format!(
                "runtime mode: {} | command: {}",
                onboard
                    .runtime_mode
                    .clone()
                    .unwrap_or_else(|| "-".to_owned()),
                onboard
                    .runtime_command_label
                    .clone()
                    .unwrap_or_else(|| "-".to_owned())
            )),
            Line::from(format!(
                "queued deliveries: {} | delegated loops: {} | auto approve: {} | budget exhausted: {}",
                onboard.queued_deliveries,
                onboard.delegated_jobs,
                onboard.auto_approve_jobs,
                onboard.budget_exhausted_jobs
            )),
            Line::from(format!(
                "automations: armed={} | paused={} | due now={}",
                onboard.armed_automations, onboard.paused_automations, onboard.due_automations
            )),
            Line::from(format!(
                "continued this tick: {}",
                if onboard.continued_jobs.is_empty() {
                    "-".to_owned()
                } else {
                    onboard.continued_jobs.join(", ")
                }
            )),
        ]))
        .block(
            Block::default()
                .title("Onboard")
                .borders(Borders::ALL)
                .border_style(panel_border_style(
                    &onboard.status,
                    accent,
                    self.animation_tick,
                )),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(meta, sections[0]);

        let center = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(5, 8), Constraint::Ratio(3, 8)])
            .split(sections[1]);
        let board = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
            ])
            .split(center[0]);

        let backlog = onboard
            .pending
            .iter()
            .chain(onboard.completed.iter())
            .cloned()
            .collect::<Vec<_>>();
        let blocked_failed = blocked_and_failed(&onboard);
        self.draw_onboard_lane(frame, board[0], "Pending / Completed", &backlog, accent);
        self.draw_onboard_lane(frame, board[1], "Running", &onboard.running, accent);
        self.draw_onboard_lane(frame, board[2], "Blocked / Failed", &blocked_failed, accent);

        let sessions_panel = Paragraph::new(monitor_sessions_text(
            &monitor.sessions,
            center[1].width.saturating_sub(4) as usize,
            center[1].height.saturating_sub(2) as usize,
        ))
        .block(
            Block::default()
                .title(format!("Codex Sessions ({})", monitor.total_codex_sessions))
                .borders(Borders::ALL)
                .border_style(secondary_panel_border_style(
                    accent,
                    if monitor.runtime_connected {
                        "running"
                    } else {
                        "blocked"
                    },
                    self.animation_tick,
                )),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(sessions_panel, center[1]);

        let footer = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
                Constraint::Ratio(1, 3),
            ])
            .split(sections[2]);
        let completed = Paragraph::new(onboard_lane_text(
            "Completed",
            &onboard.completed,
            footer[0].width.saturating_sub(4) as usize,
            footer[0].height.saturating_sub(2) as usize,
        ))
        .block(
            Block::default()
                .title("Completed")
                .borders(Borders::ALL)
                .border_style(secondary_panel_border_style(
                    accent,
                    "completed",
                    self.animation_tick,
                )),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(completed, footer[0]);

        let automations = Paragraph::new(session_automations_text(
            &onboard.automations,
            footer[1].width.saturating_sub(4) as usize,
            footer[1].height.saturating_sub(2) as usize,
        ))
        .block(
            Block::default()
                .title("Automations")
                .borders(Borders::ALL)
                .border_style(secondary_panel_border_style(
                    accent,
                    if onboard.due_automations > 0 {
                        "running"
                    } else if onboard
                        .automations
                        .iter()
                        .any(|automation| automation.status == "failed")
                    {
                        "blocked"
                    } else {
                        "completed"
                    },
                    self.animation_tick,
                )),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(automations, footer[1]);

        let controls = Paragraph::new(Text::from(vec![
            Line::from("commands: press / for slash command mode"),
            Line::from("automation: /automation create --to master --every-secs 300 \"...\""),
            Line::from("markers: AUTO = auto approve"),
            Line::from("markers: LOOP = delegated master loop"),
            Line::from("budget: time/iterations remaining when bounded"),
            Line::from(format!(
                "manual jobs: {}",
                onboard
                    .pending
                    .iter()
                    .chain(onboard.running.iter())
                    .chain(onboard.blocked.iter())
                    .filter(|item| item.automation.state == "manual")
                    .count()
            )),
            Line::from(format!("failed jobs: {}", onboard.failed.len())),
        ]))
        .block(
            Block::default()
                .title("Controls")
                .borders(Borders::ALL)
                .border_style(secondary_panel_border_style(
                    accent,
                    &onboard.status,
                    self.animation_tick,
                )),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(controls, footer[2]);
    }

    fn draw_onboard_lane(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        title: &str,
        items: &[OnboardLaneItem],
        accent: Color,
    ) {
        let lane = Paragraph::new(onboard_lane_text(
            title,
            items,
            area.width.saturating_sub(4) as usize,
            area.height.saturating_sub(2) as usize,
        ))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(secondary_panel_border_style(
                    accent,
                    if items
                        .iter()
                        .any(|item| item.status == "blocked" || item.status == "failed")
                    {
                        "blocked"
                    } else if items.iter().any(|item| item.status == "running") {
                        "running"
                    } else {
                        "completed"
                    },
                    self.animation_tick,
                )),
        )
        .wrap(Wrap { trim: false });
        frame.render_widget(lane, area);
    }

    fn draw_batch_view(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        sessions: &[SessionOverviewSnapshot],
    ) {
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
        let accent = session_accent_color_for_kind(&session.kind);
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

    fn draw_status_bar(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        sessions: &[SessionOverviewSnapshot],
    ) {
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
                    "selected={} | status={} | queued={} | batch={} | view={} | focus={} | target={} | keys: ↑↓ switch  / command  o onboard  i master  e worker  n spawn  f focus  b batch  [ ] cycle  g master  q quit",
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
                session_accent_color_for_kind(&session.kind),
                self.animation_tick,
            ))
        } else {
            Paragraph::new(status)
                .style(Style::default().bg(Color::Rgb(28, 32, 38)).fg(Color::White))
        };
        frame.render_widget(paragraph, area);
    }

    fn draw_input_bar(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        sessions: &[SessionOverviewSnapshot],
    ) {
        let selected = sessions
            .iter()
            .find(|session| session.id == self.selected_id);
        let completions = self.current_input_completions(sessions);
        let completion_count = completions.len();
        let completion_lines = input_completion_line_count(&self.input_mode, completion_count);
        let help_lines = input_help_line_count(&self.input_mode);
        let title = match &self.input_mode {
            InputMode::Normal => "Command (Enter or /)",
            InputMode::MasterPrompt => "Prompt -> master",
            InputMode::WorkerPrompt(_) => "Prompt -> worker",
            InputMode::SpawnWorker => "Spawn Worker (group: task)",
            InputMode::SlashCommand => "Slash Command",
        };
        let input_view = if matches!(self.input_mode, InputMode::Normal) {
            None
        } else {
            let content_height = area.height.saturating_sub(2) as usize;
            let reserved_lines = completion_lines + help_lines;
            let max_body_lines = content_height.saturating_sub(reserved_lines).max(1);
            Some(input_view_state(
                &self.input_buffer,
                self.input_cursor,
                area.width.saturating_sub(2) as usize,
                max_body_lines,
            ))
        };

        let body = match &self.input_mode {
            InputMode::Normal => self.status_message.clone(),
            _ => input_view
                .as_ref()
                .map(|view| view.body.clone())
                .unwrap_or_default(),
        };

        let selected_index = self.selected_completion_index(completion_count);
        let selected_completion = completions.get(selected_index);
        let (completion_start, completion_end) = completion_window(
            completion_count,
            selected_index,
            INPUT_COMPLETION_VISIBLE_ITEMS,
        );

        let help_lines_text = match &self.input_mode {
            InputMode::Normal => Vec::new(),
            InputMode::MasterPrompt => vec![
                "Enter sends. Alt+Enter or Ctrl+J inserts newline. Ctrl+P/N recalls prompt history. Esc cancels."
                    .to_owned(),
            ],
            InputMode::WorkerPrompt(worker_id) => vec![
                format!(
                    "worker target: {worker_id}. Enter sends. Alt+Enter or Ctrl+J inserts newline. Ctrl+P/N recalls history."
                ),
            ],
            InputMode::SpawnWorker => vec![if let Some(selected) = selected_completion {
                format!(
                    "selected: {} | Tab accept | Shift+Tab prev | Alt+Up/Down cycle | showing {}-{} of {}",
                    selected.label,
                    completion_start + 1,
                    completion_end,
                    completion_count
                )
            } else {
                "Format: backend: Payment API refactor. Tab completes groups. Ctrl+P/N recalls history."
                    .to_owned()
            }],
            InputMode::SlashCommand => slash_command_hint(
                &self.input_buffer,
                selected.map(|session| session.id.as_str()),
                &self.available_groups,
                selected_completion,
                completion_start,
                completion_end,
                completion_count,
            ),
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(input_border_style(
                &self.input_mode,
                selected,
                self.animation_tick,
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut constraints = vec![Constraint::Min(1)];
        if completion_lines > 0 {
            constraints.push(Constraint::Length(completion_lines as u16));
        }
        if help_lines > 0 {
            constraints.push(Constraint::Length(help_lines as u16));
        }
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        let body_paragraph = Paragraph::new(body).wrap(Wrap { trim: false });
        frame.render_widget(body_paragraph, sections[0]);

        let mut section_index = 1usize;
        if completion_lines > 0 {
            let accent = selected
                .map(|session| session_accent_color_for_kind(&session.kind))
                .unwrap_or(Color::Rgb(112, 122, 140));
            let items = completions[completion_start..completion_end]
                .iter()
                .map(|completion| {
                    ListItem::new(Line::from(Span::styled(
                        completion.label.clone(),
                        Style::default().fg(Color::Gray),
                    )))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default();
            state.select(Some(selected_index.saturating_sub(completion_start)));
            let list = List::new(items).highlight_symbol("› ").highlight_style(
                Style::default()
                    .bg(selected_highlight_bg(accent))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_stateful_widget(list, sections[section_index], &mut state);
            section_index += 1;
        }

        if help_lines > 0 {
            let lines = help_lines_text
                .into_iter()
                .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::DarkGray))))
                .collect::<Vec<_>>();
            let help_paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
            frame.render_widget(help_paragraph, sections[section_index]);
        }

        if let Some(view) = input_view {
            frame.set_cursor(sections[0].x + view.cursor_x, sections[0].y + view.cursor_y);
        }
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        sessions: &[SessionOverviewSnapshot],
    ) -> Result<bool> {
        match self.input_mode.clone() {
            InputMode::Normal => self.handle_normal_key(key, sessions).await,
            InputMode::MasterPrompt => {
                self.handle_input_key(key, PromptMode::Master, sessions)
                    .await
            }
            InputMode::WorkerPrompt(worker_id) => {
                self.handle_input_key(key, PromptMode::Worker(worker_id), sessions)
                    .await
            }
            InputMode::SpawnWorker => {
                self.handle_input_key(key, PromptMode::Spawn, sessions)
                    .await
            }
            InputMode::SlashCommand => {
                self.handle_input_key(key, PromptMode::Slash, sessions)
                    .await
            }
        }
    }

    async fn handle_normal_key(
        &mut self,
        key: KeyEvent,
        sessions: &[SessionOverviewSnapshot],
    ) -> Result<bool> {
        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Enter => {
                self.input_mode = InputMode::SlashCommand;
                self.set_input("/");
                self.status_message = "slash command mode".to_owned();
            }
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(sessions),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(sessions),
            KeyCode::Char('g') => {
                self.focus_session("master".to_owned(), sessions);
                self.status_message = "focused master".to_owned();
            }
            KeyCode::Char('o') => {
                self.focus_session("onboard".to_owned(), sessions);
                self.status_message = "focused onboard".to_owned();
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
                self.clear_input();
                self.status_message = "composing prompt to master".to_owned();
            }
            KeyCode::Char('e') => {
                if self.selected_id == "master" || self.selected_id == "onboard" {
                    self.status_message =
                        "selected session is not a worker; use `i` to send input to master"
                            .to_owned();
                } else {
                    self.input_mode = InputMode::WorkerPrompt(self.selected_id.clone());
                    self.clear_input();
                    self.status_message = format!("composing prompt to {}", self.selected_id);
                }
            }
            KeyCode::Char('n') => {
                self.input_mode = InputMode::SpawnWorker;
                self.clear_input();
                self.status_message = "spawn worker using `group: task`".to_owned();
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::SlashCommand;
                self.set_input("/");
                self.status_message = "slash command mode".to_owned();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.input_mode = InputMode::SlashCommand;
                self.set_input(format!("/{ch}"));
                self.status_message = "slash command mode".to_owned();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            _ => {}
        }

        Ok(false)
    }

    async fn handle_input_key(
        &mut self,
        key: KeyEvent,
        mode: PromptMode,
        sessions: &[SessionOverviewSnapshot],
    ) -> Result<bool> {
        let history_scope = prompt_history_scope(&mode);
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.clear_input();
                self.status_message = "cancelled".to_owned();
            }
            KeyCode::BackTab => {
                if !self.cycle_input_completion(&mode, sessions, false) {
                    self.status_message = "no completion available".to_owned();
                }
            }
            KeyCode::Tab => {
                if !self.apply_input_completion(&mode, sessions) {
                    self.status_message = "no completion available".to_owned();
                }
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert_input_newline();
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_input_newline();
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(scope) = history_scope {
                    if !self.navigate_input_history(scope, true) {
                        self.status_message = "no earlier history".to_owned();
                    }
                }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(scope) = history_scope {
                    if !self.navigate_input_history(scope, false) {
                        self.status_message = "no newer history".to_owned();
                    }
                }
            }
            KeyCode::Enter => {
                let buffer = self.input_buffer.trim().to_owned();
                if buffer.is_empty() {
                    self.status_message = "input is empty".to_owned();
                    self.input_mode = InputMode::Normal;
                    self.clear_input();
                    return Ok(false);
                }

                let result = match mode.clone() {
                    PromptMode::Master => self
                        .controller
                        .submit_prompt(PromptTarget::Master, &buffer)
                        .await
                        .map(|_| {
                            self.status_message = "submitted prompt to master".to_owned();
                        }),
                    PromptMode::Worker(worker_id) => self
                        .controller
                        .submit_prompt(PromptTarget::Worker(worker_id.clone()), &buffer)
                        .await
                        .map(|_| {
                            self.status_message = format!("submitted prompt to {worker_id}");
                        }),
                    PromptMode::Spawn => match parse_spawn_input(&buffer) {
                        Ok((group, task)) => {
                            self.controller
                                .spawn_worker(&group, &task)
                                .await
                                .map(|worker| {
                                    self.selected_id = worker.id.clone();
                                    self.detail_mode = DetailMode::Session;
                                    self.selected_batch_id = None;
                                    self.status_message = format!("spawned {}", worker.id);
                                })
                        }
                        Err(error) => Err(error),
                    },
                    PromptMode::Slash => self.execute_slash_command(&buffer).await,
                };

                if let Err(error) = result {
                    self.status_message = error.to_string();
                    return Ok(false);
                }

                if let Some(scope) = history_scope {
                    self.record_input_history(scope, &buffer);
                }
                self.input_mode = InputMode::Normal;
                self.clear_input();
            }
            KeyCode::Backspace => {
                if matches!(mode, PromptMode::Slash)
                    && self.input_buffer == "/"
                    && self.input_cursor <= 1
                {
                    return Ok(false);
                }
                self.backspace_input();
            }
            KeyCode::Delete => {
                self.delete_input();
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                if !self.cycle_input_completion(&mode, sessions, false) {
                    self.status_message = "no completion available".to_owned();
                }
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                if !self.cycle_input_completion(&mode, sessions, true) {
                    self.status_message = "no completion available".to_owned();
                }
            }
            KeyCode::Left => {
                self.move_input_left();
            }
            KeyCode::Right => {
                self.move_input_right();
            }
            KeyCode::Up => {
                self.move_input_up();
            }
            KeyCode::Down => {
                self.move_input_down();
            }
            KeyCode::Home => {
                self.move_input_home();
            }
            KeyCode::End => {
                self.move_input_end();
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_input_home();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_input_end();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.insert_input_char(ch);
                }
            }
            _ => {}
        }

        Ok(false)
    }

    fn list_state(&self, sessions: &[SessionOverviewSnapshot]) -> ListState {
        let mut state = ListState::default();
        let selected = sessions
            .iter()
            .position(|session| session.id == self.selected_id)
            .or(Some(0));
        state.select(selected);
        state
    }

    fn sync_selection(&mut self, sessions: &[SessionOverviewSnapshot]) {
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

    fn sync_batch_selection(&mut self, sessions: &[SessionOverviewSnapshot]) {
        if self.detail_mode != DetailMode::Batch {
            return;
        }
        if !sessions
            .iter()
            .any(|session| session.id == self.selected_id)
        {
            self.selected_batch_id = None;
            return;
        }
        let batch_ids = self.controller.session_batch_ids(&self.selected_id);
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

    fn sync_title(&mut self, sessions: &[SessionOverviewSnapshot]) -> Result<()> {
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

    fn select_previous(&mut self, sessions: &[SessionOverviewSnapshot]) {
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

    fn select_next(&mut self, sessions: &[SessionOverviewSnapshot]) {
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

    fn focus_session(&mut self, session_id: String, sessions: &[SessionOverviewSnapshot]) {
        self.selected_id = session_id.clone();
        if self.detail_mode == DetailMode::Batch {
            if sessions.iter().any(|session| session.id == session_id) {
                self.selected_batch_id = self
                    .controller
                    .session_batch_ids(&session_id)
                    .last()
                    .copied();
            } else {
                self.selected_batch_id = None;
            }
        } else {
            self.selected_batch_id = None;
        }
    }

    fn toggle_batch_view(&mut self, sessions: &[SessionOverviewSnapshot]) {
        match self.detail_mode {
            DetailMode::Session => {
                if !sessions
                    .iter()
                    .any(|session| session.id == self.selected_id)
                {
                    self.status_message = "no session selected".to_owned();
                    return;
                }
                let batch_ids = self.controller.session_batch_ids(&self.selected_id);
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

    fn select_previous_batch(&mut self, sessions: &[SessionOverviewSnapshot]) {
        if self.detail_mode != DetailMode::Batch {
            return;
        }
        if !sessions
            .iter()
            .any(|session| session.id == self.selected_id)
        {
            return;
        }
        let batch_ids = self.controller.session_batch_ids(&self.selected_id);
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

    fn select_next_batch(&mut self, sessions: &[SessionOverviewSnapshot]) {
        if self.detail_mode != DetailMode::Batch {
            return;
        }
        if !sessions
            .iter()
            .any(|session| session.id == self.selected_id)
        {
            return;
        }
        let batch_ids = self.controller.session_batch_ids(&self.selected_id);
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

    async fn execute_slash_command(&mut self, buffer: &str) -> Result<()> {
        match parse_slash_command(buffer)? {
            SlashCommand::Help => {
                self.status_message =
                    "commands: /job create, /automation, /send, /spawn, /focus, /monitor, /help"
                        .to_owned();
            }
            SlashCommand::Automation(command) => match command {
                SlashAutomationCommand::Create(command) => {
                    let automation = self.controller.create_session_automation(command.request)?;
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session("onboard".to_owned(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    self.status_message = format!(
                        "armed {} -> {} every {}s",
                        automation.id, automation.target_session_id, automation.interval_secs
                    );
                }
                SlashAutomationCommand::List => {
                    let automations = self.controller.list_session_automations();
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session("onboard".to_owned(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    self.status_message = format!(
                        "automation count={} armed={} paused={} failed={}",
                        automations.len(),
                        automations
                            .iter()
                            .filter(|automation| automation.status == "armed")
                            .count(),
                        automations
                            .iter()
                            .filter(|automation| automation.status == "paused")
                            .count(),
                        automations
                            .iter()
                            .filter(|automation| automation.status == "failed")
                            .count()
                    );
                }
                SlashAutomationCommand::Pause { automation_id } => {
                    let automation = self.controller.pause_session_automation(&automation_id)?;
                    let target = self
                        .controller
                        .session_automation_snapshot(&automation.id)
                        .map(|snapshot| snapshot.target_session_id)
                        .unwrap_or_else(|| automation.target_session_id.clone());
                    self.status_message = format!("paused {} -> {}", automation.id, target);
                }
                SlashAutomationCommand::Resume { automation_id } => {
                    let automation = self.controller.resume_session_automation(&automation_id)?;
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session("onboard".to_owned(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    let target = self
                        .controller
                        .session_automation_snapshot(&automation.id)
                        .map(|snapshot| snapshot.target_session_id)
                        .unwrap_or_else(|| automation.target_session_id.clone());
                    self.status_message = format!("resumed {} -> {}", automation.id, target);
                }
                SlashAutomationCommand::Cancel { automation_id } => {
                    let automation = self.controller.cancel_session_automation(&automation_id)?;
                    self.status_message = format!(
                        "cancelled {} -> {}",
                        automation.id, automation.target_session_id
                    );
                }
            },
            SlashCommand::Focus { session_id } => {
                let sessions = self.controller.sessions_overview_snapshot();
                if !sessions.iter().any(|session| session.id == session_id) {
                    anyhow::bail!("unknown session `{session_id}`");
                }
                self.focus_session(session_id.clone(), &sessions);
                self.status_message = format!("focused {session_id}");
            }
            SlashCommand::Send { target, prompt } => {
                let target = if target == "master" {
                    PromptTarget::Master
                } else {
                    PromptTarget::Worker(target.clone())
                };
                let target_name = target_label(&target);
                self.controller.submit_prompt(target, &prompt).await?;
                self.status_message = format!("queued prompt to {target_name}");
            }
            SlashCommand::Spawn { group, task } => {
                let worker = self.controller.spawn_worker(&group, &task).await?;
                self.status_message = format!(
                    "spawned {} for [{}] {}; use `/focus {}` to inspect",
                    worker.id, group, task, worker.id
                );
            }
            SlashCommand::Monitor(command) => match command {
                SlashMonitorCommand::Overview | SlashMonitorCommand::Sessions => {
                    let monitor = self.controller.monitor_snapshot();
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session("onboard".to_owned(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    self.status_message = format!(
                        "{} codex session(s) | active {} | queued {} | blocked {} | runtime {}",
                        monitor.total_codex_sessions,
                        monitor.active_codex_sessions,
                        monitor.queued_codex_sessions,
                        monitor.blocked_codex_sessions,
                        if monitor.runtime_connected {
                            "connected"
                        } else {
                            "disconnected"
                        }
                    );
                }
                SlashMonitorCommand::Runtime => {
                    let monitor = self.controller.monitor_snapshot();
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session("onboard".to_owned(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    self.status_message = format!(
                        "runtime {} pid={} mode={} command={}",
                        if monitor.runtime_connected {
                            "connected"
                        } else {
                            "disconnected"
                        },
                        monitor
                            .runtime_pid
                            .map(|pid| pid.to_string())
                            .unwrap_or_else(|| "-".to_owned()),
                        monitor.runtime_mode.unwrap_or_else(|| "-".to_owned()),
                        monitor
                            .runtime_command_label
                            .unwrap_or_else(|| "-".to_owned())
                    );
                }
                SlashMonitorCommand::Jobs => {
                    let onboard = self.controller.onboard_snapshot();
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session("onboard".to_owned(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    self.status_message = format!(
                        "jobs pending={} running={} blocked={} completed={} failed={}",
                        onboard.pending.len(),
                        onboard.running.len(),
                        onboard.blocked.len(),
                        onboard.completed.len(),
                        onboard.failed.len()
                    );
                }
                SlashMonitorCommand::Session { session_id } => {
                    if session_id == "onboard" {
                        anyhow::bail!(
                            "`onboard` is the supervisor view; use `/monitor runtime` or `/focus onboard`"
                        );
                    }
                    let Some(session) = self.controller.monitor_session_snapshot(&session_id)
                    else {
                        anyhow::bail!("unknown session `{session_id}`");
                    };
                    let sessions = self.controller.sessions_overview_snapshot();
                    self.focus_session(session_id.clone(), &sessions);
                    self.detail_mode = DetailMode::Session;
                    self.status_message = format!(
                        "{} [{} | {}] {} batch={} usr={} rsp={}",
                        session.id,
                        session.role,
                        session.work_state,
                        truncate(&session.title, 20),
                        session
                            .latest_batch_id
                            .map(|batch_id| format!("b{batch_id}"))
                            .unwrap_or_else(|| "-".to_owned()),
                        truncate(
                            session
                                .latest_user_prompt
                                .as_deref()
                                .or(session.task.as_deref())
                                .unwrap_or("-"),
                            28,
                        ),
                        truncate(
                            session
                                .latest_response
                                .as_deref()
                                .or(session.summary.as_deref())
                                .unwrap_or("-"),
                            28,
                        )
                    );
                }
            },
            SlashCommand::JobCreate(command) => {
                let SlashJobCreateCommand {
                    request,
                    defer,
                    start_target,
                } = command;
                let job = self.controller.create_job(request)?;
                if defer {
                    self.status_message =
                        format!("created {} and left it queued on onboard", job.id);
                    return Ok(());
                }

                match start_target {
                    SlashJobStartTarget::Master => {
                        let batch_id = self
                            .controller
                            .submit_prompt_for_job_with_batch(
                                PromptTarget::Master,
                                &job_intake_prompt(&job),
                                Some(&job.id),
                            )
                            .await?;
                        self.status_message = format!(
                            "created {} and queued intake on master as b{batch_id:03}",
                            job.id
                        );
                    }
                    SlashJobStartTarget::ExistingSession(session_id) => {
                        let batch_id = self
                            .controller
                            .submit_prompt_for_job_with_batch(
                                PromptTarget::Worker(session_id.clone()),
                                &worker_job_prompt(&job, &session_id),
                                Some(&job.id),
                            )
                            .await?;
                        self.status_message = format!(
                            "created {} and queued it on {} as b{batch_id:03}",
                            job.id, session_id
                        );
                    }
                    SlashJobStartTarget::NewWorkerGroup(group) => {
                        let worker = self
                            .controller
                            .spawn_worker_for_job(&group, &job.title, Some(&job.id))
                            .await?;
                        self.status_message = format!(
                            "created {} and opened {} in group {}; use `/focus {}` to inspect",
                            job.id, worker.id, group, worker.id
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
enum PromptMode {
    Master,
    Worker(String),
    Spawn,
    Slash,
}

#[derive(Debug, Clone)]
enum SlashCommand {
    Help,
    JobCreate(SlashJobCreateCommand),
    Automation(SlashAutomationCommand),
    Send { target: String, prompt: String },
    Spawn { group: String, task: String },
    Focus { session_id: String },
    Monitor(SlashMonitorCommand),
}

#[derive(Debug, Clone)]
struct SlashJobCreateCommand {
    request: CreateJobRequest,
    defer: bool,
    start_target: SlashJobStartTarget,
}

#[derive(Debug, Clone)]
enum SlashJobStartTarget {
    Master,
    ExistingSession(String),
    NewWorkerGroup(String),
}

#[derive(Debug, Clone)]
enum SlashMonitorCommand {
    Overview,
    Sessions,
    Runtime,
    Jobs,
    Session { session_id: String },
}

#[derive(Debug, Clone)]
enum SlashAutomationCommand {
    Create(SlashAutomationCreateCommand),
    List,
    Pause { automation_id: String },
    Resume { automation_id: String },
    Cancel { automation_id: String },
}

#[derive(Debug, Clone)]
struct SlashAutomationCreateCommand {
    request: CreateSessionAutomationRequest,
}

#[derive(Debug, Clone)]
struct InputTokenSpan {
    start: usize,
    end: usize,
    text: String,
    quoted: bool,
}

#[derive(Debug, Clone)]
struct InputTokenContext {
    start: usize,
    end: usize,
    token: String,
    quoted: bool,
    tokens_before: Vec<String>,
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

fn prompt_history_scope(mode: &PromptMode) -> Option<InputHistoryScope> {
    match mode {
        PromptMode::Master => Some(InputHistoryScope::Master),
        PromptMode::Worker(_) => Some(InputHistoryScope::Worker),
        PromptMode::Spawn => Some(InputHistoryScope::Spawn),
        PromptMode::Slash => Some(InputHistoryScope::Slash),
    }
}

const INPUT_HELP_LINES: usize = 2;
const MIN_INPUT_HEIGHT: u16 = 3;
const MAX_INPUT_HEIGHT: u16 = 10;
const INPUT_HISTORY_LIMIT: usize = 32;
const INPUT_COMPLETION_VISIBLE_ITEMS: usize = 3;
const UI_SERVICE_TICK_INTERVAL_MS: u64 = 1500;
const UI_SERVICE_STALL_AFTER_SECS: u64 = 900;

#[derive(Debug, Clone)]
struct EditorRow {
    text: String,
    stops: Vec<EditorCursorStop>,
}

#[derive(Debug, Clone)]
struct EditorCursorStop {
    index: usize,
    column: usize,
}

#[derive(Debug, Clone)]
struct InputViewState {
    body: String,
    cursor_x: u16,
    cursor_y: u16,
}

fn input_view_state(
    text: &str,
    cursor: usize,
    width: usize,
    max_body_lines: usize,
) -> InputViewState {
    let rows = editor_rows(text, width.max(1));
    let (cursor_row, cursor_col) = find_cursor_in_rows(&rows, cursor);
    let visible_lines = max_body_lines.max(1);
    let start_line = cursor_row.saturating_add(1).saturating_sub(visible_lines);
    let end_line = min(rows.len(), start_line + visible_lines);
    let body = rows[start_line..end_line]
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>()
        .join("\n");

    InputViewState {
        body,
        cursor_x: min(cursor_col, width) as u16,
        cursor_y: cursor_row.saturating_sub(start_line) as u16,
    }
}

fn input_help_line_count(mode: &InputMode) -> usize {
    match mode {
        InputMode::Normal => 0,
        InputMode::SlashCommand => INPUT_HELP_LINES,
        _ => 1,
    }
}

fn input_completion_line_count(mode: &InputMode, completion_count: usize) -> usize {
    match mode {
        InputMode::SlashCommand | InputMode::SpawnWorker if completion_count > 0 => {
            min(completion_count, INPUT_COMPLETION_VISIBLE_ITEMS)
        }
        _ => 0,
    }
}

fn completion_window(len: usize, selected_index: usize, visible_items: usize) -> (usize, usize) {
    if len == 0 || visible_items == 0 {
        return (0, 0);
    }

    let visible_items = visible_items.min(len);
    let selected_index = selected_index.min(len - 1);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible_items);
    let end = min(len, start + visible_items);
    (start, end)
}

fn editor_rows(text: &str, width: usize) -> Vec<EditorRow> {
    let width = width.max(1);
    let mut rows = vec![EditorRow {
        text: String::new(),
        stops: vec![EditorCursorStop {
            index: 0,
            column: 0,
        }],
    }];
    let mut current_width = 0usize;
    let mut row_index = 0usize;

    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            rows.push(EditorRow {
                text: String::new(),
                stops: vec![EditorCursorStop {
                    index: index + ch.len_utf8(),
                    column: 0,
                }],
            });
            row_index += 1;
            current_width = 0;
            continue;
        }

        let char_width = char_display_width(ch).min(width).max(1);
        if current_width > 0 && current_width + char_width > width {
            rows.push(EditorRow {
                text: String::new(),
                stops: vec![EditorCursorStop { index, column: 0 }],
            });
            row_index += 1;
            current_width = 0;
        }

        rows[row_index].text.push(ch);
        current_width += char_width;
        rows[row_index].stops.push(EditorCursorStop {
            index: index + ch.len_utf8(),
            column: current_width,
        });
    }

    rows
}

fn find_cursor_in_rows(rows: &[EditorRow], cursor: usize) -> (usize, usize) {
    for (row_index, row) in rows.iter().enumerate() {
        if let Some(stop) = row.stops.iter().find(|stop| stop.index == cursor) {
            return (row_index, stop.column);
        }
    }

    let row_index = rows.len().saturating_sub(1);
    let column = rows
        .last()
        .and_then(|row| row.stops.last())
        .map(|stop| stop.column)
        .unwrap_or(0);
    (row_index, column)
}

fn cursor_index_for_column(row: &EditorRow, target_column: usize) -> usize {
    row.stops
        .iter()
        .find(|stop| stop.column >= target_column)
        .map(|stop| stop.index)
        .or_else(|| row.stops.last().map(|stop| stop.index))
        .unwrap_or(0)
}

fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
        .unwrap_or(cursor)
}

fn input_token_spans(input: &str) -> Vec<InputTokenSpan> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut current = String::new();
    let mut quoted = false;
    let mut active_quote = None;

    for (index, ch) in input.char_indices() {
        if let Some(quote) = active_quote {
            if ch == quote {
                active_quote = None;
                quoted = true;
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                if start.is_none() {
                    start = Some(index);
                }
                quoted = true;
                active_quote = Some(ch);
            }
            ch if ch.is_whitespace() => {
                if let Some(start) = start.take() {
                    tokens.push(InputTokenSpan {
                        start,
                        end: index,
                        text: current.clone(),
                        quoted,
                    });
                    current.clear();
                    quoted = false;
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(index);
                }
                current.push(ch);
            }
        }
    }

    if let Some(start) = start {
        tokens.push(InputTokenSpan {
            start,
            end: input.len(),
            text: current,
            quoted: quoted || active_quote.is_some(),
        });
    }

    tokens
}

fn current_input_token_context(input: &str, cursor: usize) -> InputTokenContext {
    let cursor = cursor.min(input.len());
    let tokens = input_token_spans(input);
    let ends_with_space = input[..cursor]
        .chars()
        .last()
        .map(|ch| ch.is_whitespace())
        .unwrap_or(true);

    if ends_with_space {
        return InputTokenContext {
            start: cursor,
            end: cursor,
            token: String::new(),
            quoted: false,
            tokens_before: tokens.into_iter().map(|token| token.text).collect(),
        };
    }

    for (index, token) in tokens.iter().enumerate() {
        if cursor >= token.start && cursor <= token.end {
            return InputTokenContext {
                start: token.start,
                end: token.end,
                token: token.text.clone(),
                quoted: token.quoted,
                tokens_before: tokens[..index]
                    .iter()
                    .map(|token| token.text.clone())
                    .collect(),
            };
        }
    }

    InputTokenContext {
        start: cursor,
        end: cursor,
        token: String::new(),
        quoted: false,
        tokens_before: tokens.into_iter().map(|token| token.text).collect(),
    }
}

fn push_completion(
    completions: &mut Vec<InputCompletion>,
    context: &InputTokenContext,
    replacement: impl Into<String>,
    label: impl Into<String>,
) {
    completions.push(InputCompletion {
        label: label.into(),
        replacement: replacement.into(),
        start: context.start,
        end: context.end,
    });
}

fn dedup_completions(completions: Vec<InputCompletion>) -> Vec<InputCompletion> {
    let mut unique = Vec::new();
    for completion in completions {
        if unique
            .iter()
            .any(|existing: &InputCompletion| existing.replacement == completion.replacement)
        {
            continue;
        }
        unique.push(completion);
    }
    unique
}

fn completion_match_score(candidate: &str, partial: &str) -> Option<i32> {
    let partial = partial.trim();
    if partial.is_empty() {
        return Some(0);
    }

    let candidate = candidate.to_ascii_lowercase();
    let partial = partial.to_ascii_lowercase();

    if candidate.starts_with(&partial) {
        return Some(10_000 - candidate.len() as i32);
    }

    if let Some(index) = candidate.find(&partial) {
        return Some(8_000 - (index as i32 * 10) - candidate.len() as i32);
    }

    let mut search_from = 0usize;
    let mut matched = Vec::new();
    for ch in partial.chars() {
        let Some(next_index) = candidate[search_from..].find(ch) else {
            return None;
        };
        let absolute = search_from + next_index;
        matched.push(absolute);
        search_from = absolute + 1;
    }

    let spread = matched
        .last()
        .copied()
        .unwrap_or(0)
        .saturating_sub(matched.first().copied().unwrap_or(0));
    Some(5_000 - spread as i32 - candidate.len() as i32)
}

fn starts_with_partial(candidate: &str, partial: &str) -> bool {
    completion_match_score(candidate, partial).is_some()
}

fn completion_sort_text(completion: &InputCompletion, partial: &str) -> String {
    let mut text = completion.replacement.trim().to_owned();
    if !partial.starts_with('/') {
        text = text.trim_start_matches('/').to_owned();
    }
    text
}

fn sort_input_completions(
    mut completions: Vec<InputCompletion>,
    partial: &str,
) -> Vec<InputCompletion> {
    completions.sort_by(|left, right| {
        let left_score = completion_match_score(&completion_sort_text(left, partial), partial)
            .unwrap_or(i32::MIN);
        let right_score = completion_match_score(&completion_sort_text(right, partial), partial)
            .unwrap_or(i32::MIN);
        right_score
            .cmp(&left_score)
            .then_with(|| left.label.cmp(&right.label))
    });
    completions
}

fn slash_input_completions(
    buffer: &str,
    cursor: usize,
    groups: &[String],
    session_ids: &[String],
    automation_ids: &[String],
) -> Vec<InputCompletion> {
    let context = current_input_token_context(buffer, cursor);
    if context.quoted {
        return Vec::new();
    }

    let mut completions = Vec::new();
    if context.tokens_before.is_empty() {
        let partial = context.token.trim_start_matches('/');
        for command in [
            "job",
            "automation",
            "send",
            "spawn",
            "focus",
            "monitor",
            "help",
        ] {
            if context.token.is_empty() || starts_with_partial(command, partial) {
                push_completion(
                    &mut completions,
                    &context,
                    format!("/{command} "),
                    format!("/{command}"),
                );
            }
        }
        return sort_input_completions(dedup_completions(completions), partial);
    }

    let command = context.tokens_before[0].trim_start_matches('/');
    match command {
        "job" => completions.extend(job_input_completions(&context, groups, session_ids)),
        "automation" => completions.extend(automation_input_completions(
            &context,
            session_ids,
            automation_ids,
        )),
        "send" => completions.extend(send_input_completions(&context, session_ids)),
        "focus" => completions.extend(focus_input_completions(&context, session_ids)),
        "spawn" => completions.extend(spawn_slash_completions(&context, groups)),
        "monitor" => completions.extend(monitor_input_completions(&context, session_ids)),
        _ => {}
    }

    sort_input_completions(dedup_completions(completions), &context.token)
}

fn job_input_completions(
    context: &InputTokenContext,
    groups: &[String],
    session_ids: &[String],
) -> Vec<InputCompletion> {
    let mut completions = Vec::new();
    if context.tokens_before.len() == 1 {
        if context.token.is_empty() || starts_with_partial("create", &context.token) {
            push_completion(&mut completions, context, "create ", "create");
        }
        return completions;
    }

    if context.tokens_before.get(1).map(String::as_str) != Some("create") {
        return completions;
    }

    let previous = context.tokens_before.last().map(String::as_str);
    match previous {
        Some("--start-group") => {
            for group in groups {
                if context.token.is_empty() || starts_with_partial(group, &context.token) {
                    push_completion(&mut completions, context, group.clone(), group.clone());
                }
            }
            return completions;
        }
        Some("--start-session") => {
            for session_id in worker_session_ids(session_ids) {
                if context.token.is_empty() || starts_with_partial(&session_id, &context.token) {
                    push_completion(&mut completions, context, session_id.clone(), session_id);
                }
            }
            return completions;
        }
        Some("--priority") => {
            for value in ["low", "normal", "high", "urgent"] {
                if context.token.is_empty() || starts_with_partial(value, &context.token) {
                    push_completion(&mut completions, context, value, value);
                }
            }
            return completions;
        }
        Some("--pattern") => {
            for value in ["supervisor_worker", "planner_executor_reviewer"] {
                if context.token.is_empty() || starts_with_partial(value, &context.token) {
                    push_completion(&mut completions, context, value, value);
                }
            }
            return completions;
        }
        Some("--channel") => {
            for value in ["tui", "cli", "im"] {
                if context.token.is_empty() || starts_with_partial(value, &context.token) {
                    push_completion(&mut completions, context, value, value);
                }
            }
            return completions;
        }
        Some(flag) if flag.starts_with("--") => return completions,
        _ => {}
    }

    let flags = [
        ("--start-group ", "--start-group <group>"),
        ("--start-session ", "--start-session <session-id>"),
        ("--defer ", "--defer"),
        ("--delegate-master-loop ", "--delegate-master-loop"),
        ("--continue-for-secs ", "--continue-for-secs <secs>"),
        (
            "--continue-max-iterations ",
            "--continue-max-iterations <count>",
        ),
        ("--auto-approve ", "--auto-approve"),
        ("--approval-required ", "--approval-required"),
        ("--context ", "--context <text>"),
        ("--priority ", "--priority <level>"),
        ("--pattern ", "--pattern <name>"),
        ("--objective ", "--objective <text>"),
        ("--channel ", "--channel <name>"),
        ("--requester ", "--requester <name>"),
    ];

    for (replacement, label) in flags {
        let token = replacement.trim_end();
        if context.token.is_empty() || starts_with_partial(token, &context.token) {
            push_completion(&mut completions, context, replacement, label);
        }
    }

    completions
}

fn automation_input_completions(
    context: &InputTokenContext,
    session_ids: &[String],
    automation_ids: &[String],
) -> Vec<InputCompletion> {
    let mut completions = Vec::new();
    if context.tokens_before.len() == 1 {
        for subcommand in ["create", "list", "pause", "resume", "cancel"] {
            if context.token.is_empty() || starts_with_partial(subcommand, &context.token) {
                push_completion(
                    &mut completions,
                    context,
                    format!("{subcommand} "),
                    subcommand,
                );
            }
        }
        return completions;
    }

    let previous = context.tokens_before.last().map(String::as_str);
    match context.tokens_before.get(1).map(String::as_str) {
        Some("create") => {
            if matches!(previous, Some("--to")) {
                for session_id in send_targets(session_ids) {
                    if context.token.is_empty() || starts_with_partial(&session_id, &context.token)
                    {
                        push_completion(&mut completions, context, session_id.clone(), session_id);
                    }
                }
                return completions;
            }

            if matches!(
                previous,
                Some("--to" | "--every-secs" | "--max-runs" | "--for-secs")
            ) {
                return completions;
            }

            for (replacement, label) in [
                ("--to ", "--to <master|session-id>"),
                ("--every-secs ", "--every-secs <secs>"),
                ("--max-runs ", "--max-runs <count>"),
                ("--for-secs ", "--for-secs <secs>"),
            ] {
                let token = replacement.trim_end();
                if context.token.is_empty() || starts_with_partial(token, &context.token) {
                    push_completion(&mut completions, context, replacement, label);
                }
            }
        }
        Some("pause") | Some("resume") | Some("cancel") => {
            if context.tokens_before.len() != 2 {
                return completions;
            }
            for automation_id in automation_ids {
                if context.token.is_empty() || starts_with_partial(automation_id, &context.token) {
                    push_completion(
                        &mut completions,
                        context,
                        automation_id.clone(),
                        automation_id.clone(),
                    );
                }
            }
        }
        _ => {}
    }

    completions
}

fn send_input_completions(
    context: &InputTokenContext,
    session_ids: &[String],
) -> Vec<InputCompletion> {
    let mut completions = Vec::new();
    if context.tokens_before.len() != 1 {
        return completions;
    }

    for target in send_targets(session_ids) {
        if context.token.is_empty() || starts_with_partial(&target, &context.token) {
            push_completion(&mut completions, context, format!("{target} "), target);
        }
    }

    completions
}

fn focus_input_completions(
    context: &InputTokenContext,
    session_ids: &[String],
) -> Vec<InputCompletion> {
    let mut completions = Vec::new();
    if context.tokens_before.len() != 1 {
        return completions;
    }

    for target in focus_targets(session_ids) {
        if context.token.is_empty() || starts_with_partial(&target, &context.token) {
            push_completion(&mut completions, context, target.clone(), target);
        }
    }

    completions
}

fn spawn_slash_completions(context: &InputTokenContext, groups: &[String]) -> Vec<InputCompletion> {
    if context.tokens_before.len() != 1 {
        return Vec::new();
    }
    spawn_group_completions(context, groups)
}

fn monitor_input_completions(
    context: &InputTokenContext,
    session_ids: &[String],
) -> Vec<InputCompletion> {
    let mut completions = Vec::new();
    if context.tokens_before.len() == 1 {
        for subcommand in ["sessions", "runtime", "jobs", "session"] {
            if context.token.is_empty() || starts_with_partial(subcommand, &context.token) {
                push_completion(
                    &mut completions,
                    context,
                    format!("{subcommand} "),
                    subcommand,
                );
            }
        }
        return completions;
    }

    if context.tokens_before.get(1).map(String::as_str) != Some("session") {
        return completions;
    }

    for session_id in focus_targets(session_ids) {
        if context.token.is_empty() || starts_with_partial(&session_id, &context.token) {
            push_completion(&mut completions, context, session_id.clone(), session_id);
        }
    }

    completions
}

fn spawn_input_completions(buffer: &str, cursor: usize, groups: &[String]) -> Vec<InputCompletion> {
    let context = current_input_token_context(buffer, cursor);
    if context.quoted {
        return Vec::new();
    }
    if !context.tokens_before.is_empty() {
        return Vec::new();
    }
    sort_input_completions(
        spawn_group_completions(&context, groups),
        context.token.trim_end_matches(':'),
    )
}

fn spawn_group_completions(context: &InputTokenContext, groups: &[String]) -> Vec<InputCompletion> {
    let mut completions = Vec::new();
    let prefix = context.token.trim_end_matches(':');
    if context.tokens_before.is_empty() && context.token.contains(':') {
        for group in groups {
            if starts_with_partial(group, prefix) {
                push_completion(
                    &mut completions,
                    context,
                    format!("{group}: "),
                    format!("{group}:"),
                );
            }
        }
        return completions;
    }

    for group in groups {
        if context.token.is_empty() || starts_with_partial(group, prefix) {
            push_completion(
                &mut completions,
                context,
                format!("{group}: "),
                format!("{group}:"),
            );
        }
    }

    completions
}

fn worker_session_ids(session_ids: &[String]) -> Vec<String> {
    session_ids
        .iter()
        .filter(|session_id| session_id.as_str() != "onboard" && session_id.as_str() != "master")
        .cloned()
        .collect()
}

fn send_targets(session_ids: &[String]) -> Vec<String> {
    let mut targets = vec!["master".to_owned()];
    for session_id in worker_session_ids(session_ids) {
        if !targets.iter().any(|existing| existing == &session_id) {
            targets.push(session_id);
        }
    }
    targets
}

fn focus_targets(session_ids: &[String]) -> Vec<String> {
    let mut targets = vec!["onboard".to_owned(), "master".to_owned()];
    for session_id in session_ids {
        if !targets.iter().any(|existing| existing == session_id) {
            targets.push(session_id.clone());
        }
    }
    targets
}

fn parse_slash_command(input: &str) -> Result<SlashCommand> {
    let tokens = shell_split(input).context("failed to parse slash command")?;
    if tokens.is_empty() {
        return Ok(SlashCommand::Help);
    }

    let head = tokens[0].trim_start_matches('/');
    match head {
        "" | "help" => Ok(SlashCommand::Help),
        "job" => parse_job_command(&tokens[1..]),
        "automation" => parse_automation_command(&tokens[1..]),
        "send" => parse_send_command(&tokens[1..]),
        "spawn" => parse_spawn_command(&tokens[1..]),
        "focus" => parse_focus_command(&tokens[1..]),
        "monitor" => parse_monitor_command(&tokens[1..]),
        other => anyhow::bail!("unknown slash command `/{other}`"),
    }
}

fn parse_job_command(args: &[String]) -> Result<SlashCommand> {
    let Some(subcommand) = args.first() else {
        anyhow::bail!("usage: /job create <title> [options]");
    };
    match subcommand.as_str() {
        "create" => parse_job_create_command(&args[1..]),
        other => anyhow::bail!("unsupported job subcommand `{other}`"),
    }
}

fn parse_job_create_command(args: &[String]) -> Result<SlashCommand> {
    let mut objective = None;
    let mut source_channel = "tui".to_owned();
    let mut requester = None;
    let mut priority = "normal".to_owned();
    let mut pattern = "supervisor_worker".to_owned();
    let mut approval_required = false;
    let mut auto_approve = false;
    let mut delegate_to_master_loop = false;
    let mut continue_for_secs = None;
    let mut continue_max_iterations = None;
    let mut context = None;
    let mut defer = false;
    let mut start_session = None;
    let mut start_group = None;
    let mut positional = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--objective" => {
                objective = Some(next_slash_value(args, &mut index, "--objective")?);
            }
            "--channel" => {
                source_channel = next_slash_value(args, &mut index, "--channel")?;
            }
            "--requester" => {
                requester = Some(next_slash_value(args, &mut index, "--requester")?);
            }
            "--priority" => {
                priority = next_slash_value(args, &mut index, "--priority")?;
            }
            "--pattern" => {
                pattern = next_slash_value(args, &mut index, "--pattern")?;
            }
            "--approval-required" => approval_required = true,
            "--auto-approve" => auto_approve = true,
            "--delegate-master-loop" => delegate_to_master_loop = true,
            "--continue-for-secs" => {
                let value = next_slash_value(args, &mut index, "--continue-for-secs")?;
                continue_for_secs = Some(
                    value
                        .parse::<u64>()
                        .with_context(|| format!("invalid --continue-for-secs value `{value}`"))?,
                );
            }
            "--continue-max-iterations" => {
                let value = next_slash_value(args, &mut index, "--continue-max-iterations")?;
                continue_max_iterations = Some(value.parse::<u32>().with_context(|| {
                    format!("invalid --continue-max-iterations value `{value}`")
                })?);
            }
            "--context" => {
                context = Some(next_slash_value(args, &mut index, "--context")?);
            }
            "--defer" => defer = true,
            "--start-session" => {
                start_session = Some(next_slash_value(args, &mut index, "--start-session")?);
            }
            "--start-group" => {
                start_group = Some(next_slash_value(args, &mut index, "--start-group")?);
            }
            other if other.starts_with("--") => {
                anyhow::bail!("unsupported job create option `{other}`");
            }
            _ => positional.push(arg.clone()),
        }
        index += 1;
    }

    if start_session.is_some() && start_group.is_some() {
        anyhow::bail!("use either --start-session or --start-group, not both");
    }

    let title = positional.join(" ").trim().to_owned();
    if title.is_empty() {
        anyhow::bail!("usage: /job create <title> [options]");
    }
    let objective = objective.unwrap_or_else(|| title.clone());
    let start_target = if let Some(session_id) = start_session {
        SlashJobStartTarget::ExistingSession(session_id)
    } else if let Some(group) = start_group {
        SlashJobStartTarget::NewWorkerGroup(group)
    } else {
        SlashJobStartTarget::Master
    };

    Ok(SlashCommand::JobCreate(SlashJobCreateCommand {
        request: CreateJobRequest {
            title,
            objective,
            source_channel,
            requester,
            priority,
            pattern,
            approval_required,
            auto_approve,
            delegate_to_master_loop,
            continue_for_secs,
            continue_max_iterations,
            context,
        },
        defer,
        start_target,
    }))
}

fn next_slash_value(args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .with_context(|| format!("missing value for `{flag}`"))
}

fn parse_automation_command(args: &[String]) -> Result<SlashCommand> {
    let Some(subcommand) = args.first() else {
        return Ok(SlashCommand::Automation(SlashAutomationCommand::List));
    };
    match subcommand.as_str() {
        "create" => parse_automation_create_command(&args[1..]),
        "list" => Ok(SlashCommand::Automation(SlashAutomationCommand::List)),
        "pause" => parse_named_automation_command(&args[1..], "pause"),
        "resume" => parse_named_automation_command(&args[1..], "resume"),
        "cancel" => parse_named_automation_command(&args[1..], "cancel"),
        other => anyhow::bail!("unsupported automation subcommand `{other}`"),
    }
}

fn parse_automation_create_command(args: &[String]) -> Result<SlashCommand> {
    let mut target_session_id = None;
    let mut interval_secs = None;
    let mut max_runs = None;
    let mut run_for_secs = None;
    let mut positional = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--to" => {
                target_session_id = Some(next_slash_value(args, &mut index, "--to")?);
            }
            "--every-secs" => {
                let value = next_slash_value(args, &mut index, "--every-secs")?;
                interval_secs = Some(
                    value
                        .parse::<u64>()
                        .with_context(|| format!("invalid --every-secs value `{value}`"))?,
                );
            }
            "--max-runs" => {
                let value = next_slash_value(args, &mut index, "--max-runs")?;
                max_runs = Some(
                    value
                        .parse::<u32>()
                        .with_context(|| format!("invalid --max-runs value `{value}`"))?,
                );
            }
            "--for-secs" => {
                let value = next_slash_value(args, &mut index, "--for-secs")?;
                run_for_secs = Some(
                    value
                        .parse::<u64>()
                        .with_context(|| format!("invalid --for-secs value `{value}`"))?,
                );
            }
            other if other.starts_with("--") => {
                anyhow::bail!("unsupported automation create option `{other}`");
            }
            _ => positional.push(arg.clone()),
        }
        index += 1;
    }

    let prompt = positional.join(" ").trim().to_owned();
    if prompt.is_empty() {
        anyhow::bail!(
            "usage: /automation create --to <master|session-id> --every-secs <secs> <prompt>"
        );
    }

    let Some(target_session_id) = target_session_id else {
        anyhow::bail!("missing `--to <master|session-id>`");
    };
    let Some(interval_secs) = interval_secs else {
        anyhow::bail!("missing `--every-secs <secs>`");
    };

    Ok(SlashCommand::Automation(SlashAutomationCommand::Create(
        SlashAutomationCreateCommand {
            request: CreateSessionAutomationRequest {
                target_session_id,
                prompt,
                interval_secs,
                max_runs,
                run_for_secs,
            },
        },
    )))
}

fn parse_named_automation_command(args: &[String], action: &str) -> Result<SlashCommand> {
    let Some(automation_id) = args.first() else {
        anyhow::bail!("usage: /automation {action} <automation-id>");
    };
    let command = match action {
        "pause" => SlashAutomationCommand::Pause {
            automation_id: automation_id.clone(),
        },
        "resume" => SlashAutomationCommand::Resume {
            automation_id: automation_id.clone(),
        },
        "cancel" => SlashAutomationCommand::Cancel {
            automation_id: automation_id.clone(),
        },
        other => anyhow::bail!("unsupported automation action `{other}`"),
    };
    Ok(SlashCommand::Automation(command))
}

fn parse_send_command(args: &[String]) -> Result<SlashCommand> {
    if args.len() < 2 {
        anyhow::bail!("usage: /send <master|session-id> <prompt>");
    }
    Ok(SlashCommand::Send {
        target: args[0].clone(),
        prompt: args[1..].join(" "),
    })
}

fn parse_spawn_command(args: &[String]) -> Result<SlashCommand> {
    if args.is_empty() {
        anyhow::bail!("usage: /spawn backend: Payment API refactor");
    }
    let joined = args.join(" ");
    let (group, task) = parse_spawn_input(&joined)?;
    Ok(SlashCommand::Spawn { group, task })
}

fn parse_focus_command(args: &[String]) -> Result<SlashCommand> {
    let Some(session_id) = args.first() else {
        anyhow::bail!("usage: /focus <onboard|master|session-id>");
    };
    Ok(SlashCommand::Focus {
        session_id: session_id.clone(),
    })
}

fn parse_monitor_command(args: &[String]) -> Result<SlashCommand> {
    let command = match args.first().map(String::as_str) {
        None => SlashMonitorCommand::Overview,
        Some("sessions") => SlashMonitorCommand::Sessions,
        Some("runtime") => SlashMonitorCommand::Runtime,
        Some("jobs") => SlashMonitorCommand::Jobs,
        Some("session") => {
            let Some(session_id) = args.get(1) else {
                anyhow::bail!("usage: /monitor session <master|session-id>");
            };
            SlashMonitorCommand::Session {
                session_id: session_id.clone(),
            }
        }
        Some(other) => anyhow::bail!("unsupported monitor subcommand `{other}`"),
    };
    Ok(SlashCommand::Monitor(command))
}

fn slash_command_hint(
    buffer: &str,
    selected_id: Option<&str>,
    groups: &[String],
    selected_completion: Option<&InputCompletion>,
    completion_start: usize,
    completion_end: usize,
    completion_count: usize,
) -> Vec<String> {
    let trimmed = buffer.trim();
    let head = trimmed
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or_default();

    let guidance = match head {
        "" => (
            "Try `/job create \"Design CRM blueprint\" --start-group backend` or `/help`"
                .to_owned(),
            format!(
                "Also: `/automation create --to master --every-secs 300 \"Review blockers\"`  `/monitor sessions`  `/spawn {}: Payment API refactor`",
                groups.first().cloned().unwrap_or_else(|| "backend".to_owned())
            ),
        ),
        "job" => (
            "Usage: /job create <title> [--start-session ID | --start-group GROUP] [--defer]"
                .to_owned(),
            "Flags: --delegate-master-loop --continue-for-secs N --continue-max-iterations N --auto-approve --context \"...\""
                .to_owned(),
        ),
        "automation" => (
            "Usage: /automation create --to <master|session-id> --every-secs N [--max-runs N] [--for-secs N] <prompt>"
                .to_owned(),
            "Also: /automation list | /automation pause AUTO-001 | /automation resume AUTO-001"
                .to_owned(),
        ),
        "send" => (
            "Usage: /send <master|session-id> <prompt>".to_owned(),
            format!(
                "Current selection: {}",
                selected_id.unwrap_or("none")
            ),
        ),
        "spawn" => (
            "Usage: /spawn backend: Payment API refactor".to_owned(),
            format!("Available groups: {}", groups.join(", ")),
        ),
        "focus" => (
            "Usage: /focus <onboard|master|session-id>".to_owned(),
            format!(
                "Current selection: {}",
                selected_id.unwrap_or("none")
            ),
        ),
        "monitor" => (
            "Usage: /monitor [sessions|runtime|jobs|session <id>]".to_owned(),
            "Local monitor data only: no Codex round-trip for runtime/session visibility."
                .to_owned(),
        ),
        "help" => (
            "Commands: /job create, /automation, /send, /spawn, /focus, /monitor, /help"
                .to_owned(),
            "Examples: /automation list | /monitor sessions | /job create \"CRM\" --start-group backend"
                .to_owned(),
        ),
        other => (
            format!("Unknown prefix `/{other}`. Try `/help`."),
            "Supported: /job create, /automation, /send, /spawn, /focus, /monitor"
                .to_owned(),
        ),
    };

    let detail_line = if guidance.1.is_empty() {
        guidance.0
    } else {
        format!("{} | {}", guidance.0, guidance.1)
    };

    if let Some(selected_completion) = selected_completion {
        vec![
            format!(
                "selected: {} | Tab accept | Shift+Tab prev | Alt+Up/Down cycle | showing {}-{} of {}",
                selected_completion.label,
                completion_start + 1,
                completion_end,
                completion_count
            ),
            detail_line,
        ]
    } else {
        vec![
            "Tab completes commands, flags, groups, session ids, and automation ids. Ctrl+P/N recalls command history."
                .to_owned(),
            detail_line,
        ]
    }
}

fn worker_job_prompt(job: &crate::state::JobRecord, session_id: &str) -> String {
    let context = job.context.clone().unwrap_or_else(|| "none".to_owned());
    format!(
        "Job execution request for an existing CodeClaw session.\n\nJob id: {}\nWorker session: {}\nTitle: {}\nObjective: {}\nPriority: {}\nApproval required: {}\nContext: {}\n\nExecute the job from this session if it fits your current role and workspace. Keep the summary concise, report blockers clearly, and hand work back when a different session should continue.",
        job.id,
        session_id,
        job.title,
        job.objective,
        job.priority,
        if job.policy.approval_required { "yes" } else { "no" },
        context
    )
}

fn target_label(target: &PromptTarget) -> String {
    match target {
        PromptTarget::Master => "master".to_owned(),
        PromptTarget::Worker(worker_id) => worker_id.clone(),
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

fn automation_status_color(status: &str) -> Color {
    match status {
        "armed" => Color::Rgb(124, 218, 146),
        "paused" => Color::Rgb(255, 195, 120),
        "failed" => Color::Rgb(255, 133, 133),
        "completed" => Color::Rgb(96, 165, 250),
        "cancelled" => Color::Rgb(148, 163, 184),
        _ => Color::Gray,
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

fn session_accent_color_for_kind(kind: &SessionKind) -> Color {
    match kind {
        SessionKind::Onboard => Color::Rgb(201, 216, 117),
        SessionKind::Master => Color::Rgb(232, 190, 92),
        SessionKind::Worker { group, .. } => match group.as_str() {
            "backend" => Color::Rgb(96, 165, 250),
            "frontend" => Color::Rgb(72, 187, 158),
            "infra" => Color::Rgb(244, 162, 97),
            _ => Color::Rgb(167, 139, 250),
        },
    }
}

fn blocked_and_failed(onboard: &OnboardSnapshot) -> Vec<OnboardLaneItem> {
    onboard
        .blocked
        .iter()
        .chain(onboard.failed.iter())
        .cloned()
        .collect()
}

fn onboard_lane_text(
    _title: &str,
    items: &[OnboardLaneItem],
    width: usize,
    max_lines: usize,
) -> Text<'static> {
    if max_lines == 0 {
        return Text::default();
    }
    if items.is_empty() {
        return Text::from(Line::from("No jobs"));
    }

    let body_width = width.saturating_sub(2).max(16);
    let take = items.len().min(max_lines);
    let start = items.len().saturating_sub(take);
    let lines = items[start..]
        .iter()
        .flat_map(|item| {
            let mut badges = Vec::new();
            if item.automation.auto_approve {
                badges.push("AUTO".to_owned());
            }
            if item.automation.delegate_to_master_loop {
                badges.push("LOOP".to_owned());
            }
            if let Some(remaining) = item.automation.remaining_iterations {
                badges.push(format!("n={remaining}"));
            }
            if let Some(remaining) = item.automation.remaining_secs {
                badges.push(format!("t={}", onboard_duration_compact(remaining)));
            }
            let badge_text = if badges.is_empty() {
                "-".to_owned()
            } else {
                badges.join(" ")
            };
            let detail_text = format!(
                "{} | {} | {}",
                item.operator_state, badge_text, item.summary
            );
            vec![
                Line::from(vec![
                    Span::styled(
                        truncate(&item.job_id, 10),
                        Style::default()
                            .fg(status_color(&item.status, 0))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        truncate(&item.title, body_width.saturating_sub(12)),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        truncate(&detail_text, body_width),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("w{}", item.workers),
                        Style::default().fg(Color::Gray),
                    ),
                ]),
            ]
        })
        .take(max_lines)
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn monitor_sessions_text(
    sessions: &[MonitorSessionSnapshot],
    width: usize,
    max_lines: usize,
) -> Text<'static> {
    if max_lines == 0 {
        return Text::default();
    }
    if sessions.is_empty() {
        return Text::from(Line::from("No codex sessions"));
    }

    let body_width = width.max(16);
    let take = (max_lines / 2).max(1);
    let visible = sessions.len().min(take);
    let start = sessions.len().saturating_sub(visible);
    let mut lines = Vec::new();
    for session in &sessions[start..] {
        let mut badges = vec![session.work_state.clone(), session.status.clone()];
        if session.pending_turns > 0 {
            badges.push(format!("q{}", session.pending_turns));
        }
        if let Some(group) = &session.group {
            badges.push(group.clone());
        }
        if let Some(job_id) = &session.job_id {
            badges.push(job_id.clone());
        }
        if let Some(batch_id) = session.latest_batch_id {
            badges.push(format!("b{batch_id}"));
        }
        let header = format!(
            "{} [{}] {}",
            session.id,
            badges.join(" | "),
            truncate(&session.title, body_width.saturating_sub(10))
        );
        let prompt = session
            .latest_user_prompt
            .as_deref()
            .or(session.task.as_deref())
            .unwrap_or("no user prompt yet");
        let response = session
            .latest_response
            .as_deref()
            .or(session.summary.as_deref())
            .unwrap_or("-");
        let preview = format!(
            "usr> {} | rsp> {}",
            compact_inline(prompt),
            compact_inline(response)
        );

        lines.push(Line::from(vec![
            Span::styled(
                truncate(&header, body_width),
                Style::default()
                    .fg(status_color(&session.status, 0))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                truncate(&session.role, 12),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            truncate(&preview, body_width),
            Style::default().fg(Color::Gray),
        )));
    }

    if visible < sessions.len() && lines.len() < max_lines {
        lines.push(Line::from(Span::styled(
            format!("+{} more session(s)", sessions.len() - visible),
            Style::default().fg(Color::DarkGray),
        )));
    }

    Text::from(lines.into_iter().take(max_lines).collect::<Vec<_>>())
}

fn session_automations_text(
    automations: &[SessionAutomationSnapshot],
    width: usize,
    max_lines: usize,
) -> Text<'static> {
    if max_lines == 0 {
        return Text::default();
    }
    if automations.is_empty() {
        return Text::from(Line::from("No automations"));
    }

    let body_width = width.max(18);
    let take = (max_lines / 2).max(1);
    let visible = automations.len().min(take);
    let start = automations.len().saturating_sub(visible);
    let mut lines = Vec::new();
    for automation in &automations[start..] {
        let mut badges = vec![
            format!("{}>{}", automation.id, automation.target_session_id),
            automation.status.clone(),
            format!("every {}s", automation.interval_secs),
        ];
        if let Some(max_runs) = automation.max_runs {
            badges.push(format!("max={max_runs}"));
        }
        if let Some(run_for_secs) = automation.run_for_secs {
            badges.push(format!("window={}", onboard_duration_compact(run_for_secs)));
        }
        if let Some(remaining_runs) = automation.remaining_runs {
            badges.push(format!("n={remaining_runs}"));
        }
        if let Some(remaining_secs) = automation.remaining_secs {
            badges.push(format!("t={}", onboard_duration_compact(remaining_secs)));
        }

        let header = badges.join(" | ");
        let detail = if let Some(error) = &automation.last_error {
            format!("err> {}", compact_inline(error))
        } else {
            let prompt = compact_inline(&automation.prompt_preview);
            let batch = automation
                .last_batch_id
                .map(|batch_id| format!("b{batch_id}"))
                .unwrap_or_else(|| "-".to_owned());
            let last_run = automation
                .last_run_at
                .map(|last_run_at| last_run_at.to_string())
                .unwrap_or_else(|| "-".to_owned());
            format!(
                "run {} | last {} | batch {} | {}",
                automation.run_count, last_run, batch, prompt
            )
        };

        lines.push(Line::from(Span::styled(
            truncate(&header, body_width),
            Style::default()
                .fg(automation_status_color(&automation.status))
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            truncate(&detail, body_width),
            Style::default().fg(Color::Gray),
        )));
    }

    Text::from(lines.into_iter().take(max_lines).collect::<Vec<_>>())
}

fn onboard_duration_compact(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn session_title_style(accent: Color) -> Style {
    Style::default().fg(accent).add_modifier(Modifier::BOLD)
}

fn session_kind_badge_for_kind(kind: &SessionKind) -> &'static str {
    match kind {
        SessionKind::Onboard => "BRD",
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

fn input_border_style(
    mode: &InputMode,
    selected: Option<&SessionOverviewSnapshot>,
    tick: u64,
) -> Style {
    match mode {
        InputMode::Normal => selected
            .map(|session| Style::default().fg(session_accent_color_for_kind(&session.kind)))
            .unwrap_or_else(|| Style::default().fg(Color::Gray)),
        InputMode::MasterPrompt | InputMode::WorkerPrompt(_) => {
            Style::default().fg(status_color("running", tick))
        }
        InputMode::SpawnWorker => Style::default().fg(status_color("queued", tick)),
        InputMode::SlashCommand => Style::default().fg(Color::Rgb(201, 216, 117)),
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

fn input_target_label(mode: &InputMode, session: &SessionOverviewSnapshot) -> String {
    match mode {
        InputMode::MasterPrompt => "master".to_owned(),
        InputMode::WorkerPrompt(worker_id) => worker_id.clone(),
        InputMode::SpawnWorker => "spawn".to_owned(),
        InputMode::SlashCommand => "slash".to_owned(),
        InputMode::Normal => match &session.kind {
            SessionKind::Onboard => "master".to_owned(),
            SessionKind::Master => "master".to_owned(),
            SessionKind::Worker { .. } => "master".to_owned(),
        },
    }
}

fn session_list_subtitle(session: &SessionOverviewSnapshot) -> String {
    if session.pending_turns == 0 {
        session.subtitle.clone()
    } else {
        format!("q{} | {}", session.pending_turns, session.subtitle)
    }
}

fn session_identity_line(session: &SessionSnapshot) -> String {
    match &session.kind {
        SessionKind::Onboard => "role: onboard supervisor".to_owned(),
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

fn session_latest_user_line(session: &SessionSnapshot) -> String {
    format!(
        "user: {}",
        truncate(
            &session
                .latest_user_prompt()
                .unwrap_or_else(|| "-".to_owned()),
            47,
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
        SessionKind::Onboard => format!("control root: {}", truncate(&session.cwd, 39)),
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

#[cfg(test)]
mod tests {
    use super::{
        completion_window, editor_rows, input_completion_line_count, input_help_line_count,
        input_view_state, parse_slash_command, parse_spawn_input, slash_input_completions,
        spawn_input_completions, InputMode, SlashAutomationCommand, SlashCommand,
        SlashJobCreateCommand, SlashJobStartTarget, SlashMonitorCommand,
    };

    #[test]
    fn slash_job_create_parses_start_group_and_loop_options() {
        let command = parse_slash_command(
            "/job create \"Design CRM blueprint\" --start-group backend --delegate-master-loop --continue-for-secs 3600 --continue-max-iterations 10 --auto-approve",
        )
        .expect("slash command should parse");

        let SlashCommand::JobCreate(SlashJobCreateCommand {
            request,
            defer,
            start_target,
        }) = command
        else {
            panic!("expected job create command");
        };

        assert_eq!(request.title, "Design CRM blueprint");
        assert_eq!(request.objective, "Design CRM blueprint");
        assert!(request.auto_approve);
        assert!(request.delegate_to_master_loop);
        assert_eq!(request.continue_for_secs, Some(3600));
        assert_eq!(request.continue_max_iterations, Some(10));
        assert!(!defer);
        match start_target {
            SlashJobStartTarget::NewWorkerGroup(group) => assert_eq!(group, "backend"),
            other => panic!("unexpected start target: {other:?}"),
        }
    }

    #[test]
    fn slash_send_parses_quoted_prompt() {
        let command =
            parse_slash_command("/send master \"Plan the next API step\"").expect("parse send");

        match command {
            SlashCommand::Send { target, prompt } => {
                assert_eq!(target, "master");
                assert_eq!(prompt, "Plan the next API step");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn slash_monitor_parses_session_target() {
        let command =
            parse_slash_command("/monitor session backend-001").expect("parse monitor session");

        match command {
            SlashCommand::Monitor(SlashMonitorCommand::Session { session_id }) => {
                assert_eq!(session_id, "backend-001");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn slash_automation_create_parses_repeat_budget() {
        let command = parse_slash_command(
            "/automation create --to master --every-secs 300 --max-runs 10 --for-secs 3600 \"Review blocked jobs and continue\"",
        )
        .expect("parse automation create");

        match command {
            SlashCommand::Automation(SlashAutomationCommand::Create(command)) => {
                assert_eq!(command.request.target_session_id, "master");
                assert_eq!(command.request.interval_secs, 300);
                assert_eq!(command.request.max_runs, Some(10));
                assert_eq!(command.request.run_for_secs, Some(3600));
                assert_eq!(command.request.prompt, "Review blocked jobs and continue");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn spawn_input_requires_group_and_task() {
        assert!(parse_spawn_input("backend: Refactor API").is_ok());
        assert!(parse_spawn_input("backend").is_err());
    }

    #[test]
    fn editor_rows_wrap_long_lines() {
        let rows = editor_rows("abcdef", 3);
        let texts = rows.into_iter().map(|row| row.text).collect::<Vec<_>>();
        assert_eq!(texts, vec!["abc".to_owned(), "def".to_owned()]);
    }

    #[test]
    fn input_view_keeps_cursor_visible_in_wrapped_content() {
        let view = input_view_state("abcdef", 6, 3, 1);
        assert_eq!(view.body, "def");
        assert_eq!(view.cursor_x, 3);
        assert_eq!(view.cursor_y, 0);
    }

    #[test]
    fn input_view_uses_display_width_for_cjk_text() {
        let view = input_view_state("你好a", "你好a".len(), 6, 1);
        assert_eq!(view.body, "你好a");
        assert_eq!(view.cursor_x, 5);
        assert_eq!(view.cursor_y, 0);

        let wrapped = input_view_state("你好a", "你好a".len(), 3, 2);
        assert_eq!(wrapped.body, "你\n好a");
        assert_eq!(wrapped.cursor_x, 3);
        assert_eq!(wrapped.cursor_y, 1);
    }

    #[test]
    fn slash_completion_suggests_commands_and_targets() {
        let root = slash_input_completions(
            "/se",
            3,
            &["backend".to_owned()],
            &["master".to_owned()],
            &[],
        );
        assert_eq!(root[0].replacement, "/send ");

        let send = slash_input_completions(
            "/send ba",
            8,
            &["backend".to_owned()],
            &["backend-001".to_owned(), "master".to_owned()],
            &[],
        );
        assert_eq!(send[0].replacement, "backend-001 ");

        let monitor = slash_input_completions(
            "/monitor se",
            11,
            &["backend".to_owned()],
            &["backend-001".to_owned(), "master".to_owned()],
            &[],
        );
        assert_eq!(monitor[0].replacement, "session ");

        let automation = slash_input_completions(
            "/automation re",
            14,
            &["backend".to_owned()],
            &["backend-001".to_owned(), "master".to_owned()],
            &["AUTO-001".to_owned()],
        );
        assert_eq!(automation[0].replacement, "resume ");
    }

    #[test]
    fn slash_completion_supports_fuzzy_matching() {
        let command = slash_input_completions(
            "/fcs",
            4,
            &["backend".to_owned()],
            &["master".to_owned()],
            &[],
        );
        assert_eq!(command[0].replacement, "/focus ");

        let send = slash_input_completions(
            "/send bk1",
            9,
            &["backend".to_owned()],
            &["backend-001".to_owned(), "backend-ops".to_owned()],
            &[],
        );
        assert_eq!(send[0].replacement, "backend-001 ");
    }

    #[test]
    fn slash_completion_suggests_job_flags_and_groups() {
        let flags = slash_input_completions(
            "/job create CRM --sta",
            21,
            &["backend".to_owned()],
            &["backend-001".to_owned()],
            &[],
        );
        assert_eq!(flags[0].replacement, "--start-group ");

        let groups = slash_input_completions(
            "/job create CRM --start-group ba",
            32,
            &["backend".to_owned(), "frontend".to_owned()],
            &["backend-001".to_owned()],
            &[],
        );
        assert_eq!(groups[0].replacement, "backend");
    }

    #[test]
    fn spawn_completion_suggests_group_prefix() {
        let groups =
            spawn_input_completions("ba", 2, &["backend".to_owned(), "frontend".to_owned()]);
        assert_eq!(groups[0].replacement, "backend: ");
    }

    #[test]
    fn assist_layout_counts_help_and_completion_lines() {
        assert_eq!(input_help_line_count(&InputMode::SlashCommand), 2);
        assert_eq!(input_help_line_count(&InputMode::SpawnWorker), 1);
        assert_eq!(input_help_line_count(&InputMode::MasterPrompt), 1);
        assert_eq!(input_help_line_count(&InputMode::Normal), 0);

        assert_eq!(input_completion_line_count(&InputMode::SlashCommand, 0), 0);
        assert_eq!(input_completion_line_count(&InputMode::SlashCommand, 4), 3);
        assert_eq!(input_completion_line_count(&InputMode::SpawnWorker, 2), 2);
        assert_eq!(input_completion_line_count(&InputMode::MasterPrompt, 4), 0);
    }

    #[test]
    fn completion_window_tracks_selected_tail() {
        assert_eq!(completion_window(0, 0, 3), (0, 0));
        assert_eq!(completion_window(2, 0, 3), (0, 2));
        assert_eq!(completion_window(5, 0, 3), (0, 3));
        assert_eq!(completion_window(5, 2, 3), (0, 3));
        assert_eq!(completion_window(5, 4, 3), (2, 5));
    }
}
