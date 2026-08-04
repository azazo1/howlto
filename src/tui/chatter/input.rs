use std::io;

use crate::{
    error::Result,
    tui::{command_helper::MINIMUM_TUI_WIDTH, terminal::InlineTerminal},
};
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    TerminalOptions, Viewport,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType},
};
use ratatui_textarea::TextArea;
use tokio::{
    sync::mpsc::{UnboundedSender, unbounded_channel},
    task::JoinHandle,
};
use tokio_stream::StreamExt;
use unicode_width::UnicodeWidthStr;

const TITLE: &str = "Howlto Prompt";
const TITLE_STYLE: Style = Style::new()
    .fg(Color::LightGreen)
    .add_modifier(Modifier::BOLD);
const HINT: &str = "enter: send | ctrl+c/esc: quit | /exit: quit";
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const INPUT_BORDER_STYLE: Style = Style::new().fg(Color::Blue);

pub struct App {
    terminal: InlineTerminal,
    widget: AppWidget,
}

struct AppWidget {
    text_area: TextArea<'static>,
}

#[derive(Debug)]
enum AppEvent {
    Quit,
    Confirm,
    Key(Event),
    Err(io::Error),
}

impl Widget for &mut AppWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [area] = Layout::horizontal([Constraint::Length(
            (HINT.width_cjk() + 4).max(MINIMUM_TUI_WIDTH) as u16,
        )])
        .areas(area);
        let [input_area, hint_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).areas(area);
        let [hint_area] = Layout::horizontal([Constraint::Fill(1)])
            .horizontal_margin(1)
            .areas(hint_area);
        Line::from(HINT)
            .style(HINT_STYLE)
            .right_aligned()
            .render(hint_area, buf);
        self.text_area.render(input_area, buf);
    }
}

impl App {
    pub fn new() -> Result<Self> {
        let terminal = InlineTerminal::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(4),
        })?;
        let mut text_area = TextArea::default();
        text_area.set_block(
            Block::bordered()
                .title_top("")
                .title_top(Line::from(TITLE).style(TITLE_STYLE))
                .border_style(INPUT_BORDER_STYLE)
                .border_type(BorderType::Rounded),
        );
        Ok(Self {
            terminal,
            widget: AppWidget { text_area },
        })
    }

    fn start_handling_events(&self, tx: UnboundedSender<AppEvent>) -> JoinHandle<()> {
        macro_rules! send {
            ($s:expr) => {
                if tx.send($s).is_err() {
                    break;
                }
            };
        }
        tokio::spawn(async move {
            let mut event_stream = crossterm::event::EventStream::new();
            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(Event::Key(key)) => match key.code {
                        KeyCode::Char('c') | KeyCode::Char('d')
                            if key.modifiers == KeyModifiers::CONTROL =>
                        {
                            send!(AppEvent::Quit)
                        }
                        KeyCode::Esc => send!(AppEvent::Quit),
                        KeyCode::Enter if key.modifiers.is_empty() => send!(AppEvent::Confirm),
                        _ => send!(AppEvent::Key(event.unwrap())),
                    },
                    Err(error) => send!(AppEvent::Err(error)),
                    _ => (),
                }
            }
        })
    }

    async fn run(mut self) -> Result<Option<String>> {
        let (tx, mut rx) = unbounded_channel();
        let handle = self.start_handling_events(tx);
        let result = loop {
            self.terminal.draw(|frame| {
                frame.render_widget(&mut self.widget, frame.area());
            })?;
            let Some(event) = rx.recv().await else {
                break Ok(None);
            };
            match event {
                AppEvent::Quit => break Ok(None),
                AppEvent::Confirm => break Ok(Some(self.widget.text_area.lines().join("\n"))),
                AppEvent::Key(key) => {
                    self.widget.text_area.input(key);
                }
                AppEvent::Err(error) => break Err(error.into()),
            }
        };
        handle.abort();
        handle.await.ok();
        result
    }

    pub async fn prompt() -> Result<Option<String>> {
        App::new()?.run().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "需要真实 TTY 交互 (手动输入), 用 `cargo test chatter_input -- --ignored --nocapture` 运行"]
    async fn chatter_input_manual_prompt() {
        let prompt = App::prompt().await.unwrap();
        assert!(prompt.is_some());
    }
}
