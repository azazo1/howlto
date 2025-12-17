use std::io::{self, Stderr};

use crate::{
    error::{Error, Result},
    tui::command_helper::MINIMUM_TUI_WIDTH,
};
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::{
    Terminal, Viewport, crossterm,
    layout::{Constraint, Layout},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, List, ListItem, ListState, Padding, StatefulWidget, Widget},
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};
use tokio_stream::StreamExt;

const TITLE: &str = "Select Command";
const TITLE_STYLE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
const HINT1: &str = "j/k: up/down | m: modify | c: copy";
const HINT2: &str = "e: execute | enter: print | q/esc: quit";
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const BORDER_STYLE: Style = Style::new().fg(Color::Blue);

#[derive(Debug, Clone)]
pub struct Item {
    pub command: String,
}

impl<T> From<T> for Item
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self {
            command: value.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Action {
    pub command: String,
    pub kind: ActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Copy,
    Execute,
    Modify,
    Print,
}

struct AppWidget {
    items: Vec<Item>,
    list_state: ListState,
}

pub struct App {
    terminal: Terminal<CrosstermBackend<Stderr>>,
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
    // todo 添加一个 tab 直接粘贴到下一个 shell 输入中, 可能需要 shell 集成脚本.
}

impl Drop for App {
    fn drop(&mut self) {
        // ratatui::restore(); // ratatui::restore() 对 Inline 的恢复效果不好.
        self.terminal.clear().ok();
        self.terminal.show_cursor().ok();
        crossterm::terminal::disable_raw_mode().ok();
    }
}

impl Widget for &mut AppWidget {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let width = self
            .items
            .iter()
            .max_by_key(|x| x.command.len())
            .map(|x| x.command.len())
            .unwrap_or(0)
            .max(HINT1.len() + 5)
            .max(HINT2.len() + 5)
            .max(MINIMUM_TUI_WIDTH);
        let [block_area] = Layout::horizontal([Constraint::Length(width as u16)]).areas(area);
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
                    .map(|x| {
                        let mut inserted = x.command.clone();
                        inserted.insert(0, ' ');
                        inserted
                    })
                    .map(ListItem::from),
            )
            .highlight_symbol(">")
            .highlight_style(Style::new().fg(Color::LightCyan)),
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
        macro_rules! break_on_error {
            ($s:expr) => {
                if $s.is_err() {
                    break;
                }
            };
        }
        tokio::spawn(async move {
            let mut event_stream = crossterm::event::EventStream::new();
            while let Some(evt) = event_stream.next().await {
                match evt {
                    Ok(Event::Key(kevt)) => match kevt.code {
                        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w')
                            if kevt.modifiers.is_empty() =>
                        {
                            break_on_error!(tx.send(AppEvent::Up));
                        }
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s')
                            if kevt.modifiers.is_empty() =>
                        {
                            break_on_error!(tx.send(AppEvent::Down));
                        }
                        KeyCode::Esc | KeyCode::Char('q') if kevt.modifiers.is_empty() => {
                            break_on_error!(tx.send(AppEvent::Quit));
                        }
                        KeyCode::Char('c') if kevt.modifiers == KeyModifiers::CONTROL => {
                            break_on_error!(tx.send(AppEvent::Quit));
                        }
                        KeyCode::Char('c') if kevt.modifiers.is_empty() => {
                            break_on_error!(tx.send(AppEvent::C));
                        }
                        KeyCode::Char('m') if kevt.modifiers.is_empty() => {
                            break_on_error!(tx.send(AppEvent::M));
                        }
                        KeyCode::Char('e') if kevt.modifiers.is_empty() => {
                            break_on_error!(tx.send(AppEvent::E));
                        }
                        KeyCode::Enter if kevt.modifiers.is_empty() => {
                            break_on_error!(tx.send(AppEvent::Enter));
                        }
                        _ => (),
                    },
                    Err(e) => {
                        break_on_error!(tx.send(AppEvent::Err(e)));
                    }
                    _ => {}
                }
            }
        })
    }

    fn action_result(&self, kind: ActionKind) -> Option<Action> {
        if let Some(sel) = self.widget.list_state.selected() {
            let command = self.widget.items.get(sel).map(|x| x.command.clone());
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
                AppEvent::Enter => break Ok(self.action_result(ActionKind::Print)),
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

    fn new(items: Vec<Item>) -> io::Result<App> {
        crossterm::terminal::enable_raw_mode()?;
        let backend: CrosstermBackend<Stderr> = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(
            backend,
            ratatui::TerminalOptions {
                // (border:2) + (hints:2)+ (items.len())
                viewport: Viewport::Inline(4 + items.len() as u16),
            },
        )?;
        // todo 使用 tty 作为标准输入流, 然后 drop 的时候还原, 防止标准输入流的错误输入.
        let mut list_state = ListState::default();
        list_state.select_first();
        Ok(App {
            terminal,
            widget: AppWidget { items, list_state },
        })
    }

    pub async fn select(items: Vec<impl Into<Item>>) -> Result<Option<Action>> {
        if items.is_empty() {
            return Err(Error::InvalidInput("items can't be empty".into()));
        }
        let app = App::new(items.into_iter().map(Into::into).collect())?;
        app.run().await
    }
}
