use std::io::Stdout;

use crate::error::{Error, Result};
use crossterm::event::{Event, KeyCode};
use ratatui::{
    Terminal, Viewport, crossterm,
    layout::{Alignment, Constraint, Layout},
    prelude::{Backend, CrosstermBackend},
    style::{Color, Style},
    widgets::{Block, List, ListItem, ListState, Padding, StatefulWidget, Widget},
};
use tokio_stream::StreamExt;

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

pub enum ActionKind {
    Copy,
    Execute,
    Modify,
}

struct CommandSelectWidget {
    items: Vec<Item>,
    list_state: ListState,
}

pub struct CommandSelectApp<B>
where
    B: Backend,
{
    terminal: Terminal<B>,
    widget: CommandSelectWidget,
    should_exit: bool,
}

impl<B> Drop for CommandSelectApp<B>
where
    B: Backend,
{
    fn drop(&mut self) {
        ratatui::restore();
    }
}

impl Widget for &mut CommandSelectWidget {
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
            .max(50);
        let [list_area, _rest_area] =
            Layout::horizontal([Constraint::Length(width as u16), Constraint::Fill(1)]).areas(area);
        let block = Block::bordered()
            .padding(Padding::horizontal(1))
            .title("j/k: up/down | enter: execute | m: modify | c: copy | q/esc: Quit") // todo 实现这几个功能
            .title_alignment(Alignment::Left);
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
            .block(block)
            .highlight_symbol(">")
            .highlight_style(Style::new().fg(Color::LightCyan)),
            list_area,
            buf,
            &mut self.list_state,
        );
    }
}

impl<B> CommandSelectApp<B>
where
    B: Backend,
{
    async fn run(mut self) -> Result<Option<String>> {
        let mut event_stream = crossterm::event::EventStream::new();
        while !self.should_exit {
            self.terminal.draw(|frame| {
                frame.render_widget(&mut self.widget, frame.area());
            })?;
            match event_stream.next().await {
                Some(Ok(evt)) => {
                    if let Event::Key(kevt) = evt {
                        match kevt.code {
                            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') => {
                                self.widget.list_state.select_previous();
                            }
                            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') => {
                                self.widget.list_state.select_next();
                            }
                            KeyCode::Esc | KeyCode::Char('q') => self.should_exit = true,
                            _ => (),
                        }
                    }
                }
                Some(Err(e)) => Err(e)?,
                None => self.should_exit = true,
            }
        }
        if let Some(sel) = self.widget.list_state.selected() {
            Ok(self.widget.items.get(sel).map(|x| x.command.clone()))
        } else {
            Ok(None)
        }
    }
}

impl CommandSelectApp<CrosstermBackend<Stdout>> {
    fn new(items: Vec<Item>) -> CommandSelectApp<CrosstermBackend<Stdout>> {
        let terminal = ratatui::init_with_options(ratatui::TerminalOptions {
            viewport: Viewport::Inline(2 + items.len() as u16),
        });
        let list_state = ListState::default();
        CommandSelectApp {
            terminal,
            widget: CommandSelectWidget { items, list_state },
            should_exit: false,
        }
    }

    pub async fn select(items: Vec<impl Into<Item>>) -> Result<Option<String>> {
        if items.is_empty() {
            return Err(Error::InvalidInput("items can't be empty".into()));
        }
        let app = CommandSelectApp::new(items.into_iter().map(Into::into).collect());
        app.run().await
    }
}
