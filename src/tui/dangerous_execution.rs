use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Viewport, crossterm,
    layout::{Constraint, Layout},
    prelude::*,
    widgets::{Block, BorderType, Padding, Paragraph, Wrap},
};
use tracing::{info, warn};
use tui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

use crate::tui::terminal::InlineTerminal;

const TITLE: &str = "Confirm Execution";
const TITLE_STYLE: Style = Style::new()
    .fg(Color::LightRed)
    .add_modifier(Modifier::BOLD);
const BORDER_STYLE: Style = Style::new().fg(Color::Red);
const WARNING_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const HINT_STYLE: Style = Style::new().fg(Color::DarkGray);
const INPUT_BORDER_STYLE: Style = Style::new().fg(Color::Gray);
const INPUT_STYLE: Style = Style::new();
const MINIMUM_TUI_WIDTH: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Decision,
    InputReason,
}

#[derive(Debug)]
struct AppWidget {
    command: String,
    mode: Mode,
    reason_input: TextArea<'static>,
}

#[derive(Debug)]
struct App {
    terminal: InlineTerminal,
    widget: AppWidget,
}

#[derive(Debug)]
enum AppDecision {
    Approve,
    Reject(String),
}

impl AppWidget {
    fn calc_width(&self) -> u16 {
        self.command
            .lines()
            .map(UnicodeWidthStr::width_cjk)
            .max()
            .unwrap_or(0)
            .max(TITLE.width_cjk() + 6)
            .max("enter/y: approve | esc/n: reject | m: reject with reason".width_cjk() + 6)
            .max("Press enter to submit reject reason".width_cjk() + 6)
            .max(MINIMUM_TUI_WIDTH) as u16
    }

    fn calc_height(&self) -> u16 {
        match self.mode {
            Mode::Decision => 6,
            Mode::InputReason => 9,
        }
    }
}

impl Widget for &AppWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let [dialog_area] = Layout::horizontal([Constraint::Length(self.calc_width())]).areas(area);
        let block = Block::bordered()
            .title_top("")
            .title_top(Line::from(TITLE).style(TITLE_STYLE))
            .padding(Padding::horizontal(1))
            .border_type(BorderType::Rounded)
            .border_style(BORDER_STYLE);
        block.render(dialog_area, buf);
        match self.mode {
            Mode::Decision => {
                let [warning_area, command_area, hint_area] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(2),
                ])
                .margin(1)
                .areas(dialog_area);

                let [command_prefix_area, command_area] =
                    Layout::horizontal([Constraint::Length(2), Constraint::Fill(1)])
                        .areas(command_area);

                Line::from("This command may be dangerous. Confirm before execution.")
                    .style(WARNING_STYLE)
                    .render(warning_area, buf);
                Line::from("> ")
                    .style(Style::new().fg(Color::LightGreen))
                    .render(command_prefix_area, buf);
                Paragraph::new(self.command.as_str())
                    .style(Style::new().fg(Color::White))
                    .wrap(Wrap { trim: true })
                    .render(command_area, buf);
                Paragraph::new("enter/y: approve | esc/n: reject | m: reject with reason\nPress m to input reject reason")
                    .style(HINT_STYLE)
                    .right_aligned()
                    .render(hint_area, buf);
            }
            Mode::InputReason => {
                let [warning_area, command_area, input_area, hint_area] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(2),
                ])
                .margin(1)
                .areas(dialog_area);

                let [command_prefix_area, command_area] =
                    Layout::horizontal([Constraint::Length(2), Constraint::Fill(1)])
                        .areas(command_area);

                Line::from("This command may be dangerous. Confirm before execution.")
                    .style(WARNING_STYLE)
                    .render(warning_area, buf);
                Line::from("> ")
                    .style(Style::new().fg(Color::LightGreen))
                    .render(command_prefix_area, buf);
                Paragraph::new(self.command.as_str())
                    .style(Style::new().fg(Color::White))
                    .wrap(Wrap { trim: true })
                    .render(command_area, buf);
                self.reason_input.render(input_area, buf);
                Paragraph::new("enter: submits rejection\nesc: back to decision mode")
                    .style(HINT_STYLE)
                    .right_aligned()
                    .render(hint_area, buf);
            }
        }
    }
}

impl App {
    fn new(command: String) -> io::Result<Self> {
        let mut reason_input = TextArea::default();
        reason_input.set_block(
            Block::bordered()
                .title_top("")
                .title_top(Line::from("Reject Reason").style(HINT_STYLE))
                .border_type(BorderType::Rounded)
                .border_style(INPUT_BORDER_STYLE),
        );
        reason_input.set_style(INPUT_STYLE);
        let widget = AppWidget {
            command,
            mode: Mode::Decision,
            reason_input,
        };
        let terminal = InlineTerminal::init_with_options(ratatui::TerminalOptions {
            viewport: Viewport::Inline(widget.calc_height()),
        })?;
        Ok(Self { terminal, widget })
    }

