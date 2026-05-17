use std::{
    io,
    time::{Duration, Instant},
};

use crate::{
    agent::tools::CommandCandidate,
    error::{Error, Result},
    tui::{cmd::MINIMUM_TUI_WIDTH, terminal::InlineTerminal},
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Viewport, crossterm,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, List, ListItem, ListState, Padding, StatefulWidget, Widget},
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};
use tokio_stream::StreamExt;
use unicode_width::UnicodeWidthStr;

const TITLE: &str = "Select Command";
const TITLE_STYLE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
const HINT1: &str = "j/k: up/down | m: modify | c: copy";
const HINT2: &str = "e: execute | enter: place to input | q/esc: quit";
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const BORDER_STYLE: Style = Style::new().fg(Color::Blue);

fn max_text_width(value: &str) -> usize {
    value
        .lines()
        .map(UnicodeWidthStr::width_cjk)
        .max()
        .unwrap_or(0)
}

fn text_lines(value: &str) -> impl Iterator<Item = &str> {
    let mut lines = value.lines();
    let first = lines.next();
    first.into_iter().chain(lines)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Copy,
    Execute,
    Modify,
    PrintToInputBuffer,
}

struct AppWidget {
    items: Vec<CommandCandidate>,
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
    Quit,
    Enter,
    C,
    M,
    E,
    Err(io::Error),
}

impl AppWidget {
    #[inline]
    fn calc_width(&self) -> u16 {
        self.items
            .iter()
            .map(|item| max_text_width(&item.summary).max(max_text_width(&item.command)) + 5)
            .max()
            .unwrap_or(0)
            .max(TITLE.width_cjk() + 6)
            .max(HINT1.width_cjk() + 6)
            .max(HINT2.width_cjk() + 6)
            .max(MINIMUM_TUI_WIDTH) as u16
    }

    fn current(&self) -> Option<&CommandCandidate> {
        self.list_state
            .selected()
            .and_then(|index| self.items.get(index))
    }

    fn should_line_break(&self, width: usize, gap: usize, command: &str, summary: &str) -> bool {
        if command.lines().count() + summary.lines().count() > 2 {
            return true;
        }
        command.width_cjk() + summary.width_cjk() + gap > width
    }

    fn item_height(&self, width: usize, item: &CommandCandidate) -> usize {
        if self.should_line_break(width, 2, &item.command, &item.summary) {
            text_lines(&item.command).count().max(1) + text_lines(&item.summary).count()
        } else {
            1
        }
    }

    fn render_item(&self, index: usize, item: &CommandCandidate, width: usize) -> Text<'static> {
        let selected = self.list_state.selected() == Some(index);
        let prefix = if selected { "> " } else { "  " };
        let prefix = Span::styled(prefix, Style::new().fg(Color::LightCyan));
        if self.should_line_break(width, 2, &item.command, &item.summary) {
            let command_style = if selected {
                Style::new().fg(Color::LightCyan)
            } else {
                Style::new()
            };
            let mut lines = text_lines(&item.command)
                .enumerate()
                .map(|(line_index, line)| {
                    if line_index == 0 {
                        Line::from(vec![
                            prefix.clone(),
                            Span::styled(line.to_string(), command_style),
                        ])
                    } else {
                        Line::from(format!("  {line}")).style(command_style)
                    }
                })
                .collect::<Vec<_>>();
            lines.extend(text_lines(&item.summary).map(|line| {
                Line::from(line.to_string())
                    .alignment(Alignment::Right)
                    .style(Style::new().fg(Color::DarkGray))
            }));
            Text::from(lines)
        } else {
            let command = if selected {
                Span::styled(item.command.clone(), Style::new().fg(Color::LightCyan))
            } else {
                Span::raw(item.command.clone())
            };
            let summary = Span::styled(item.summary.clone(), Style::new().fg(Color::DarkGray));
            let spaces =
                Span::raw(" ".repeat(
                    width.saturating_sub(item.command.width_cjk() + item.summary.width_cjk()),
                ));
            Text::from(Line::from_iter([prefix, command, spaces, summary]))
        }
    }
}

impl Widget for &mut AppWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let width = self.calc_width();
        let [block_area] = Layout::horizontal([Constraint::Length(width)]).areas(area);
        let block = Block::bordered()
            .padding(Padding::horizontal(1))
            .border_style(BORDER_STYLE)
            .border_type(BorderType::Rounded)
            .title_top("")
            .title_top(Line::from(TITLE).style(TITLE_STYLE));
        block.render(block_area, buf);

        let [list_area, hint1_area, hint2_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .margin(1)
        .areas(block_area);
        let [hint1_area] = Layout::horizontal([Constraint::Fill(1)])
            .horizontal_margin(1)
            .areas(hint1_area);
        let [hint2_area] = Layout::horizontal([Constraint::Fill(1)])
            .horizontal_margin(1)
            .areas(hint2_area);

        StatefulWidget::render(
            List::new(
                self.items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        ListItem::from(self.render_item(index, item, list_area.width as usize - 2))
                    })
                    .collect::<Vec<_>>(),
            ),
            list_area,
            buf,
            &mut self.list_state,
        );

        Line::from(HINT1)
            .right_aligned()
            .style(HINT_STYLE)
            .render(hint1_area, buf);
        Line::from(HINT2)
            .right_aligned()
            .style(HINT_STYLE)
            .render(hint2_area, buf);
    }
}

