use std::{io, path::Path, sync::Arc};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    Viewport, crossterm,
    layout::{Constraint, Layout, Margin, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, List, ListItem, Paragraph, Wrap},
};
use tokio_stream::StreamExt;
use tracing::debug;

use crate::{
    agent::{
        AssistantTurn,
        chat::{ChatAgent, ChatStream, ChatStreamEvent},
    },
    config::{AppConfig, profile::Profiles},
    error::Result,
    logging::{self, UiLogEntry},
    session::{ChatSession, SessionStore, TranscriptEntry, TranscriptRole},
    shell::Shell,
    tui::{
        cmd::{copy_command, execute_command, print_command_to_input_buffer},
        editor::EditorState,
        logs::render_compact_log_lines,
        markdown::render_markdown,
        terminal::InlineTerminal,
    },
};

const HISTORY_ITEM_HEIGHT: u16 = 4;
const COMMAND_ITEM_HEIGHT: u16 = 4;
const LOG_LIMIT: usize = 128;
const OUTER_BORDER_STYLE: Style = Style::new().fg(Color::Blue);
const PANEL_BORDER_STYLE: Style = Style::new().fg(Color::DarkGray);
const ACTIVE_BORDER_STYLE: Style = Style::new().fg(Color::LightBlue);
const TITLE_STYLE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
const USER_STYLE: Style = Style::new()
    .fg(Color::LightGreen)
    .add_modifier(Modifier::BOLD);
const ASSISTANT_STYLE: Style = Style::new()
    .fg(Color::LightBlue)
    .add_modifier(Modifier::BOLD);
const CARD_STYLE: Style = Style::new().fg(Color::Yellow);
const MUTED_STYLE: Style = Style::new().fg(Color::DarkGray);
const STATUS_STYLE: Style = Style::new().fg(Color::Gray);

#[derive(Debug, Clone)]
struct CommandCard {
    command: String,
    summary: String,
}

#[derive(Debug, Clone)]
struct ConversationEntry {
    role: TranscriptRole,
    markdown: String,
    commands: Vec<CommandCard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Timeline,
    Spotlight,
    Commands,
    Composer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LayoutMode {
    Tiny,
    Wide,
    Compact,
    #[default]
    Narrow,
}

#[derive(Debug, Clone, Copy, Default)]
struct LayoutCache {
    mode: LayoutMode,
    header: Rect,
    timeline: Rect,
    timeline_body: Rect,
    spotlight: Rect,
    spotlight_body: Rect,
    commands: Rect,
    commands_body: Rect,
    logs: Rect,
    logs_body: Rect,
    composer: Rect,
    footer: Rect,
}

#[derive(Debug, Clone)]
struct ChatWidget {
    session_id: String,
    history: Vec<ConversationEntry>,
    focus: Focus,
    input: EditorState,
    selected_history: usize,
    selected_command: Option<usize>,
    status: String,
    log_entries: Vec<UiLogEntry>,
    history_offset: usize,
    command_offset: usize,
    detail_scroll: u16,
    log_scroll: u16,
    pin_logs_to_tail: bool,
    streaming: bool,
    layout: LayoutCache,
}

struct App {
    terminal: InlineTerminal,
    widget: ChatWidget,
    agent: Arc<ChatAgent>,
    session: ChatSession,
    session_store: SessionStore,
    shell_path: std::path::PathBuf,
    attached: Option<String>,
    pending_stream: Option<ChatStream>,
    streaming_entry: Option<usize>,
}

#[derive(Debug)]
enum AppEvent {
    Up,
    Down,
    Left,
    Right,
    Send,
    Newline,
    NextFocus,
    PrevFocus,
    SoftQuit,
    HardQuit,
    RawKey(KeyEvent),
    Mouse(MouseEvent),
    Resize,
    Stream(ChatStreamEvent),
    StreamError(String),
    StreamClosed,
    Err(io::Error),
}

fn detect_os() -> String {
    sysinfo::System::name().unwrap_or(std::env::consts::OS.to_string())
}

fn map_key_event(key: KeyEvent) -> AppEvent {
    match key.code {
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => AppEvent::HardQuit,
        KeyCode::Char('j') if key.modifiers == KeyModifiers::CONTROL => AppEvent::Newline,
        KeyCode::Enter if key.modifiers.is_empty() => AppEvent::Send,
        KeyCode::Tab if key.modifiers.is_empty() => AppEvent::NextFocus,
        KeyCode::BackTab if key.modifiers.is_empty() => AppEvent::PrevFocus,
        KeyCode::Up if key.modifiers.is_empty() => AppEvent::Up,
        KeyCode::Down if key.modifiers.is_empty() => AppEvent::Down,
        KeyCode::Left if key.modifiers.is_empty() => AppEvent::Left,
        KeyCode::Right if key.modifiers.is_empty() => AppEvent::Right,
        KeyCode::Esc if key.modifiers.is_empty() => AppEvent::SoftQuit,
        _ => AppEvent::RawKey(key),
    }
}

fn inset(rect: Rect, horizontal: u16, vertical: u16) -> Rect {
    rect.inner(Margin {
        horizontal,
        vertical,
    })
}

fn point_in_rect(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && row >= rect.y
        && column < rect.x.saturating_add(rect.width)
        && row < rect.y.saturating_add(rect.height)
}

fn truncate_line(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut collected = String::new();
    for (count, ch) in value.chars().enumerate() {
        if count + 1 >= max_chars {
            collected.push('…');
            return collected;
        }
        collected.push(ch);
    }
    collected
}

fn command_preview(command: &str, max_chars: usize) -> String {
    let first_line = command.lines().next().unwrap_or("");
    let mut preview = truncate_line(first_line, max_chars);
    let extra_lines = command.lines().skip(1).count();
    if extra_lines > 0 {
        if !preview.is_empty() {
            preview.push(' ');
        }
        preview.push_str(&format!("(+{extra_lines} lines)"));
    }
    preview
}

fn command_text_lines(command: &str, style: Style) -> Vec<Line<'static>> {
    let lines = command
        .lines()
        .map(|line| Line::from(line.to_string()).style(style))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec![Line::from("").style(style)]
    } else {
        lines
    }
}

