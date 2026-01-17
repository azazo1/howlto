use std::{
    io,
    time::{Duration, Instant},
};

use crate::{
    agent::tools::FinishResponseItem,
    error::{Error, Result},
    tui::{command_helper::MINIMUM_TUI_WIDTH, terminal::InlineTerminal},
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Viewport, crossterm,
    layout::{Constraint, Layout},
    prelude::*,
    style::{Color, Modifier, Style},
    text::Line,
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
    items: Vec<FinishResponseItem>,
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
    #[must_use]
    fn calc_width(&self) -> u16 {
        self.items
            .iter()
            .map(|x| x.desc.width_cjk().max(x.content.width_cjk()) + 5)
            .max()
            .unwrap_or(0)
            .max(TITLE.width_cjk() + 5)
            .max(HINT1.width_cjk() + 5)
            .max(HINT2.width_cjk() + 5)
            .max(MINIMUM_TUI_WIDTH) as u16
    }

    /// 判断 命令-描述 对是否需要分行.
    /// # Parameters
    /// - `width`: 渲染宽度.
    /// - `gap`: content 和 desc 之间的最小间隔.
    /// - `content`, `desc`: 命令-描述 对.
    fn should_line_break(&self, width: usize, gap: usize, content: &str, desc: &str) -> bool {
        // 只有两个都是单行的时候才能不分行.
        if content.lines().count() + desc.lines().count() > 2 {
            return true;
        }
        content.width_cjk() + desc.width_cjk() + gap > width
    }
}

impl Widget for &mut AppWidget {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let width = self.calc_width();
        let [block_area] = Layout::horizontal([Constraint::Length(width)]).areas(area);
        let block = Block::bordered()
            .padding(Padding::horizontal(1))
            .border_style(BORDER_STYLE)
            .border_type(BorderType::Rounded)
            .title_top("") // 添加一个空占位, 让 title 不至于在最左侧.
            .title_top(Line::from(TITLE).style(TITLE_STYLE));
        block.render(block_area, buf);
        let [list_area, hint1_area, hint2_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .margin(1)
        .areas(block_area);
        let (hint1_area, hint2_area) = {
            // right margin
            let layout = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)]);
            (layout.split(hint1_area)[0], layout.split(hint2_area)[0])
        };
        StatefulWidget::render(
            List::new(
                self.items
                    .iter()
                    .enumerate()
                    .map(|(idx, x)| {
                        let width = list_area.width;
                        let selected = if let Some(selected) = self.list_state.selected()
                            && selected.clamp(0, self.items.len() - 1) == idx
                        {
                            true
                        } else {
                            false
                        };
                        let prefix = if selected {
                            Span::from("> ")
                        } else {
                            Span::from("  ")
                        }
                        .fg(Color::LightCyan);
                        let content = x.content.as_str();
                        let desc = x.desc.as_str();
                        if self.should_line_break(width as usize - 2, 2, content, desc) {
                            let mut text = if selected {
                                Text::from(prefix + content.into()).fg(Color::LightCyan)
                            } else {
                                Text::from(prefix + content.into())
                            };
                            text.extend(desc.lines().map(|line| {
                                Line::from(line).alignment(Alignment::Right).dark_gray()
                            }));
                            text
                        } else {
                            let left = if selected {
                                Span::from(content).fg(Color::LightCyan)
                            } else {
                                content.into()
                            };
                            let right = Span::from(desc).dark_gray();
                            // 分别左右对齐.
                            let spaces = Span::raw(
                                " ".repeat(
                                    ((width - 2) as usize)
                                        .saturating_sub(content.width_cjk() + desc.width_cjk()),
                                ),
                            );
                            Text::from(Line::from_iter([prefix, left, spaces, right]))
                        }
                    })
                    .map(ListItem::from),
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
            ($s:expr) => {
                if tx.send($s).is_err() {
                    break;
                }
            };
        }
        // 在 windows 某些终端中会将执行命令的回车键也监听到, 因此忽略这个事件.
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
                            _ => (),
                        }
                    }
                    Err(e) => {
                        send!(AppEvent::Err(e));
                    }
                    _ => {}
                }
            }
        })
    }

    fn action_result(&self, kind: ActionKind) -> Option<Action> {
        if let Some(sel) = self.widget.list_state.selected() {
            let command = self.widget.items.get(sel).map(|x| x.content.clone());
            command.map(|command| Action { command, kind })
        } else {
            None
        }
    }

    async fn run(mut self) -> Result<Option<Action>> {
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = self.start_handling_events(evt_tx);
        let rst = loop {
            self.terminal.draw(|frame| {
                frame.render_widget(&mut self.widget, frame.area());
            })?;
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
                AppEvent::Err(e) => break Err(e),
                AppEvent::E => break Ok(self.action_result(ActionKind::Execute)),
            }
        };
        handle.abort();
        handle.await.ok();
        Ok(rst?)
    }

    fn new(items: Vec<FinishResponseItem>) -> io::Result<App> {
        let mut list_state = ListState::default();
        list_state.select_first();
        let widget = AppWidget { items, list_state };
        let terminal = InlineTerminal::init_with_options(ratatui::TerminalOptions {
            // (border:2) + (hints:2)+ (items.len())
            viewport: Viewport::Inline(
                4 + widget
                    .items
                    .iter()
                    .map(|x| {
                        if widget.should_line_break(
                            // -2: List widget 的高亮 symbol 宽度.
                            widget.calc_width() as usize - 2,
                            2,
                            x.content.as_str(),
                            x.desc.as_str(),
                        ) {
                            // 分行.
                            x.content.lines().count() + x.desc.lines().count()
                        } else {
                            // 不分行.
                            1
                        }
                    })
                    .sum::<usize>() as u16,
            ),
        })?;
        Ok(App { terminal, widget })
    }

    pub async fn select(items: Vec<FinishResponseItem>) -> Result<Option<Action>> {
        if items.is_empty() {
            return Err(Error::InvalidInput("items can't be empty".into()));
        }
        let app = App::new(items.into_iter().collect())?;
        app.run().await
    }
}