impl App {
    fn start_handling_events(&self, tx: UnboundedSender<AppEvent>) -> JoinHandle<()> {
        macro_rules! send {
            ($value:expr) => {
                if tx.send($value).is_err() {
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
                    Ok(Event::Key(kevt))
                        if matches!(kevt.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        match kevt.code {
                            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w')
                                if kevt.modifiers.is_empty() =>
                            {
                                send!(AppEvent::Up);
                            }
                            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s')
                                if kevt.modifiers.is_empty() =>
                            {
                                send!(AppEvent::Down);
                            }
                            KeyCode::Esc | KeyCode::Char('q') if kevt.modifiers.is_empty() => {
                                send!(AppEvent::Quit);
                            }
                            KeyCode::Char('c') if kevt.modifiers == KeyModifiers::CONTROL => {
                                send!(AppEvent::Quit);
                            }
                            KeyCode::Char('c') if kevt.modifiers.is_empty() => {
                                send!(AppEvent::C);
                            }
                            KeyCode::Char('m') if kevt.modifiers.is_empty() => {
                                send!(AppEvent::M);
                            }
                            KeyCode::Char('e') if kevt.modifiers.is_empty() => {
                                send!(AppEvent::E);
                            }
                            KeyCode::Enter if kevt.modifiers.is_empty() => {
                                if start_time.elapsed() > skip_enter_duration {
                                    send!(AppEvent::Enter);
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(error) => {
                        send!(AppEvent::Err(error));
                    }
                    _ => {}
                }
            }
        })
    }

    fn action_result(&self, kind: ActionKind) -> Option<Action> {
        self.widget.current().map(|current| Action {
            command: current.command.clone(),
            kind,
        })
    }

    async fn run(mut self) -> Result<Option<Action>> {
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = self.start_handling_events(evt_tx);
        let rst = loop {
            self.terminal
                .draw(|frame| frame.render_widget(&mut self.widget, frame.area()))?;
            let Some(evt) = evt_rx.recv().await else {
                break Ok(None);
            };
            match evt {
                AppEvent::Up => self.widget.list_state.select_previous(),
                AppEvent::Down => self.widget.list_state.select_next(),
                AppEvent::Quit => break Ok(None),
                AppEvent::Enter => break Ok(self.action_result(ActionKind::PrintToInputBuffer)),
                AppEvent::C => break Ok(self.action_result(ActionKind::Copy)),
                AppEvent::M => break Ok(self.action_result(ActionKind::Modify)),
                AppEvent::E => break Ok(self.action_result(ActionKind::Execute)),
                AppEvent::Err(error) => break Err(error),
            }
        };
        handle.abort();
        handle.await.ok();
        Ok(rst?)
    }

    fn new(items: Vec<CommandCandidate>, _reply_markdown: String) -> io::Result<Self> {
        let mut list_state = ListState::default();
        list_state.select_first();
        let widget = AppWidget { items, list_state };
        let viewport_height = 4 + widget
            .items
            .iter()
            .map(|item| widget.item_height(widget.calc_width() as usize - 2, item))
            .sum::<usize>() as u16;
        let terminal = InlineTerminal::init_inline_with_options(ratatui::TerminalOptions {
            viewport: Viewport::Inline(viewport_height),
        })?;
        Ok(Self { terminal, widget })
    }

    pub async fn select(
        items: Vec<CommandCandidate>,
        reply_markdown: String,
    ) -> Result<Option<Action>> {
        if items.is_empty() {
            return Err(Error::InvalidInput("items can't be empty".into()));
        }
        App::new(items, reply_markdown)?.run().await
    }
}

#[cfg(test)]
mod tests {
    use super::{AppWidget, max_text_width};
    use crate::agent::tools::CommandCandidate;
    use ratatui::widgets::ListState;

    #[test]
    fn max_text_width_uses_widest_line() {
        assert_eq!(max_text_width("a\nabcd\nab"), 4);
    }

    #[test]
    fn multiline_item_height_counts_command_and_summary_lines() {
        let widget = AppWidget {
            items: Vec::new(),
            list_state: ListState::default(),
        };
        let item = CommandCandidate {
            command: "git status\ngit diff --stat".into(),
            summary: "show current state".into(),
        };

        assert_eq!(widget.item_height(60, &item), 3);
    }

    #[test]
    fn multiline_command_forces_line_break() {
        let widget = AppWidget {
            items: Vec::new(),
            list_state: ListState::default(),
        };

        assert!(widget.should_line_break(80, 2, "git status\ngit diff", "summary"));
    }
}
