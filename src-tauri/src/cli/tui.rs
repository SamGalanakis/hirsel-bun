//! Terminal UI for worker attach command.
//!
//! Uses ratatui to display a live view of worker events similar to the GUI.

use crate::core::state::{SQLiteState, ToolCallStatus, WorkerEvent, WorkerEventType, WorkerStatus};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use std::io::stdout;
use std::time::{Duration, Instant};

/// Styled message segment for rendering
#[derive(Clone)]
enum MessageSegment {
    Text(String),
    Thinking(String),
    ToolCall {
        id: String,
        title: String,
        kind: Option<String>,
        status: ToolCallStatus,
    },
}

/// TUI App state
pub struct AttachTui {
    run_name: String,
    worker_name: String,
    state: SQLiteState,
    messages: Vec<MessageSegment>,
    last_event_id: Option<i64>,
    scroll_offset: u16,
    max_scroll: u16,
    show_thinking: bool,
    worker_status: Option<WorkerStatus>,
    should_quit: bool,
    auto_scroll: bool,
    // Current streaming state
    current_text: String,
    current_thinking: String,
}

impl AttachTui {
    pub fn new(run_name: String, worker_name: String, state: SQLiteState) -> Self {
        Self {
            run_name,
            worker_name,
            state,
            messages: Vec::new(),
            last_event_id: None,
            scroll_offset: 0,
            max_scroll: 0,
            show_thinking: true,
            worker_status: None,
            should_quit: false,
            auto_scroll: true,
            current_text: String::new(),
            current_thinking: String::new(),
        }
    }

    /// Run the TUI main loop
    pub fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        // Initial load
        self.load_events()?;
        self.update_worker_status();

        let tick_rate = Duration::from_millis(100);
        let mut last_tick = Instant::now();

