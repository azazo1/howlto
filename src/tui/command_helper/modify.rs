use std::io::{self, Stderr};

use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    Terminal, Viewport,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Widget},
};
use tokio::{
    sync::mpsc::{UnboundedSender, unbounded_channel},
    task::JoinHandle,
};
use tokio_stream::StreamExt;
use tui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

use crate::{error::Result, tui::command_helper::MINIMUM_TUI_WIDTH};

const TITLE: &str = "Modify Prompt";
const TITLE_STYLE: Style = Style::new()
    .fg(Color::LightGreen)
    .add_modifier(Modifier::BOLD);
const HINT: &str = "enter: confirm | esc: quit";
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const COMMAND_TITLE: &str = "Command";
const COMMAND_TITLE_STYLE: Style = Style::new().fg(Color::LightBlue);
const COMMAND_BORDER_STYLE: Style = Style::new().fg(Color::Blue);
const INPUT_BORDER_STYLE: Style = Style::new().fg(Color::Gray);
const INPUT_STYLE: Style = Style::new();

#[derive(Debug)]
pub struct App {
    terminal: Terminal<CrosstermBackend<Stderr>>,
    widget: AppWidget,
}

#[derive(Debug)]
struct AppWidget {
    command: String,
    text_area: TextArea<'static>,
}

#[derive(Debug)]
enum AppEvent {
    Quit,
    Confirm,
    Key(Event),
    Err(io::Error),
}

// ----------

impl Widget for &AppWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let [area] =
            Layout::horizontal([Constraint::Length((self.command.width_cjk() + 3).clamp(
                (HINT.width_cjk() + 3).max(MINIMUM_TUI_WIDTH),
                area.width as usize,
            ) as u16)])
            .areas(area);
        let [command_block_area, input_area, hint_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
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
        Line::from(self.command.clone()).render(command_area, buf);
        self.text_area.render(input_area, buf);
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.terminal.clear().ok();
        self.terminal.show_cursor().ok();
        crossterm::terminal::disable_raw_mode().ok();
    }
}

impl App {
    pub fn new(command: String) -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let backend: CrosstermBackend<Stderr> = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(
            backend,
            ratatui::TerminalOptions {
                viewport: Viewport::Inline(7),
            },
        )?;
        let mut text_area = TextArea::default();
        text_area.set_block(
            Block::bordered()
                .title_top("")
                .title_top(Line::from(TITLE).style(TITLE_STYLE))
                .border_style(INPUT_BORDER_STYLE)
                .border_type(BorderType::Rounded),
        );
        text_area.set_style(INPUT_STYLE);
        let widget = AppWidget { command, text_area };
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
                        KeyCode::Enter => {
                            send!(AppEvent::Confirm)
                        }
                        _ => send!(AppEvent::Key(evt.unwrap())),
                    },
                    Err(e) => send!(AppEvent::Err(e)),
                    _ => (),
                }
            }
        })
    }

    fn handle_key(&mut self, key_evt: Event) {
        self.widget.text_area.input(key_evt);
    }

    async fn run(mut self) -> Result<Option<String>> {
        let (tx, mut rx) = unbounded_channel();
        let handle = self.start_handle_events(tx);
        let rst = loop {
            if let Err(e) = self.terminal.draw(|frame| {
                frame.render_widget(&self.widget, frame.area());
            }) {
                break Err(e);
            }
            let Some(evt) = rx.recv().await else {
                break Ok(None);
            };
            match evt {
                AppEvent::Quit => break Ok(None),
                AppEvent::Confirm => {
                    break Ok(Some(self.widget.text_area.lines().join("\n")));
                }
                AppEvent::Key(key_evt) => self.handle_key(key_evt),
                AppEvent::Err(e) => break Err(e),
            }
        };
        handle.abort();
        handle.await.ok();
        Ok(rst?)
    }

    pub async fn prompt(command: String) -> Result<Option<String>> {
        App::new(command)?.run().await
    }
}