impl From<crate::agent::tools::CommandCandidate> for CommandCard {
    fn from(value: crate::agent::tools::CommandCandidate) -> Self {
        Self {
            command: value.command,
            summary: value.summary,
        }
    }
}

impl From<CommandCard> for crate::agent::tools::CommandCandidate {
    fn from(value: CommandCard) -> Self {
        Self {
            command: value.command,
            summary: value.summary,
        }
    }
}

impl From<&TranscriptEntry> for ConversationEntry {
    fn from(value: &TranscriptEntry) -> Self {
        Self {
            role: value.role,
            markdown: value.markdown.clone(),
            commands: value
                .commands
                .iter()
                .cloned()
                .map(CommandCard::from)
                .collect(),
        }
    }
}

impl ConversationEntry {
    fn to_transcript_entry(&self) -> TranscriptEntry {
        TranscriptEntry {
            role: self.role,
            markdown: self.markdown.clone(),
            commands: self.commands.iter().cloned().map(Into::into).collect(),
        }
    }

    fn title(&self) -> &'static str {
        match self.role {
            TranscriptRole::User => "User",
            TranscriptRole::Assistant => "Assistant",
        }
    }

    fn title_style(&self) -> Style {
        match self.role {
            TranscriptRole::User => USER_STYLE,
            TranscriptRole::Assistant => ASSISTANT_STYLE,
        }
    }

    fn preview_lines(&self) -> (String, String) {
        let mut lines = self
            .markdown
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let first = lines.next().unwrap_or(match self.role {
            TranscriptRole::User => "Awaiting prompt",
            TranscriptRole::Assistant => "Waiting for stream",
        });
        let second = lines.next().unwrap_or(if self.commands.is_empty() {
            ""
        } else {
            "Command cards available"
        });
        (first.to_string(), second.to_string())
    }
}

impl Focus {
    fn label(self) -> &'static str {
        match self {
            Focus::Timeline => "Timeline",
            Focus::Spotlight => "Spotlight",
            Focus::Commands => "Commands",
            Focus::Composer => "Composer",
        }
    }

    fn next(self) -> Self {
        match self {
            Focus::Timeline => Focus::Spotlight,
            Focus::Spotlight => Focus::Commands,
            Focus::Commands => Focus::Composer,
            Focus::Composer => Focus::Timeline,
        }
    }

    fn previous(self) -> Self {
        match self {
            Focus::Timeline => Focus::Composer,
            Focus::Spotlight => Focus::Timeline,
            Focus::Commands => Focus::Spotlight,
            Focus::Composer => Focus::Commands,
        }
    }
}

impl LayoutMode {
    fn is_tiny(self) -> bool {
        matches!(self, Self::Tiny)
    }

    fn shows_timeline(self) -> bool {
        !self.is_tiny()
    }

    fn shows_command_dock(self) -> bool {
        !self.is_tiny()
    }

    fn shows_logs(self) -> bool {
        !self.is_tiny()
    }

    fn spotlight_title(self) -> &'static str {
        if self.is_tiny() {
            "Output"
        } else {
            "Spotlight"
        }
    }
}

impl LayoutCache {
    fn build(area: Rect) -> Self {
        let outer = inset(area, 1, 1);
        if outer.width < 92 || outer.height < 22 {
            let [spotlight, composer] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(5)]).areas(outer);
            let empty = Rect::new(0, 0, 0, 0);
            return Self {
                mode: LayoutMode::Tiny,
                header: empty,
                timeline: empty,
                timeline_body: empty,
                spotlight,
                spotlight_body: inset(spotlight, 1, 1),
                commands: empty,
                commands_body: empty,
                logs: empty,
                logs_body: empty,
                composer,
                footer: empty,
            };
        }

        let [header, body, composer_row, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(6),
            Constraint::Length(1),
        ])
        .areas(outer);

        let composer_width = composer_row.width.saturating_sub(4).clamp(28, 96);
        let [_, composer, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(composer_width),
            Constraint::Fill(1),
        ])
        .areas(composer_row);

        if body.width >= 138 {
            let [timeline, spotlight, dock] = Layout::horizontal([
                Constraint::Length(30),
                Constraint::Min(40),
                Constraint::Length(38),
            ])
            .areas(body);
            let [commands, logs] =
                Layout::vertical([Constraint::Length(14), Constraint::Min(8)]).areas(dock);
            return Self {
                mode: LayoutMode::Wide,
                header,
                timeline,
                timeline_body: inset(timeline, 1, 1),
                spotlight,
                spotlight_body: inset(spotlight, 1, 1),
                commands,
                commands_body: inset(commands, 1, 1),
                logs,
                logs_body: inset(logs, 1, 1),
                composer,
                footer,
            };
        }

        if body.width >= 96 {
            let [timeline, stage] =
                Layout::horizontal([Constraint::Length(28), Constraint::Min(40)]).areas(body);
            let [spotlight, utility] =
                Layout::vertical([Constraint::Min(14), Constraint::Length(14)]).areas(stage);
            let [commands, logs] =
                Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
                    .areas(utility);
            return Self {
                mode: LayoutMode::Compact,
                header,
                timeline,
                timeline_body: inset(timeline, 1, 1),
                spotlight,
                spotlight_body: inset(spotlight, 1, 1),
                commands,
                commands_body: inset(commands, 1, 1),
                logs,
                logs_body: inset(logs, 1, 1),
                composer,
                footer,
            };
        }

        let [spotlight, lower] =
            Layout::vertical([Constraint::Min(10), Constraint::Length(18)]).areas(body);
        let [timeline, utility] =
            Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)])
                .areas(lower);
        let [commands, logs] =
            Layout::vertical([Constraint::Length(8), Constraint::Min(8)]).areas(utility);
        Self {
            mode: LayoutMode::Narrow,
            header,
            timeline,
            timeline_body: inset(timeline, 1, 1),
            spotlight,
            spotlight_body: inset(spotlight, 1, 1),
            commands,
            commands_body: inset(commands, 1, 1),
            logs,
            logs_body: inset(logs, 1, 1),
            composer,
            footer,
        }
    }
}

