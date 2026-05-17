use ratatui::text::{Line, Span, Text};

pub fn render_markdown(content: &str) -> Text<'static> {
    let rendered = tui_markdown::from_str(content);
    let lines = rendered
        .lines
        .into_iter()
        .map(|line| Line {
            style: line.style,
            alignment: line.alignment,
            spans: line
                .spans
                .into_iter()
                .map(|span| Span {
                    style: span.style,
                    content: span.content.into_owned().into(),
                })
                .collect(),
        })
        .collect();
    Text {
        alignment: rendered.alignment,
        style: rendered.style,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::render_markdown;

    #[test]
    fn markdown_render_does_not_panic() {
        let rendered =
            render_markdown("# Title\n\n- item\n- `code`\n\n> quote\n\n```sh\nls -la\n```");
        assert!(!rendered.lines.is_empty());
    }
}
