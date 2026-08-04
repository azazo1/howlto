use std::io;

use crate::{
    error::Result,
    session::SessionSummary,
    tui::{command_helper::MINIMUM_TUI_WIDTH, terminal::InlineTerminal},
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    TerminalOptions, Viewport,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, List, ListItem, ListState, Padding, StatefulWidget, Widget},
};
use tokio::{
    sync::mpsc::UnboundedSender,
    task::JoinHandle,
};
use tokio_stream::StreamExt;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

const TITLE: &str = "Howlto Session";
const TITLE_STYLE: Style = Style::new().fg(Color::LightGreen).add_modifier(Modifier::BOLD);
const HINT: &str = "j/k: up/down | enter: choose | q/esc: quit";
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const BORDER_STYLE: Style = Style::new().fg(Color::Blue);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuChoice {
    New,
    Session(Uuid),
}

struct AppWidget {
    choices: Vec<MenuChoice>,
    labels: Vec<String>,
    list_state: ListState,
}

pub struct App {
    terminal: InlineTerminal,
    widget: AppWidget,
}

#[derive(Debug)]
enum AppEvent {
    Up,
    Down,
    Choose,
    Quit,
    Err(io::Error),
}

fn summary_label(summary: &SessionSummary) -> String {
    let prompt = summary.last_prompt.lines().next().unwrap_or("").trim();
    let detail = if prompt.is_empty() {
        summary.final_text.lines().next().unwrap_or("").trim()
    } else {
        prompt
    };
    let detail = if detail.chars().count() > 40 {
        detail.chars().take(37).collect::<String>() + "..."
    } else {
        detail.to_string()
    };
    format!(
        "{} | {} | {} command(s)",
        format_updated_at(summary.updated_at),
        if detail.is_empty() { "(empty)" } else { &detail },
        summary.command_count
    )
}

fn format_updated_at(seconds: i64) -> String {
    let Ok(datetime) = time::OffsetDateTime::from_unix_timestamp(seconds) else {
        return "unknown".into();
    };
    let Ok(rendered) = datetime.format(&time::format_description::well_known::Rfc3339) else {
        return "unknown".into();
    };
    rendered
}

impl AppWidget {
    fn calc_height(&self) -> u16 {
        (self.choices.len() + 4).min(u16::MAX as usize) as u16
    }

    fn calc_width(&self) -> u16 {
        self.labels
            .iter()
            .map(|label| UnicodeWidthStr::width_cjk(label.as_str()) + 6)
            .max()
            .unwrap_or(0)
            .max(TITLE.width_cjk() + 6)
            .max(HINT.width_cjk() + 6)
            .max(MINIMUM_TUI_WIDTH) as u16
    }
}

impl Widget for &mut AppWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [block_area] = Layout::horizontal([Constraint::Length(self.calc_width())]).areas(area);
        Block::bordered()
            .padding(Padding::horizontal(1))
            .border_style(BORDER_STYLE)
            .border_type(BorderType::Rounded)
            .title_top("")
            .title_top(Line::from(TITLE).style(TITLE_STYLE))
            .render(block_area, buf);
        let [list_area, hint_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .margin(1)
        .areas(block_area);
        let [hint_area] = Layout::horizontal([Constraint::Fill(1)])
            .horizontal_margin(1)
            .areas(hint_area);
        let items = self
            .labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let selected = self.list_state.selected() == Some(index);
                let prefix = if selected { "> " } else { "  " };
                let style = if selected {
                    Style::new().fg(Color::LightCyan)
                } else {
                    Style::new()
                };
                ListItem::new(Line::from_iter([
                    Span::from(prefix).fg(Color::LightCyan),
                    Span::styled(label.to_string(), style),
                ]))
            })
            .collect::<Vec<_>>();
        StatefulWidget::render(List::new(items), list_area, buf, &mut self.list_state);
        Line::from(HINT)
            .right_aligned()
            .style(HINT_STYLE)
            .render(hint_area, buf);
    }
}

impl App {
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
                    Ok(Event::Key(key))
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                                send!(AppEvent::Up)
                            }
                            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                                send!(AppEvent::Down)
                            }
                            KeyCode::Enter if key.modifiers.is_empty() => send!(AppEvent::Choose),
                            KeyCode::Esc | KeyCode::Char('q') if key.modifiers.is_empty() => {
                                send!(AppEvent::Quit)
                            }
                            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                                send!(AppEvent::Quit)
                            }
                            _ => (),
                        }
                    }
                    Err(error) => send!(AppEvent::Err(error)),
                    _ => (),
                }
            }
        })
    }

    fn choice(&self) -> Option<MenuChoice> {
        let selected = self.widget.list_state.selected()?;
        self.widget.choices.get(selected).copied()
    }

    async fn run(mut self) -> Result<Option<MenuChoice>> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = self.start_handling_events(tx);
        let result = loop {
            self.terminal.draw(|frame| {
                frame.render_widget(&mut self.widget, frame.area());
            })?;
            let Some(event) = rx.recv().await else {
                break Ok(None);
            };
            match event {
                AppEvent::Up => self.widget.list_state.select_previous(),
                AppEvent::Down => self.widget.list_state.select_next(),
                AppEvent::Choose => break Ok(self.choice()),
                AppEvent::Quit => break Ok(None),
                AppEvent::Err(error) => break Err(error.into()),
            }
        };
        handle.abort();
        handle.await.ok();
        result
    }

    fn new(sessions: Vec<SessionSummary>) -> io::Result<Self> {
        let mut choices = vec![MenuChoice::New];
        let mut labels = vec!["New session".to_string()];
        for summary in sessions {
            labels.push(summary_label(&summary));
            choices.push(MenuChoice::Session(summary.id));
        }
        let mut list_state = ListState::default();
        list_state.select_first();
        let widget = AppWidget {
            choices,
            labels,
            list_state,
        };
        let terminal = InlineTerminal::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(widget.calc_height()),
        })?;
        Ok(Self { terminal, widget })
    }
}

pub async fn choose(sessions: Vec<SessionSummary>) -> Result<Option<MenuChoice>> {
    if sessions.is_empty() {
        return Ok(Some(MenuChoice::New));
    }
    App::new(sessions)?.run().await
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    #[ignore = "需要真实 TTY 交互 (手动选择), 用 `cargo test chatter_menu -- --ignored --nocapture` 运行"]
    async fn chatter_menu_manual_selection() {
        let sessions = vec![SessionSummary {
            id: Uuid::new_v4(),
            updated_at: 0,
            last_prompt: "list files".into(),
            final_text: "Use rg --files.".into(),
            command_count: 1,
        }];

        let choice = choose(sessions).await.unwrap();
        assert!(choice.is_some());
    }
}
