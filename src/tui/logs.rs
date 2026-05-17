use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use tracing::Level;

use crate::logging::UiLogEntry;

#[cfg_attr(not(test), allow(dead_code))]
const TIME_STYLE: Style = Style::new().fg(Color::DarkGray);
#[cfg_attr(not(test), allow(dead_code))]
const TARGET_STYLE: Style = Style::new()
    .fg(Color::LightCyan)
    .add_modifier(Modifier::BOLD);
const MESSAGE_STYLE: Style = Style::new().fg(Color::Gray);
const FIELD_KEY_STYLE: Style = Style::new()
    .fg(Color::LightBlue)
    .add_modifier(Modifier::ITALIC);
const FIELD_VALUE_STYLE: Style = Style::new().fg(Color::Gray);

fn level_style(level: Level) -> Style {
    match level {
        Level::ERROR => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        Level::WARN => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        Level::INFO => Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
        Level::DEBUG => Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD),
        Level::TRACE => Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn render_log_lines(entries: &[UiLogEntry]) -> Vec<Line<'static>> {
    entries.iter().map(render_log_line).collect()
}

pub fn render_compact_log_lines(entries: &[UiLogEntry]) -> Vec<Line<'static>> {
    entries.iter().map(render_compact_log_line).collect()
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn render_log_line(entry: &UiLogEntry) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{} ", entry.timestamp), TIME_STYLE),
        Span::styled(
            format!("{:>5} ", entry.level.as_str()),
            level_style(entry.level),
        ),
        Span::styled(format!("{}: ", entry.target), TARGET_STYLE),
    ];

    if !entry.message.is_empty() {
        spans.push(Span::styled(entry.message.clone(), MESSAGE_STYLE));
    }

    for (key, value) in &entry.fields {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(key.clone(), FIELD_KEY_STYLE));
        spans.push(Span::styled(format!("={value}"), FIELD_VALUE_STYLE));
    }

    Line::from(spans)
}

pub fn render_compact_log_line(entry: &UiLogEntry) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{:>5} ", entry.level.as_str()),
        level_style(entry.level),
    )];

    if !entry.message.is_empty() {
        spans.push(Span::styled(entry.message.clone(), MESSAGE_STYLE));
    }

    for (key, value) in &entry.fields {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(key.clone(), FIELD_KEY_STYLE));
        spans.push(Span::styled(format!("={value}"), FIELD_VALUE_STYLE));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::{render_compact_log_line, render_log_line, render_log_lines};
    use crate::logging::UiLogEntry;
    use ratatui::style::Color;
    use tracing::Level;

    #[test]
    fn render_log_line_highlights_level_and_target() {
        let lines = render_log_lines(&[UiLogEntry {
            timestamp: "2026-05-17T12:00:00Z".into(),
            level: Level::WARN,
            target: "howlto::chat".into(),
            message: "stream updated".into(),
            fields: vec![("turn".into(), "3".into())],
        }]);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Yellow));
        assert_eq!(lines[0].spans[2].style.fg, Some(Color::LightCyan));
    }

    #[test]
    fn render_single_log_line_keeps_message_text() {
        let line = render_log_line(&UiLogEntry {
            timestamp: "2026-05-17T12:00:00Z".into(),
            level: Level::INFO,
            target: "howlto::session".into(),
            message: "saved chat session".into(),
            fields: Vec::new(),
        });

        assert!(
            line.spans
                .iter()
                .any(|span| span.content == "saved chat session")
        );
    }

    #[test]
    fn compact_log_line_omits_timestamp_and_target() {
        let line = render_compact_log_line(&UiLogEntry {
            timestamp: "2026-05-17T12:00:00Z".into(),
            level: Level::DEBUG,
            target: "howlto::chat".into(),
            message: "tool call".into(),
            fields: vec![("tool".into(), "help".into())],
        });

        assert_eq!(line.spans[0].style.fg, Some(Color::Blue));
        assert!(
            line.spans
                .iter()
                .all(|span| span.content != "2026-05-17T12:00:00Z ")
        );
        assert!(
            line.spans
                .iter()
                .all(|span| span.content != "howlto::chat: ")
        );
        assert!(line.spans.iter().any(|span| span.content == "tool call"));
    }
}