        // Main loop
        while !self.should_quit {
            // Draw
            terminal.draw(|frame| self.render(frame))?;

            // Handle events with timeout
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code);
                    }
                }
            }

            // Poll for new events on tick
            if last_tick.elapsed() >= tick_rate {
                self.load_events()?;
                self.update_worker_status();
                last_tick = Instant::now();
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;

        Ok(())
    }

    fn handle_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('t') => self.show_thinking = !self.show_thinking,
            KeyCode::Char('a') => self.auto_scroll = !self.auto_scroll,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                self.auto_scroll = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset = self.scroll_offset.saturating_add(1).min(self.max_scroll);
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                self.auto_scroll = false;
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(10).min(self.max_scroll);
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                self.auto_scroll = false;
            }
            KeyCode::End => {
                self.scroll_offset = self.max_scroll;
                self.auto_scroll = true;
            }
            _ => {}
        }
    }

    fn load_events(&mut self) -> anyhow::Result<()> {
        let events = self
            .state
            .get_worker_events(&self.worker_name, self.last_event_id, 500)
            .map_err(|e| anyhow::anyhow!("Failed to get events: {}", e))?;

        for event in events {
            self.last_event_id = Some(event.id);
            self.process_event(event);
        }

        Ok(())
    }

    fn process_event(&mut self, event: WorkerEvent) {
        match event.event_type {
            WorkerEventType::Text => {
                if let Some(content) = event.content {
                    self.current_text.push_str(&content);
                }
            }
            WorkerEventType::Thought => {
                if let Some(content) = event.content {
                    self.current_thinking.push_str(&content);
                }
            }
            WorkerEventType::ToolStart => {
                // Flush any pending text/thinking before tool
                self.flush_current_content();

                self.messages.push(MessageSegment::ToolCall {
                    id: event.tool_call_id.unwrap_or_default(),
                    title: event.tool_title.unwrap_or_else(|| "Unknown".to_string()),
                    kind: event.tool_kind,
                    status: ToolCallStatus::InProgress,
                });
            }
            WorkerEventType::ToolUpdate => {
                // Update existing tool call status
                if let Some(tool_id) = &event.tool_call_id {
                    for msg in self.messages.iter_mut().rev() {
                        if let MessageSegment::ToolCall { id, status, .. } = msg {
                            if id == tool_id {
                                *status = event.tool_status.unwrap_or(ToolCallStatus::Completed);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    fn flush_current_content(&mut self) {
        if !self.current_thinking.is_empty() {
            self.messages.push(MessageSegment::Thinking(std::mem::take(
                &mut self.current_thinking,
            )));
        }
        if !self.current_text.is_empty() {
            self.messages
                .push(MessageSegment::Text(std::mem::take(&mut self.current_text)));
        }
    }

    fn update_worker_status(&mut self) {
        if let Ok(Some(worker)) = self.state.get_worker(&self.worker_name) {
            self.worker_status = Some(worker.status);
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Layout: header, main content, footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(1),    // Content
                Constraint::Length(2), // Footer
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let status_str = match self.worker_status {
            Some(WorkerStatus::Working) => "● Working",
            Some(WorkerStatus::Awaiting) => "◑ Awaiting",
            Some(WorkerStatus::Paused) => "⏸ Paused",
            Some(WorkerStatus::Error) => "✗ Error",
            None => "? Unknown",
        };

        let status_color = match self.worker_status {
            Some(WorkerStatus::Working) => Color::Yellow,
            Some(WorkerStatus::Awaiting) => Color::Cyan,
            Some(WorkerStatus::Paused) => Color::Gray,
            Some(WorkerStatus::Error) => Color::Red,
            None => Color::DarkGray,
        };

        let header_text = Line::from(vec![
            Span::styled(&self.worker_name, Style::default().fg(Color::White).bold()),
            Span::raw(" │ "),
            Span::styled(&self.run_name, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(status_str, Style::default().fg(status_color)),
        ]);

        let header = Paragraph::new(header_text).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        frame.render_widget(header, area);
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect) {
        // Flush current content for display
        let display_thinking = self.current_thinking.clone();
        let display_text = self.current_text.clone();

        // Build styled text
        let mut lines: Vec<Line> = Vec::new();

        for segment in &self.messages {
            match segment {
                MessageSegment::Text(text) => {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(
                            line,
                            Style::default().fg(Color::White),
                        )));
                    }
                    if !text.ends_with('\n') && !text.is_empty() {
                        // Continue on same line if no newline
                    } else {
                        lines.push(Line::from(""));
                    }
                }
                MessageSegment::Thinking(text) if self.show_thinking => {
                    lines.push(Line::from(Span::styled(
                        "💭 Thinking...",
                        Style::default().fg(Color::Magenta).italic(),
                    )));
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(
                            format!("   {}", line),
                            Style::default().fg(Color::DarkGray).italic(),
                        )));
                    }
                    lines.push(Line::from(""));
                }
                MessageSegment::Thinking(_) => {}
                MessageSegment::ToolCall {
                    title,
                    kind,
                    status,
                    ..
                } => {
                    let (icon, color) = match status {
                        ToolCallStatus::Pending => ("○", Color::DarkGray),
                        ToolCallStatus::InProgress => ("◐", Color::Yellow),
                        ToolCallStatus::Completed => ("✓", Color::Green),
                        ToolCallStatus::Failed => ("✗", Color::Red),
                    };

                    let kind_icon = match kind.as_deref() {
                        Some("read") => "📖",
                        Some("edit") | Some("write") => "✏️",
                        Some("execute") | Some("bash") => "⚡",
                        Some("search") | Some("grep") | Some("glob") => "🔍",
                        Some("web") => "🌐",
                        _ => "🔧",
                    };

                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", kind_icon), Style::default()),
                        Span::styled(title, Style::default().fg(Color::Cyan)),
                        Span::raw("  "),
                        Span::styled(icon, Style::default().fg(color)),
                    ]));
                }
            }
        }

        // Add currently streaming content
        if self.show_thinking && !display_thinking.is_empty() {
            lines.push(Line::from(Span::styled(
                "💭 Thinking...",
                Style::default().fg(Color::Magenta).italic(),
            )));
            for line in display_thinking.lines() {
                lines.push(Line::from(Span::styled(
                    format!("   {}", line),
                    Style::default().fg(Color::DarkGray).italic(),
                )));
            }
        }

        if !display_text.is_empty() {
            for line in display_text.lines() {
                lines.push(Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::White),
                )));
            }
            // Show cursor if streaming
            if matches!(self.worker_status, Some(WorkerStatus::Working)) {
                if let Some(last) = lines.last_mut() {
                    last.spans
                        .push(Span::styled("▋", Style::default().fg(Color::Yellow)));
                }
            }
        }

        // Calculate scroll
        let content_height = lines.len() as u16;
        let view_height = area.height.saturating_sub(2);
        self.max_scroll = content_height.saturating_sub(view_height);

        if self.auto_scroll {
            self.scroll_offset = self.max_scroll;
        }

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0))
            .block(Block::default().borders(Borders::NONE));

        frame.render_widget(paragraph, area);

        // Scrollbar
        if self.max_scroll > 0 {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            let mut scrollbar_state =
                ScrollbarState::new(self.max_scroll as usize).position(self.scroll_offset as usize);

            frame.render_stateful_widget(
                scrollbar,
                area.inner(Margin {
                    vertical: 0,
                    horizontal: 0,
                }),
                &mut scrollbar_state,
            );
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let thinking_status = if self.show_thinking { "on" } else { "off" };
        let auto_scroll_status = if self.auto_scroll { "on" } else { "off" };

        let footer = Line::from(vec![
            Span::styled(" q", Style::default().fg(Color::Yellow)),
            Span::raw(" quit  "),
            Span::styled("t", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" thinking:{thinking_status}  ")),
            Span::styled("a", Style::default().fg(Color::Yellow)),
            Span::raw(format!(" autoscroll:{auto_scroll_status}  ")),
            Span::styled("↑↓", Style::default().fg(Color::Yellow)),
            Span::raw(" scroll"),
        ]);

        let footer_widget = Paragraph::new(footer).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        frame.render_widget(footer_widget, area);
    }
}