impl ChatWidget {
    fn from_session(session: &ChatSession, initial_prompt: Option<String>) -> Self {
        let history = session
            .transcript
            .iter()
            .map(ConversationEntry::from)
            .collect::<Vec<_>>();
        let mut widget = Self {
            session_id: session.id.clone(),
            history,
            focus: Focus::Composer,
            input: EditorState::default(),
            selected_history: 0,
            selected_command: None,
            status: "Enter to send. Tab to move focus. Esc or Ctrl-C to quit.".to_string(),
            log_entries: Vec::new(),
            history_offset: 0,
            command_offset: 0,
            detail_scroll: 0,
            log_scroll: 0,
            pin_logs_to_tail: true,
            streaming: false,
            layout: LayoutCache::default(),
        };
        if let Some(prompt) = initial_prompt {
            widget.input = EditorState::from_text(&prompt);
        }
        widget.repair_selection();
        widget
    }

    fn repair_selection(&mut self) {
        if self.history.is_empty() {
            self.selected_history = 0;
            self.selected_command = None;
            self.history_offset = 0;
            self.command_offset = 0;
            self.detail_scroll = 0;
            return;
        }
        self.selected_history = self.selected_history.min(self.history.len() - 1);
        let commands = &self.history[self.selected_history].commands;
        self.selected_command = if commands.is_empty() {
            None
        } else {
            Some(self.selected_command.unwrap_or(0).min(commands.len() - 1))
        };
    }

    fn sync_layout(&mut self, area: Rect) {
        self.layout = LayoutCache::build(area);
        if self.layout.mode.is_tiny() && matches!(self.focus, Focus::Timeline | Focus::Commands) {
            self.focus = Focus::Spotlight;
        }
        self.ensure_history_visible();
        self.ensure_command_visible();
        self.clamp_detail_scroll();
        self.clamp_log_scroll();
    }

    fn refresh_logs(&mut self) {
        self.log_entries = logging::recent_logs(LOG_LIMIT);
        if self.pin_logs_to_tail {
            self.log_scroll = self.max_log_scroll();
        } else {
            self.clamp_log_scroll();
        }
    }

    fn history_slots(&self) -> usize {
        let slots = self.layout.timeline_body.height / HISTORY_ITEM_HEIGHT;
        slots.max(1) as usize
    }

    fn command_slots(&self) -> usize {
        let slots = self.layout.commands_body.height / COMMAND_ITEM_HEIGHT;
        slots.max(1) as usize
    }

    fn max_detail_scroll(&self) -> u16 {
        let content_lines = self.selected_detail_text().lines.len() as u16;
        content_lines.saturating_sub(self.layout.spotlight_body.height)
    }

    fn max_log_scroll(&self) -> u16 {
        (self.log_entries.len() as u16).saturating_sub(self.layout.logs_body.height)
    }

    fn clamp_detail_scroll(&mut self) {
        self.detail_scroll = self.detail_scroll.min(self.max_detail_scroll());
    }

    fn clamp_log_scroll(&mut self) {
        self.log_scroll = self.log_scroll.min(self.max_log_scroll());
    }

    fn ensure_history_visible(&mut self) {
        let slots = self.history_slots();
        if self.selected_history < self.history_offset {
            self.history_offset = self.selected_history;
        } else if self.selected_history >= self.history_offset.saturating_add(slots) {
            self.history_offset = self.selected_history + 1 - slots;
        }
    }

    fn ensure_command_visible(&mut self) {
        let Some(selected_command) = self.selected_command else {
            self.command_offset = 0;
            return;
        };
        let slots = self.command_slots();
        if selected_command < self.command_offset {
            self.command_offset = selected_command;
        } else if selected_command >= self.command_offset.saturating_add(slots) {
            self.command_offset = selected_command + 1 - slots;
        }
    }

    fn selected_entry(&self) -> Option<&ConversationEntry> {
        self.history.get(self.selected_history)
    }

    fn history_index_at(&self, column: u16, row: u16) -> Option<usize> {
        if !point_in_rect(self.layout.timeline_body, column, row) {
            return None;
        }
        let relative_y = row.saturating_sub(self.layout.timeline_body.y);
        let slot = (relative_y / HISTORY_ITEM_HEIGHT) as usize;
        let index = self.history_offset + slot;
        (index < self.history.len()).then_some(index)
    }

    fn command_index_at(&self, column: u16, row: u16) -> Option<usize> {
        let entry = self.selected_entry()?;
        if !point_in_rect(self.layout.commands_body, column, row) {
            return None;
        }
        let relative_y = row.saturating_sub(self.layout.commands_body.y);
        let slot = (relative_y / COMMAND_ITEM_HEIGHT) as usize;
        let index = self.command_offset + slot;
        (index < entry.commands.len()).then_some(index)
    }

    fn selected_command(&self) -> Option<&CommandCard> {
        let entry = self.selected_entry()?;
        let index = self.selected_command?;
        entry.commands.get(index)
    }

