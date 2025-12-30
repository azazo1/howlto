use std::{
    io::{self, Stderr},
    ops::{Deref, DerefMut},
};

use ratatui::{Terminal, TerminalOptions, prelude::CrosstermBackend};

type AppTerminal = Terminal<CrosstermBackend<Stderr>>;

#[derive(Debug)]
pub(crate) struct InlineTerminal {
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
        // ratatui::restore(); // ratatui::restore() 对 Inline 的恢复效果不好.
        self.terminal.clear().ok();
        self.terminal.show_cursor().ok();
        crossterm::terminal::disable_raw_mode().ok();
    }
}

impl InlineTerminal {
    pub(crate) fn init_with_options(options: TerminalOptions) -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let backend: CrosstermBackend<Stderr> = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(backend, options)?;
        Ok(Self { terminal })
    }
}
