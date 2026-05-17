use std::{
    io::{self, Stderr},
    ops::{Deref, DerefMut},
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, TerminalOptions, prelude::CrosstermBackend};

use crate::logging;

type AppTerminal = Terminal<CrosstermBackend<Stderr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalMode {
    Inline,
    Fullscreen,
}

#[derive(Debug)]
pub(crate) struct InlineTerminal {
    closed: bool,
    mode: TerminalMode,
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
    fn init_with_mode(options: TerminalOptions, mode: TerminalMode) -> io::Result<Self> {
        logging::enter_tui();
        if let Err(error) = crossterm::terminal::enable_raw_mode() {
            logging::exit_tui();
            return Err(error);
        }
        if matches!(mode, TerminalMode::Fullscreen) {
            let mut stderr = io::stderr();
            if let Err(error) = execute!(stderr, EnterAlternateScreen, EnableMouseCapture) {
                crossterm::terminal::disable_raw_mode().ok();
                logging::exit_tui();
                return Err(error);
            }
        }
        let backend: CrosstermBackend<Stderr> = CrosstermBackend::new(io::stderr());
        let terminal = match Terminal::with_options(backend, options) {
            Ok(terminal) => terminal,
            Err(error) => {
                if matches!(mode, TerminalMode::Fullscreen) {
                    execute!(io::stderr(), DisableMouseCapture, LeaveAlternateScreen).ok();
                }
                crossterm::terminal::disable_raw_mode().ok();
                logging::exit_tui();
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            closed: false,
            mode,
        })
    }

    pub(crate) fn init_inline_with_options(options: TerminalOptions) -> io::Result<Self> {
        Self::init_with_mode(options, TerminalMode::Inline)
    }

    pub(crate) fn init_fullscreen_with_options(options: TerminalOptions) -> io::Result<Self> {
        Self::init_with_mode(options, TerminalMode::Fullscreen)
    }

    pub(crate) fn close(&mut self) {
        if self.closed {
            return;
        }
        // ratatui::restore(); // ratatui::restore() 对 Inline 的恢复效果不好.
        self.terminal.clear().ok();
        self.terminal.show_cursor().ok();
        if matches!(self.mode, TerminalMode::Fullscreen) {
            execute!(io::stderr(), DisableMouseCapture, LeaveAlternateScreen).ok();
        }
        crossterm::terminal::disable_raw_mode().ok();
        logging::exit_tui();
        self.closed = true;
    }
}