    fn selected_detail_text(&self) -> Text<'static> {
        let Some(entry) = self.selected_entry() else {
            return Text::from(vec![
                Line::from("No conversation yet.").style(MUTED_STYLE),
                Line::from("Use the composer below to start a chat.").style(MUTED_STYLE),
            ]);
        };
        let mut lines = vec![
            Line::from(vec![
                Span::styled(entry.title().to_string(), entry.title_style()),
                Span::raw("  "),
                Span::styled(
                    format!("#{}", self.selected_history + 1),
                    Style::new().fg(Color::Gray),
                ),
            ]),
            Line::from(""),
        ];
        let markdown =
            if entry.markdown.trim().is_empty() && entry.role == TranscriptRole::Assistant {
                "Streaming reply..."
            } else {
                entry.markdown.as_str()
            };
        lines.extend(render_markdown(markdown).lines);
        if !entry.commands.is_empty() {
            lines.push(Line::from(""));
            if self.layout.mode.shows_command_dock() {
                lines
                    .push(Line::from("Command cards are available in the dock.").style(CARD_STYLE));
            } else {
                lines.push(Line::from("Commands").style(CARD_STYLE));
                for (index, command) in entry.commands.iter().enumerate() {
                    let marker = if self.selected_command == Some(index) {
                        "▶"
                    } else {
                        " "
                    };
                    lines.push(Line::from(format!(
                        "{marker} {}. {}",
                        index + 1,
                        command.summary
                    )));
                    lines.extend(command_text_lines(
                        &command.command,
                        Style::new().fg(Color::Gray),
                    ));
                }
                lines.push(Line::from("1-9 select | c copy | e exec | p place").style(MUTED_STYLE));
            }
        }
        Text::from(lines)
    }

    fn visible_history_items(&self) -> Vec<ListItem<'static>> {
        let width = self.layout.timeline_body.width.saturating_sub(3) as usize;
        self.history
            .iter()
            .enumerate()
            .skip(self.history_offset)
            .take(self.history_slots())
            .map(|(index, entry)| {
                let (first, second) = entry.preview_lines();
                let selected = index == self.selected_history;
                let marker = if selected { "●" } else { "○" };
                let meta = if entry.commands.is_empty() {
                    "No command cards".to_string()
                } else {
                    format!("{} command card(s)", entry.commands.len())
                };
                let lines = vec![
                    Line::from(vec![
                        Span::styled(marker, if selected { TITLE_STYLE } else { MUTED_STYLE }),
                        Span::raw(" "),
                        Span::styled(
                            format!("#{} {}", index + 1, entry.title()),
                            entry.title_style(),
                        ),
                    ]),
                    Line::from(truncate_line(&first, width)),
                    Line::from(truncate_line(&second, width)).style(MUTED_STYLE),
                    Line::from(meta).style(CARD_STYLE),
                ];
                ListItem::new(Text::from(lines)).style(if selected {
                    Style::new().bg(Color::DarkGray)
                } else {
                    Style::new()
                })
            })
            .collect()
    }

    fn visible_command_items(&self) -> Vec<ListItem<'static>> {
        let width = self.layout.commands_body.width.saturating_sub(3) as usize;
        let Some(entry) = self.selected_entry() else {
            return Vec::new();
        };
        entry
            .commands
            .iter()
            .enumerate()
            .skip(self.command_offset)
            .take(self.command_slots())
            .map(|(index, command)| {
                let selected = self.selected_command == Some(index);
                let marker = if selected { "▶" } else { " " };
                let lines = vec![
                    Line::from(vec![
                        Span::styled(marker, if selected { TITLE_STYLE } else { MUTED_STYLE }),
                        Span::raw(" "),
                        Span::styled(
                            format!("{}. {}", index + 1, truncate_line(&command.summary, width)),
                            if selected { CARD_STYLE } else { Style::new() },
                        ),
                    ]),
                    Line::from(command_preview(&command.command, width))
                        .style(Style::new().fg(Color::Gray)),
                    Line::from("c copy | e exec | enter place").style(MUTED_STYLE),
                    Line::from(""),
                ];
                ListItem::new(Text::from(lines)).style(if selected {
                    Style::new().bg(Color::DarkGray)
                } else {
                    Style::new()
                })
            })
            .collect()
    }

    fn push_user_markdown(&mut self, prompt: String) {
        self.history.push(ConversationEntry {
            role: TranscriptRole::User,
            markdown: prompt,
            commands: Vec::new(),
        });
        self.selected_history = self.history.len().saturating_sub(1);
        self.selected_command = None;
        self.detail_scroll = 0;
    }

    fn begin_assistant_stream(&mut self) -> usize {
        self.history.push(ConversationEntry {
            role: TranscriptRole::Assistant,
            markdown: String::new(),
            commands: Vec::new(),
        });
        self.streaming = true;
        self.selected_history = self.history.len().saturating_sub(1);
        self.selected_command = None;
        self.detail_scroll = 0;
        self.ensure_history_visible();
        self.selected_history
    }

    fn prepare_submitted_prompt(&mut self, prompt: String) -> usize {
        self.push_user_markdown(prompt);
        self.input = EditorState::default();
        self.status = "Streaming assistant reply...".to_string();
        self.begin_assistant_stream()
    }

    fn append_stream_chunk(&mut self, index: usize, chunk: &str) {
        if let Some(entry) = self.history.get_mut(index) {
            entry.markdown.push_str(chunk);
        }
    }

    fn replace_stream_commands(
        &mut self,
        index: usize,
        commands: &[crate::agent::tools::CommandCandidate],
    ) {
        if let Some(entry) = self.history.get_mut(index) {
            entry.commands = commands.iter().cloned().map(CommandCard::from).collect();
            if self.selected_history == index
                && !entry.commands.is_empty()
                && self.selected_command.is_none()
            {
                self.selected_command = Some(0);
            }
            self.ensure_command_visible();
        }
    }

    fn finalize_stream(&mut self, index: usize, turn: &AssistantTurn) {
        if let Some(entry) = self.history.get_mut(index) {
            entry.markdown = turn.reply_markdown.clone();
            entry.commands = turn
                .commands
                .iter()
                .cloned()
                .map(CommandCard::from)
                .collect();
        }
        self.streaming = false;
        self.selected_history = index;
        self.selected_command = if turn.commands.is_empty() {
            None
        } else {
            Some(0)
        };
        self.detail_scroll = 0;
        self.ensure_history_visible();
        self.ensure_command_visible();
    }

    fn mark_stream_error(&mut self, index: usize, message: &str) {
        if let Some(entry) = self.history.get_mut(index) {
            if entry.markdown.trim().is_empty() {
                entry.markdown = format!("Streaming error: {message}");
            } else {
                entry.markdown.push_str("\n\n");
                entry
                    .markdown
                    .push_str(&format!("Streaming error: {message}"));
            }
        }
        self.streaming = false;
    }

    fn current_command(&self) -> Option<&CommandCard> {
        self.selected_command()
    }
}

impl App {
    fn new(
        agent: ChatAgent,
        session: ChatSession,
        session_store: SessionStore,
        shell: &Shell,
        attached: Option<String>,
        initial_prompt: Option<String>,
    ) -> io::Result<Self> {
        let widget = ChatWidget::from_session(&session, initial_prompt);
        let terminal = InlineTerminal::init_fullscreen_with_options(ratatui::TerminalOptions {
            viewport: Viewport::Fullscreen,
        })?;
        Ok(Self {
            terminal,
            widget,
            agent: Arc::new(agent),
            session,
            session_store,
            shell_path: shell.path().to_path_buf(),
            attached,
            pending_stream: None,
            streaming_entry: None,
        })
    }

