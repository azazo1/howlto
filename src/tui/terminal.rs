use std::{
    io::{self, Stderr},
    ops::{Deref, DerefMut},
};

use ratatui::{
    Terminal, TerminalOptions,
    backend::{Backend, ClearType},
    layout::Position,
    prelude::CrosstermBackend,
    Viewport,
};

type AppTerminal = Terminal<CrosstermBackend<Stderr>>;

#[derive(Debug)]
pub(crate) struct InlineTerminal {
    closed: bool,
    terminal: AppTerminal,
}

impl Deref for InlineTerminal {
    type Target = AppTerminal;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for InlineTerminal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for InlineTerminal {
    fn drop(&mut self) {
        self.close();
    }
}

impl InlineTerminal {
    pub(crate) fn init_with_options(options: TerminalOptions) -> io::Result<Self> {
        let options = Self::fit_options_to_terminal(options)?;
        crossterm::terminal::enable_raw_mode()?;
        let backend: CrosstermBackend<Stderr> = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(backend, options)?;
        Ok(Self {
            terminal,
            closed: false,
        })
    }

    fn fit_options_to_terminal(options: TerminalOptions) -> io::Result<TerminalOptions> {
        let terminal_height = crossterm::terminal::size()?.1;
        Ok(Self::fit_options_to_height(options, terminal_height))
    }

    fn fit_options_to_height(mut options: TerminalOptions, terminal_height: u16) -> TerminalOptions {
        if let Viewport::Inline(height) = options.viewport
            && terminal_height > 0
        {
            options.viewport = Viewport::Inline(height.min(terminal_height));
        }
        options
    }

    pub(crate) fn close(&mut self) {
        if self.closed {
            return;
        }
        let area = self.terminal.current_buffer_mut().area;
        let backend = self.terminal.backend_mut();
        backend
            .set_cursor_position(Position {
                x: area.x,
                y: area.y,
            })
            .ok();
        backend.clear_region(ClearType::AfterCursor).ok();
        backend.show_cursor().ok();
        backend.flush().ok();
        crossterm::terminal::disable_raw_mode().ok();
        self.closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_viewport_height_is_clamped_to_terminal_height() {
        let options = InlineTerminal::fit_options_to_height(
            TerminalOptions {
                viewport: Viewport::Inline(200),
            },
            24,
        );

        assert_eq!(options.viewport, Viewport::Inline(24));
    }

    #[test]
    fn inline_viewport_height_keeps_smaller_request() {
        let options = InlineTerminal::fit_options_to_height(
            TerminalOptions {
                viewport: Viewport::Inline(10),
            },
            24,
        );

        assert_eq!(options.viewport, Viewport::Inline(10));
    }
}
