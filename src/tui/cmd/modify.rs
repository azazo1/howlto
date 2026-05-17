use std::{
    io,
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    Viewport,
    prelude::*,
    widgets::{Block, BorderType, Paragraph},
};
use tokio::{
    sync::mpsc::{UnboundedSender, unbounded_channel},
    task::JoinHandle,
};
use tokio_stream::StreamExt;
use unicode_width::UnicodeWidthStr;

use crate::{
    error::Result,
    tui::{cmd::MINIMUM_TUI_WIDTH, editor::EditorState, terminal::InlineTerminal},
};

const TITLE: &str = "Modify Prompt";
const TITLE_STYLE: Style = Style::new()
    .fg(Color::LightGreen)
    .add_modifier(Modifier::BOLD);
const HINT: &str = "enter: confirm | ctrl-j: newline | esc: quit";
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const COMMAND_TITLE: &str = "Command";
const COMMAND_TITLE_STYLE: Style = Style::new().fg(Color::LightBlue);
const COMMAND_BORDER_STYLE: Style = Style::new().fg(Color::Blue);
const INPUT_BORDER_STYLE: Style = Style::new().fg(Color::Gray);

#[derive(Debug)]
pub struct App {
    terminal: InlineTerminal,
    widget: AppWidget,
}

#[derive(Debug)]
struct AppWidget {
    command: String,
    editor: EditorState,
}

#[derive(Debug)]
enum AppEvent {
    Quit,
    Confirm,
    Raw(Event),
    Err(io::Error),
}

impl Widget for &AppWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let width = (self.command.width_cjk() + 3)
            .max(HINT.width_cjk() + 3)
            .max(MINIMUM_TUI_WIDTH) as u16;
        let [area] = Layout::horizontal([Constraint::Length(width)]).areas(area);
        let [command_block_area, input_area, hint_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .areas(area);
        let [hint_area] = Layout::horizontal([Constraint::Fill(1)])
            .horizontal_margin(1)
            .areas(hint_area);
        let [command_area] = Layout::horizontal([Constraint::Fill(1)])
            .margin(1)
            .areas(command_block_area);
        Line::from(HINT)
            .style(HINT_STYLE)
            .right_aligned()
            .render(hint_area, buf);
        Block::bordered()
            .border_style(COMMAND_BORDER_STYLE)
            .border_type(BorderType::Rounded)
            .title_top("")
            .title_top(Line::from(COMMAND_TITLE).style(COMMAND_TITLE_STYLE))
            .render(command_block_area, buf);
        Paragraph::new(self.command.clone()).render(command_area, buf);
        Paragraph::new(self.editor.lines_with_cursor())
            .block(
                Block::bordered()
                    .title_top("")
                    .title_top(Line::from(TITLE).style(TITLE_STYLE))
                    .border_style(INPUT_BORDER_STYLE)
                    .border_type(BorderType::Rounded),
            )
            .render(input_area, buf);
    }
}

impl App {
    pub fn new(command: String) -> Result<Self> {
        let terminal = InlineTerminal::init_inline_with_options(ratatui::TerminalOptions {
            viewport: Viewport::Inline(8 + command.lines().count() as u16),
        })?;
        let widget = AppWidget {
            command,
            editor: EditorState::default(),
        };
        Ok(Self { terminal, widget })
    }

    fn start_handle_events(&self, tx: UnboundedSender<AppEvent>) -> JoinHandle<()> {
        macro_rules! send {
            ($s:expr) => {
                if tx.send($s).is_err() {
                    break;
                }
            };
        }

        let start_time = Instant::now();
        let skip_enter_duration = Duration::from_millis(10);
        tokio::spawn(async move {
            let mut event_stream = crossterm::event::EventStream::new();
            while let Some(evt) = event_stream.next().await {
                match evt {
                    Ok(Event::Key(key)) => match key.code {
                        KeyCode::Char('c') | KeyCode::Char('d')
                            if key.modifiers == KeyModifiers::CONTROL =>
                        {
                            send!(AppEvent::Quit)
                        }
                        KeyCode::Esc => send!(AppEvent::Quit),
                        KeyCode::Enter if key.modifiers == KeyModifiers::CONTROL => {
                            send!(AppEvent::Raw(Event::Key(key)))
                        }
                        KeyCode::Enter if key.modifiers.is_empty() => {
                            if start_time.elapsed() > skip_enter_duration {
                                send!(AppEvent::Confirm)
                            }
                        }
                        _ => send!(AppEvent::Raw(Event::Key(key))),
                    },
                    Err(e) => send!(AppEvent::Err(e)),
                    _ => {}
                }
            }
        })
    }

    async fn run(mut self) -> Result<Option<String>> {
        let (tx, mut rx) = unbounded_channel();
        let handle = self.start_handle_events(tx);
        let rst = loop {
            self.terminal
                .draw(|frame| frame.render_widget(&self.widget, frame.area()))?;
            let Some(evt) = rx.recv().await else {
                break Ok(None);
            };
            match evt {
                AppEvent::Quit => break Ok(None),
                AppEvent::Confirm => break Ok(Some(self.widget.editor.text())),
                AppEvent::Raw(raw) => {
                    self.widget.editor.handle_event(raw, true);
                }
                AppEvent::Err(error) => break Err(error.into()),
            }
        };
        handle.abort();
        handle.await.ok();
        rst
    }

    pub async fn prompt(command: String) -> Result<Option<String>> {
        App::new(command)?.run().await
    }
}