    fn spawn_event_loop(
        tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    ) -> tokio::task::JoinHandle<()> {
        macro_rules! send {
            ($value:expr) => {
                if tx.send($value).is_err() {
                    break;
                }
            };
        }

        tokio::spawn(async move {
            let mut event_stream = crossterm::event::EventStream::new();
            while let Some(evt) = event_stream.next().await {
                match evt {
                    Ok(Event::Key(key))
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        send!(map_key_event(key));
                    }
                    Ok(Event::Mouse(mouse)) => send!(AppEvent::Mouse(mouse)),
                    Ok(Event::Resize(_, _)) => send!(AppEvent::Resize),
                    Ok(_) => {}
                    Err(error) => send!(AppEvent::Err(error)),
                }
            }
        })
    }

    async fn save_session(&mut self) -> Result<()> {
        self.session.transcript = self
            .widget
            .history
            .iter()
            .map(ConversationEntry::to_transcript_entry)
            .collect();
        self.session.touch();
        self.session_store.save(&self.session).await
    }

    async fn submit_prompt(&mut self, prompt: String) -> Result<()> {
        if self.pending_stream.is_some() {
            self.widget.status =
                "Assistant is still streaming. Wait for the current turn.".to_string();
            return Ok(());
        }

        let assistant_index = self.widget.prepare_submitted_prompt(prompt.clone());
        self.save_session().await?;

        match self
            .agent
            .stream_resolve(prompt, self.session.messages.clone(), self.attached.take())
            .await
        {
            Ok(stream) => {
                self.pending_stream = Some(stream);
                self.streaming_entry = Some(assistant_index);
            }
            Err(error) => {
                self.widget
                    .mark_stream_error(assistant_index, &error.to_string());
                self.widget.status = format!("Failed to start stream: {error}");
                self.save_session().await?;
            }
        }

        Ok(())
    }

    async fn finish_pending_stream(&mut self) {
        if let Some(stream) = self.pending_stream.take() {
            stream.finish().await;
        }
        self.streaming_entry = None;
    }

    async fn abort_pending_stream(&mut self) {
        if let Some(stream) = self.pending_stream.take() {
            stream.stop().await;
        }
        self.streaming_entry = None;
    }

    async fn handle_stream_event(&mut self, event: ChatStreamEvent) -> Result<()> {
        let Some(index) = self.streaming_entry else {
            return Ok(());
        };

        match event {
            ChatStreamEvent::TextDelta(delta) => {
                self.widget.append_stream_chunk(index, &delta);
            }
            ChatStreamEvent::Commands(commands) => {
                self.widget.replace_stream_commands(index, &commands);
                self.widget.status = "Command cards received. Review them in the dock.".to_string();
            }
            ChatStreamEvent::Final(turn) => {
                self.session.messages = turn.messages.clone();
                self.widget.finalize_stream(index, &turn);
                self.widget.status = if turn.commands.is_empty() {
                    "Reply received. Continue chatting from the composer.".to_string()
                } else {
                    "Reply received. Use the command dock to copy, execute or place.".to_string()
                };
                self.finish_pending_stream().await;
                self.save_session().await?;
            }
        }

        Ok(())
    }

    async fn handle_stream_error(&mut self, error: String) -> Result<()> {
        if let Some(index) = self.streaming_entry {
            self.widget.mark_stream_error(index, &error);
            self.widget.status = format!("Stream failed: {error}");
            self.finish_pending_stream().await;
            self.save_session().await?;
        }
        Ok(())
    }

    async fn handle_command_action(&mut self, action: char) -> Result<()> {
        let Some(command) = self.widget.current_command().cloned() else {
            self.widget.status = "No command card is selected.".to_string();
            return Ok(());
        };

        match action {
            'c' => {
                copy_command(command.command.clone())?;
                self.widget.status = "Copied selected command.".to_string();
            }
            'e' => {
                execute_command(command.command.clone(), &self.shell_path).await?;
                self.widget.status = "Executed selected command.".to_string();
            }
            'p' => {
                print_command_to_input_buffer(None::<&Path>, &command.command).await?;
                self.widget.status =
                    "Printed selected command to the shell input buffer.".to_string();
            }
            _ => {}
        }

        Ok(())
    }

    fn move_history(&mut self, step: isize) {
        if self.widget.history.is_empty() {
            return;
        }
        let current = self.widget.selected_history as isize;
        let max = self.widget.history.len().saturating_sub(1) as isize;
        self.widget.selected_history = (current + step).clamp(0, max) as usize;
        self.widget.selected_command = None;
        self.widget.detail_scroll = 0;
        self.widget.repair_selection();
        self.widget.ensure_history_visible();
        self.widget.ensure_command_visible();
    }

    fn move_command(&mut self, step: isize) {
        let Some(entry) = self.widget.selected_entry() else {
            return;
        };
        if entry.commands.is_empty() {
            self.widget.selected_command = None;
            return;
        }
        let current = self.widget.selected_command.unwrap_or(0) as isize;
        let max = entry.commands.len().saturating_sub(1) as isize;
        self.widget.selected_command = Some((current + step).clamp(0, max) as usize);
        self.widget.ensure_command_visible();
    }

    fn scroll_detail(&mut self, step: i16) {
        let current = self.widget.detail_scroll as i16;
        let next = (current + step).clamp(0, self.widget.max_detail_scroll() as i16);
        self.widget.detail_scroll = next as u16;
    }

    fn scroll_logs(&mut self, step: i16) {
        self.widget.pin_logs_to_tail = false;
        let current = self.widget.log_scroll as i16;
        let next = (current + step).clamp(0, self.widget.max_log_scroll() as i16);
        self.widget.log_scroll = next as u16;
        if self.widget.log_scroll >= self.widget.max_log_scroll() {
            self.widget.pin_logs_to_tail = true;
        }
    }

    fn focus_next(&mut self) {
        self.widget.focus = if self.widget.layout.mode.is_tiny() {
            match self.widget.focus {
                Focus::Composer => Focus::Spotlight,
                _ => Focus::Composer,
            }
        } else {
            self.widget.focus.next()
        };
    }

    fn focus_previous(&mut self) {
        self.widget.focus = if self.widget.layout.mode.is_tiny() {
            match self.widget.focus {
                Focus::Composer => Focus::Spotlight,
                _ => Focus::Composer,
            }
        } else {
            self.widget.focus.previous()
        };
    }

    fn select_history_from_mouse(&mut self, mouse: MouseEvent) {
        if let Some(index) = self.widget.history_index_at(mouse.column, mouse.row) {
            self.widget.selected_history = index;
            self.widget.selected_command = None;
            self.widget.detail_scroll = 0;
            self.widget.focus = Focus::Timeline;
            self.widget.repair_selection();
            self.widget.ensure_command_visible();
        }
    }

    fn select_command_from_mouse(&mut self, mouse: MouseEvent) {
        if let Some(index) = self.widget.command_index_at(mouse.column, mouse.row) {
            self.widget.selected_command = Some(index);
            self.widget.focus = Focus::Commands;
            self.widget.ensure_command_visible();
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if point_in_rect(self.widget.layout.composer, mouse.column, mouse.row) {
                    self.widget.focus = Focus::Composer;
                    return;
                }
                if point_in_rect(self.widget.layout.timeline, mouse.column, mouse.row) {
                    self.select_history_from_mouse(mouse);
                    return;
                }
                if point_in_rect(self.widget.layout.commands, mouse.column, mouse.row) {
                    self.select_command_from_mouse(mouse);
                    return;
                }
                if point_in_rect(self.widget.layout.spotlight, mouse.column, mouse.row) {
                    self.widget.focus = Focus::Spotlight;
                }
            }
            MouseEventKind::ScrollUp => {
                if point_in_rect(self.widget.layout.timeline, mouse.column, mouse.row) {
                    self.move_history(-1);
                } else if point_in_rect(self.widget.layout.commands, mouse.column, mouse.row) {
                    self.move_command(-1);
                } else if point_in_rect(self.widget.layout.spotlight, mouse.column, mouse.row) {
                    self.scroll_detail(-3);
                } else if point_in_rect(self.widget.layout.logs, mouse.column, mouse.row) {
                    self.scroll_logs(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if point_in_rect(self.widget.layout.timeline, mouse.column, mouse.row) {
                    self.move_history(1);
                } else if point_in_rect(self.widget.layout.commands, mouse.column, mouse.row) {
                    self.move_command(1);
                } else if point_in_rect(self.widget.layout.spotlight, mouse.column, mouse.row) {
                    self.scroll_detail(3);
                } else if point_in_rect(self.widget.layout.logs, mouse.column, mouse.row) {
                    self.scroll_logs(3);
                }
            }
            _ => {}
        }
    }

    fn render_frame(frame: &mut Frame, widget: &ChatWidget) {
        let area = frame.area();

        frame.render_widget(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(OUTER_BORDER_STYLE)
                .title(Line::from("Howlto Chat").style(TITLE_STYLE)),
            area,
        );

        if widget.layout.mode.shows_timeline() {
            Self::render_header(frame, widget);
            Self::render_timeline(frame, widget);
        }
        Self::render_spotlight(frame, widget);
        if widget.layout.mode.shows_command_dock() {
            Self::render_commands(frame, widget);
        }
        if widget.layout.mode.shows_logs() {
            Self::render_logs(frame, widget);
        }
        Self::render_composer(frame, widget);
        if widget.layout.mode.shows_timeline() {
            Self::render_footer(frame, widget);
        }
    }

    fn render_header(frame: &mut Frame, widget: &ChatWidget) {
        let short_session = widget.session_id.chars().take(8).collect::<String>();
        let header = Text::from(vec![
            Line::from(vec![
                Span::styled("Howlto", TITLE_STYLE),
                Span::raw("  "),
                Span::styled(format!("session {}", short_session), STATUS_STYLE),
                Span::raw("  "),
                Span::styled(
                    format!("focus {}", widget.focus.label()),
                    Style::new().fg(Color::LightCyan),
                ),
                Span::raw("  "),
                Span::styled(
                    if widget.streaming {
                        "live stream on"
                    } else {
                        "idle"
                    },
                    if widget.streaming {
                        Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        MUTED_STYLE
                    },
                ),
            ]),
            Line::from(widget.status.clone()).style(STATUS_STYLE),
        ]);

        frame.render_widget(
            Paragraph::new(header).block(
                Block::bordered()
                    .title("Workbench")
                    .border_type(BorderType::Rounded)
                    .border_style(PANEL_BORDER_STYLE),
            ),
            widget.layout.header,
        );
    }

    fn render_timeline(frame: &mut Frame, widget: &ChatWidget) {
        frame.render_widget(
            List::new(widget.visible_history_items()).block(
                Block::bordered()
                    .title("Timeline")
                    .border_type(BorderType::Rounded)
                    .border_style(if widget.focus == Focus::Timeline {
                        ACTIVE_BORDER_STYLE
                    } else {
                        PANEL_BORDER_STYLE
                    }),
            ),
            widget.layout.timeline,
        );
    }

    fn render_spotlight(frame: &mut Frame, widget: &ChatWidget) {
        frame.render_widget(
            Paragraph::new(widget.selected_detail_text())
                .wrap(Wrap { trim: false })
                .scroll((widget.detail_scroll, 0))
                .block(
                    Block::bordered()
                        .title(widget.layout.mode.spotlight_title())
                        .border_type(BorderType::Rounded)
                        .border_style(if widget.focus == Focus::Spotlight {
                            ACTIVE_BORDER_STYLE
                        } else {
                            PANEL_BORDER_STYLE
                        }),
                ),
            widget.layout.spotlight,
        );
    }

    fn render_commands(frame: &mut Frame, widget: &ChatWidget) {
        let title = if let Some(entry) = widget.selected_entry() {
            format!("Command Dock ({})", entry.commands.len())
        } else {
            "Command Dock".to_string()
        };

        if widget.visible_command_items().is_empty() {
            frame.render_widget(
                Paragraph::new(Text::from(vec![
                    Line::from("No command cards on the selected turn.").style(MUTED_STYLE),
                    Line::from("Assistant command suggestions will appear here.")
                        .style(MUTED_STYLE),
                ]))
                .wrap(Wrap { trim: false })
                .block(
                    Block::bordered()
                        .title(title)
                        .border_type(BorderType::Rounded)
                        .border_style(if widget.focus == Focus::Commands {
                            ACTIVE_BORDER_STYLE
                        } else {
                            PANEL_BORDER_STYLE
                        }),
                ),
                widget.layout.commands,
            );
            return;
        }

        frame.render_widget(
            List::new(widget.visible_command_items()).block(
                Block::bordered()
                    .title(title)
                    .border_type(BorderType::Rounded)
                    .border_style(if widget.focus == Focus::Commands {
                        ACTIVE_BORDER_STYLE
                    } else {
                        PANEL_BORDER_STYLE
                    }),
            ),
            widget.layout.commands,
        );
    }

    fn render_logs(frame: &mut Frame, widget: &ChatWidget) {
        frame.render_widget(
            Paragraph::new(render_compact_log_lines(&widget.log_entries))
                .wrap(Wrap { trim: false })
                .scroll((widget.log_scroll, 0))
                .block(
                    Block::bordered()
                        .title("Tracing")
                        .border_type(BorderType::Rounded)
                        .border_style(Style::new().fg(Color::Magenta)),
                ),
            widget.layout.logs,
        );
    }

    fn render_composer(frame: &mut Frame, widget: &ChatWidget) {
        frame.render_widget(
            Paragraph::new(widget.input.lines_with_cursor())
                .wrap(Wrap { trim: false })
                .block(
                    Block::bordered()
                        .title("Composer")
                        .border_type(BorderType::Rounded)
                        .border_style(if widget.focus == Focus::Composer {
                            Style::new().fg(Color::LightGreen)
                        } else {
                            PANEL_BORDER_STYLE
                        }),
                ),
            widget.layout.composer,
        );
    }

    fn render_footer(frame: &mut Frame, widget: &ChatWidget) {
        let footer = Line::from(
            "Tab shift focus | arrows scroll or move | c/e/p act on command cards | Esc or Ctrl-C quit",
        )
        .style(MUTED_STYLE)
        .centered();
        footer.render(widget.layout.footer, frame.buffer_mut());
    }

    async fn run(mut self) -> Result<ChatSession> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = Self::spawn_event_loop(tx);

        let result: Result<()> = loop {
            {
                let App {
                    terminal, widget, ..
                } = &mut self;
                let area = terminal.size()?;
                widget.sync_layout(area.into());
                widget.refresh_logs();
                terminal.draw(|frame| Self::render_frame(frame, widget))?;
            }

            let next_event = if let Some(stream) = self.pending_stream.as_mut() {
                tokio::select! {
                    maybe_event = rx.recv() => maybe_event,
                    maybe_stream = stream.next() => {
                        match maybe_stream {
                            Some(Ok(event)) => Some(AppEvent::Stream(event)),
                            Some(Err(error)) => Some(AppEvent::StreamError(error.to_string())),
                            None => Some(AppEvent::StreamClosed),
                        }
                    }
                }
            } else {
                rx.recv().await
            };

            let Some(event) = next_event else {
                break Ok(());
            };
            debug!("chat event: {event:?}");

            match event {
                AppEvent::Up => match self.widget.focus {
                    Focus::Timeline => self.move_history(-1),
                    Focus::Spotlight => self.scroll_detail(-2),
                    Focus::Commands => self.move_command(-1),
                    Focus::Composer => {
                        self.widget.input.handle_event(
                            Event::Key(crossterm::event::KeyEvent::new(
                                KeyCode::Up,
                                KeyModifiers::NONE,
                            )),
                            false,
                        );
                    }
                },
                AppEvent::Down => match self.widget.focus {
                    Focus::Timeline => self.move_history(1),
                    Focus::Spotlight => self.scroll_detail(2),
                    Focus::Commands => self.move_command(1),
                    Focus::Composer => {
                        self.widget.input.handle_event(
                            Event::Key(crossterm::event::KeyEvent::new(
                                KeyCode::Down,
                                KeyModifiers::NONE,
                            )),
                            false,
                        );
                    }
                },
                AppEvent::Left => match self.widget.focus {
                    Focus::Composer => {
                        self.widget.input.handle_event(
                            Event::Key(crossterm::event::KeyEvent::new(
                                KeyCode::Left,
                                KeyModifiers::NONE,
                            )),
                            false,
                        );
                    }
                    _ => self.focus_previous(),
                },
                AppEvent::Right => match self.widget.focus {
                    Focus::Composer => {
                        self.widget.input.handle_event(
                            Event::Key(crossterm::event::KeyEvent::new(
                                KeyCode::Right,
                                KeyModifiers::NONE,
                            )),
                            false,
                        );
                    }
                    _ => self.focus_next(),
                },
                AppEvent::NextFocus => self.focus_next(),
                AppEvent::PrevFocus => self.focus_previous(),
                AppEvent::Newline => {
                    if self.widget.focus == Focus::Composer {
                        self.widget.input.insert_newline();
                    }
                }
                AppEvent::Send => {
                    if self.widget.focus == Focus::Composer {
                        let prompt = self.widget.input.text().trim().to_string();
                        if prompt.is_empty() {
                            self.widget.status = "Input is empty.".to_string();
                        } else {
                            self.submit_prompt(prompt).await?;
                        }
                    } else if self.widget.focus == Focus::Commands {
                        self.handle_command_action('p').await?;
                    }
                }
                AppEvent::SoftQuit => break Ok(()),
                AppEvent::HardQuit => break Ok(()),
                AppEvent::RawKey(key) => match self.widget.focus {
                    Focus::Composer => {
                        self.widget.input.handle_event(Event::Key(key), false);
                    }
                    Focus::Timeline => match key.code {
                        KeyCode::Char('j') => self.move_history(1),
                        KeyCode::Char('k') => self.move_history(-1),
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Char('1'..='9') => {}
                        _ => {}
                    },
                    Focus::Spotlight => match key.code {
                        KeyCode::Char('j') => self.scroll_detail(2),
                        KeyCode::Char('k') => self.scroll_detail(-2),
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Char(ch @ '1'..='9') if self.widget.layout.mode.is_tiny() => {
                            let index = ch.to_digit(10).unwrap_or(1) as usize - 1;
                            if let Some(entry) = self.widget.selected_entry()
                                && index < entry.commands.len()
                            {
                                self.widget.selected_command = Some(index);
                            }
                        }
                        KeyCode::Char('c') | KeyCode::Char('e') | KeyCode::Char('p') => {
                            if let KeyCode::Char(action) = key.code {
                                self.handle_command_action(action).await?;
                            }
                        }
                        _ => {}
                    },
                    Focus::Commands => match key.code {
                        KeyCode::Char('j') => self.move_command(1),
                        KeyCode::Char('k') => self.move_command(-1),
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Char('c') | KeyCode::Char('e') | KeyCode::Char('p') => {
                            if let KeyCode::Char(action) = key.code {
                                self.handle_command_action(action).await?;
                            }
                        }
                        KeyCode::Char(ch @ '1'..='9') => {
                            let index = ch.to_digit(10).unwrap_or(1) as usize - 1;
                            if let Some(entry) = self.widget.selected_entry()
                                && index < entry.commands.len()
                            {
                                self.widget.selected_command = Some(index);
                                self.widget.ensure_command_visible();
                            }
                        }
                        _ => {}
                    },
                },
                AppEvent::Mouse(mouse) => self.handle_mouse(mouse),
                AppEvent::Resize => {}
                AppEvent::Stream(event) => {
                    self.handle_stream_event(event).await?;
                }
                AppEvent::StreamError(error) => {
                    self.handle_stream_error(error).await?;
                }
                AppEvent::StreamClosed => {
                    if self.widget.streaming {
                        self.handle_stream_error(
                            "assistant stream closed unexpectedly".to_string(),
                        )
                        .await?;
                    }
                }
                AppEvent::Err(error) => break Err(error.into()),
            }
        };

        self.abort_pending_stream().await;
        handle.abort();
        handle.await.ok();
        result?;
        self.save_session().await?;
        Ok(self.session)
    }
}