    fn update_viewport(&mut self) -> io::Result<()> {
        self.terminal.close();
        self.terminal = InlineTerminal::init_with_options(ratatui::TerminalOptions {
            viewport: Viewport::Inline(self.widget.calc_height()),
        })?;
        Ok(())
    }

    fn handle_decision_key(&mut self, key: KeyEvent) -> io::Result<Option<AppDecision>> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y')
                if key.modifiers.is_empty() =>
            {
                Ok(Some(AppDecision::Approve))
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') if key.modifiers.is_empty() => {
                Ok(Some(AppDecision::Reject("Rejected by user.".to_string())))
            }
            KeyCode::Char('m') | KeyCode::Char('M') if key.modifiers.is_empty() => {
                self.widget.mode = Mode::InputReason;
                self.update_viewport()?;
                Ok(None)
            }
            KeyCode::Char('c') | KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                Ok(Some(AppDecision::Reject("Rejected by user.".to_string())))
            }
            _ => Ok(None),
        }
    }

    fn handle_reason_key(&mut self, key: KeyEvent) -> io::Result<Option<AppDecision>> {
        match key.code {
            KeyCode::Enter if key.modifiers.is_empty() => {
                let reason = self
                    .widget
                    .reason_input
                    .lines()
                    .join("\n")
                    .trim()
                    .to_string();
                let reason = if reason.is_empty() {
                    "Rejected by user.".to_string()
                } else {
                    format!("Rejected by user: {reason}")
                };
                Ok(Some(AppDecision::Reject(reason)))
            }
            KeyCode::Esc if key.modifiers.is_empty() => {
                self.widget.mode = Mode::Decision;
                self.update_viewport()?;
                Ok(None)
            }
            KeyCode::Char('c') | KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                Ok(Some(AppDecision::Reject("Rejected by user.".to_string())))
            }
            _ => {
                self.widget.reason_input.input(Event::Key(key));
                Ok(None)
            }
        }
    }

    fn run(mut self) -> io::Result<AppDecision> {
        // 在 windows 某些终端中会将执行命令的回车键也监听到, 因此忽略这个事件.
        let start_time = Instant::now();
        let skip_enter_duration = Duration::from_millis(10);
        loop {
            self.terminal
                .draw(|frame| frame.render_widget(&self.widget, frame.area()))?;

            let event = crossterm::event::read()?;
            let Event::Key(key) = event else {
                continue;
            };
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            if matches!(key.code, KeyCode::Enter) && start_time.elapsed() <= skip_enter_duration {
                continue;
            }

            let decision = match self.widget.mode {
                Mode::Decision => self.handle_decision_key(key)?,
                Mode::InputReason => self.handle_reason_key(key)?,
            };
            if let Some(decision) = decision {
                return Ok(decision);
            }
        }
    }
}

/// 使用 tui 向用户询问是否执行某个命令, 如果用户同意, 返回 Ok(()), 如果用户拒绝, 返回 Err(String), 内含拒绝原因.
pub(crate) async fn confirm_execution(
    program: &Path,
    args: &[impl AsRef<str>],
) -> Result<(), String> {
    let command = std::iter::once(program.display().to_string())
        .chain(args.iter().map(|x| x.as_ref().to_string()))
        .collect::<Vec<_>>()
        .join(" ");

    let app = App::new(command.clone()).map_err(|e| {
        warn!(error = %e, "failed to initialize dangerous command tui");
        format!("Failed to initialize confirmation dialog: {e}")
    })?;

    // 暂时禁用 tracing_indicatif 进度条
    let decision = tokio::task::spawn_blocking(|| {
        tracing_indicatif::suspend_tracing_indicatif(|| {
            app.run().map_err(|e| {
                warn!(error = %e, "dangerous confirmation dialog exited with error");
                format!("Failed to read confirmation input: {e}")
            })
        })
    })
    .await
    .unwrap()?;

    match decision {
        AppDecision::Approve => {
            info!(command = %command, "dangerous command approved by user");
            Ok(())
        }
        AppDecision::Reject(reason) => {
            info!(command = %command, reason = %reason, "dangerous command rejected by user");
            Err(reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing::level_filters::LevelFilter;
    use tracing_indicatif::IndicatifLayer;
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    use crate::tui::dangerous_execution::confirm_execution;

    fn log_init() {
        let indicatif_layer = IndicatifLayer::new();
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .without_time()
            .with_writer(indicatif_layer.get_stderr_writer())
            .with_filter(LevelFilter::INFO);
        tracing_subscriber::registry()
            .with(stderr_layer)
            .with(indicatif_layer.with_filter(LevelFilter::INFO)) // 在进度条上不显示内容
            .init();
    }

    #[tokio::test]
    async fn test_confirm_execution() {
        log_init();
        confirm_execution("approve".as_ref(), &["hello", "world"]).await.unwrap();
        assert_eq!(
            confirm_execution("reject".as_ref(), &["hello", "worlds"]).await.unwrap_err(),
            "Rejected by user."
        );
        assert_eq!(
            confirm_execution("reject_with_reason".as_ref(), &["reason:", "noicant"]).await.unwrap_err(),
            "Rejected by user: noicant"
        );
    }
}