#[bon::builder]
pub async fn run(
    config: AppConfig,
    shell: &Shell,
    profiles: Profiles,
    session_store: SessionStore,
    prompt: Option<String>,
    resume_id: Option<String>,
    attached: Option<String>,
) -> Result<()> {
    let mut session = if let Some(resume_id) = resume_id {
        session_store.load(&resume_id).await?
    } else {
        ChatSession::new()
    };
    let agent = ChatAgent::new(detect_os(), shell, profiles.chat, config)?;
    let app = App::new(
        agent,
        session.clone(),
        session_store,
        shell,
        attached,
        prompt,
    )?;
    session = app.run().await?;
    println!("session_id={}", session.id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AppEvent, ChatWidget, CommandCard, ConversationEntry, Focus, LayoutMode, TranscriptRole,
        map_key_event,
    };
    use crate::session::ChatSession;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Rect;

    fn widget_with_history() -> ChatWidget {
        let mut widget = ChatWidget::from_session(&ChatSession::new(), None);
        widget.history = vec![
            ConversationEntry {
                role: TranscriptRole::User,
                markdown: "show me files".into(),
                commands: Vec::new(),
            },
            ConversationEntry {
                role: TranscriptRole::Assistant,
                markdown: "Use ls for this.".into(),
                commands: vec![
                    CommandCard {
                        command: "ls".into(),
                        summary: "List files".into(),
                    },
                    CommandCard {
                        command: "ls -la".into(),
                        summary: "List files with hidden entries".into(),
                    },
                ],
            },
        ];
        widget.selected_history = 1;
        widget.selected_command = Some(0);
        widget.sync_layout(Rect::new(0, 0, 160, 42));
        widget
    }

    #[test]
    fn ctrl_c_maps_to_hard_quit() {
        let event = map_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(event, AppEvent::HardQuit));
    }

    #[test]
    fn plain_q_stays_as_raw_key() {
        let event = map_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(matches!(event, AppEvent::RawKey(_)));
    }

    #[test]
    fn mouse_hit_testing_maps_to_history_and_command_slots() {
        let widget = widget_with_history();
        let timeline_index = widget
            .history_index_at(widget.layout.timeline_body.x, widget.layout.timeline_body.y)
            .unwrap();
        let command_index = widget
            .command_index_at(widget.layout.commands_body.x, widget.layout.commands_body.y)
            .unwrap();

        assert_eq!(timeline_index, widget.history_offset);
        assert_eq!(command_index, widget.command_offset);
    }

    #[test]
    fn stream_chunk_updates_assistant_entry_incrementally() {
        let mut widget = ChatWidget::from_session(&ChatSession::new(), None);
        let index = widget.begin_assistant_stream();
        widget.append_stream_chunk(index, "hello");
        widget.append_stream_chunk(index, " world");

        assert_eq!(widget.history[index].markdown, "hello world");
        assert!(widget.streaming);
        assert_eq!(widget.focus, Focus::Composer);
    }

    #[test]
    fn submitting_prompt_keeps_composer_focus() {
        let mut widget = ChatWidget::from_session(&ChatSession::new(), None);
        widget.focus = Focus::Composer;
        let index = widget.prepare_submitted_prompt("show me rust files".into());

        assert_eq!(widget.focus, Focus::Composer);
        assert_eq!(widget.selected_history, index);
        assert!(widget.streaming);
        assert!(widget.input.text().is_empty());
    }

    #[test]
    fn tiny_layout_hides_timeline_and_tracing() {
        let mut widget = widget_with_history();
        widget.sync_layout(Rect::new(0, 0, 80, 20));

        assert_eq!(widget.layout.mode, LayoutMode::Tiny);
        assert_eq!(widget.layout.timeline.width, 0);
        assert_eq!(widget.layout.commands.width, 0);
        assert_eq!(widget.layout.logs.width, 0);
    }

    #[test]
    fn tiny_layout_inlines_command_cards_into_output() {
        let mut widget = widget_with_history();
        widget.sync_layout(Rect::new(0, 0, 80, 20));
        let rendered = widget.selected_detail_text();
        let content = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(content.contains("Commands"));
        assert!(content.contains("1-9 select | c copy | e exec | p place"));
        assert!(content.contains("ls -la"));
    }

    #[test]
    fn tiny_layout_focus_cycle_skips_hidden_panels() {
        let mut widget = widget_with_history();
        widget.sync_layout(Rect::new(0, 0, 80, 20));

        let next_focus = match widget.focus {
            Focus::Composer => Focus::Spotlight,
            _ => Focus::Composer,
        };
        widget.focus = next_focus;
        assert_eq!(widget.focus, Focus::Spotlight);

        let next_focus = match widget.focus {
            Focus::Composer => Focus::Spotlight,
            _ => Focus::Composer,
        };
        widget.focus = next_focus;
        assert_eq!(widget.focus, Focus::Composer);
    }
}
