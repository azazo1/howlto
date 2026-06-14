//! 把 markdown 字符串解析成带颜色高亮的 [`ratatui::text::Text`].
//!
//! 设计: 用 [`pulldown_cmark`] 做 CommonMark 解析, 遍历事件流,
//! 给不同语义区域 (标题/代码块/行内代码/强调/引用/列表) 套颜色与样式
//! ([`ratatui::style::Style`]), **不改变布局结构**, 输出逐行拼接的
//! [`ratatui::text::Text`].
//!
//! 同时提供 [`to_plain_text`] 提取纯文本 (用于 plain/管道模式输出, 避免
//! markdown 标记污染下游).

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

/// 标题颜色 (按层级), 索引 0 对应 H1.
const HEADING_COLORS: [Color; 6] = [
    Color::LightRed,
    Color::LightYellow,
    Color::LightGreen,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
];

/// 行内/块代码颜色.
const CODE_COLOR: Color = Color::Yellow;

/// 强调 (粗体) 颜色.
const STRONG_COLOR: Color = Color::White;

/// 行内渲染上下文: 当前生效的样式修饰栈.
#[derive(Debug, Clone, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    code: bool,
}

impl InlineStyle {
    fn to_style(&self) -> Style {
        let mut style = Style::new();
        let mut mods = Modifier::empty();
        if self.bold {
            mods |= Modifier::BOLD;
        }
        if self.italic {
            mods |= Modifier::ITALIC;
        }
        if self.strikethrough {
            mods |= Modifier::CROSSED_OUT;
        }
        style = style.add_modifier(mods);
        if self.code {
            style = style.fg(CODE_COLOR);
        } else if self.bold {
            // 粗体非代码: 用更亮的颜色突出.
            style = style.fg(STRONG_COLOR);
        }
        style
    }
}

/// 解析 markdown, 渲染成带颜色高亮的 [`Text`].
pub fn render(markdown: &str) -> Text<'static> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    // 当前行的 spans, 遇到块边界时 flush 成一行.
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style = InlineStyle::default();
    // 嵌套标题级别 (0 表示非标题).
    let mut heading_level: Option<usize> = None;
    // 列表项计数, 用于输出 "1. " / "- " 前缀. 简化: 只处理一层有序/无序.
    let mut list_stack: Vec<Option<u64>> = Vec::new();

    let mut flush = |spans: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>| {
        out.push(std::mem::take(spans).into());
    };

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    heading_level = Some(level as usize);
                }
                Tag::Paragraph | Tag::BlockQuote(_) => {
                    // 段落/引用开始: 不特殊处理, 文本会按 SoftBreak/HardBreak 自然分行.
                }
                Tag::CodeBlock(_) => {
                    style.code = true;
                }
                Tag::List(start) => {
                    list_stack.push(start);
                }
                Tag::Item => {
                    // 列表项: 写入前缀.
                    let prefix = match list_stack.last_mut() {
                        Some(Some(n)) => {
                            let p = format!("{n}. ");
                            *n += 1;
                            p
                        }
                        Some(None) => "- ".to_string(),
                        None => String::new(),
                    };
                    push_span(&mut current_spans, &prefix, Style::new());
                }
                Tag::Emphasis => style.italic = true,
                Tag::Strong => style.bold = true,
                Tag::Strikethrough => style.strikethrough = true,
                _ => (),
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    flush(&mut current_spans, &mut lines);
                    heading_level = None;
                }
                TagEnd::Paragraph | TagEnd::BlockQuote(_) => {
                    flush(&mut current_spans, &mut lines);
                }
                TagEnd::CodeBlock => {
                    style.code = false;
                    flush(&mut current_spans, &mut lines);
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::Item => {
                    flush(&mut current_spans, &mut lines);
                }
                TagEnd::Emphasis => style.italic = false,
                TagEnd::Strong => style.bold = false,
                TagEnd::Strikethrough => style.strikethrough = false,
                _ => (),
            },
            Event::Text(text) => {
                let mut s = style.to_style();
                if let Some(level) = heading_level
                    && let Some(color) = HEADING_COLORS.get(level - 1).copied()
                {
                    s = s.fg(color).add_modifier(Modifier::BOLD);
                }
                // 文本可能含换行 (代码块/多行段落), 按行拆分 flush.
                push_multiline(&mut current_spans, &mut lines, &text, s, &mut flush);
            }
            Event::Code(code) => {
                let s = Style::new().fg(CODE_COLOR);
                push_multiline(&mut current_spans, &mut lines, &code, s, &mut flush);
            }
            Event::SoftBreak | Event::HardBreak => {
                flush(&mut current_spans, &mut lines);
            }
            // 忽略图片/规则/脚注/HTML 等较冷门的元素 (不影响主路径).
            _ => (),
        }
    }
    // 末尾若还有未 flush 的 spans.
    if !current_spans.is_empty() {
        flush(&mut current_spans, &mut lines);
    }
    Text::from(lines)
}

/// 把单个不含换行的字符串作为 span 推入当前行.
fn push_span(spans: &mut Vec<Span<'static>>, s: &str, style: Style) {
    if s.is_empty() {
        return;
    }
    spans.push(Span::styled(s.to_string(), style));
}

/// 把可能含换行的字符串按行拆分推入, 换行处 flush.
fn push_multiline(
    spans: &mut Vec<Span<'static>>,
    lines: &mut Vec<Line<'static>>,
    s: &str,
    style: Style,
    flush: &mut impl FnMut(&mut Vec<Span<'static>>, &mut Vec<Line<'static>>),
) {
    let mut iter = s.split('\n');
    if let Some(first) = iter.next() {
        push_span(spans, first, style);
    }
    for part in iter {
        // 遇到换行: flush 当前行, 再开始新行.
        flush(spans, lines);
        push_span(spans, part, style);
    }
}

/// 提取 markdown 的纯文本 (去掉所有标记), 各块用换行分隔.
/// 用于 plain/管道模式输出, 避免标记污染下游.
pub fn to_plain_text(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);

    let mut out = String::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    for event in parser {
        match event {
            Event::Start(Tag::List(start)) => list_stack.push(start),
            Event::Start(Tag::Item) => match list_stack.last_mut() {
                Some(Some(n)) => {
                    out.push_str(&format!("{n}. "));
                    *n += 1;
                }
                Some(None) => out.push_str("- "),
                None => {}
            },
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::End(TagEnd::Item | TagEnd::Paragraph | TagEnd::Heading(_)) => {
                out.push('\n');
            }
            Event::Text(t) | Event::Code(t) => out.push_str(t.as_ref()),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            _ => (),
        }
    }
    // 折叠多余空行.
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.trim_end().to_string()
}

/// 把 markdown 渲染成带 ANSI 颜色高亮的文本并打印到 stdout (逐行).
/// 用于交互模式下直接展示纯文本/markdown 回答.
pub fn print_ansi(markdown: &str) {
    let text = render(markdown);
    for line in &text.lines {
        let rendered: String = line.spans.iter().map(span_to_ansi).collect();
        println!("{rendered}");
    }
}

/// 把 ratatui [`Color`](ratatui::style::Color) 映射到 ANSI 前景色 SGR 序列.
fn color_to_ansi_fg(c: Color) -> String {
    match c {
        Color::Black => "30".into(),
        Color::Red => "31".into(),
        Color::Green => "32".into(),
        Color::Yellow => "33".into(),
        Color::Blue => "34".into(),
        Color::Magenta => "35".into(),
        Color::Cyan => "36".into(),
        Color::Gray | Color::DarkGray => "90".into(),
        Color::LightRed => "91".into(),
        Color::LightGreen => "92".into(),
        Color::LightYellow => "93".into(),
        Color::LightBlue => "94".into(),
        Color::LightMagenta => "95".into(),
        Color::LightCyan => "96".into(),
        Color::White => "97".into(),
        _ => "0".into(), // Reset / 默认.
    }
}

/// 把一个 span 渲染成带 ANSI 样式的字符串 (前景色 + 修饰符).
fn span_to_ansi(span: &Span<'_>) -> String {
    let style = span.style;
    let mut codes: Vec<String> = Vec::new();
    if let Some(fg) = style.fg {
        codes.push(color_to_ansi_fg(fg));
    }
    let add = style.add_modifier;
    if add.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if add.contains(Modifier::ITALIC) {
        codes.push("3".into());
    }
    if add.contains(Modifier::CROSSED_OUT) {
        codes.push("9".into());
    }
    let prefix = if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    };
    format!("{prefix}{}\x1b[0m", span.content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn render_heading_has_bold_modifier() {
        let text = render("# Title");
        let lines = text.lines;
        assert_eq!(lines.len(), 1, "heading should be one line");
        let spans: &[Span<'static>] = lines[0].spans.as_ref();
        assert!(!spans.is_empty());
        assert!(
            spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD)),
            "heading span should be bold, got {:?}",
            spans
        );
    }

    #[test]
    fn render_code_has_code_color() {
        let text = render("`code`");
        let spans: Vec<&Span> = text.lines.iter().flat_map(|l| l.spans.iter()).collect();
        assert!(
            spans.iter().any(|s| s.style.fg == Some(CODE_COLOR)),
            "some span should use CODE_COLOR, got {:?}",
            spans
        );
    }

    #[test]
    fn render_multiline_paragraph_splits_lines() {
        let text = render("line one\nline two");
        // 两行段落 -> 两行输出.
        assert!(text.lines.len() >= 2, "got {} lines", text.lines.len());
    }

    #[test]
    fn to_plain_text_strips_markup() {
        let md = "# Title\n\nSome **bold** and `code`.\n- item";
        let plain = to_plain_text(md);
        assert!(!plain.contains('#'), "heading marker should be stripped");
        assert!(!plain.contains("**"), "bold marker should be stripped");
        assert!(!plain.contains('`'), "code marker should be stripped");
        assert!(plain.contains("Title"));
        assert!(plain.contains("bold"));
        assert!(plain.contains("code"));
        assert!(plain.contains("- item") || plain.contains("item"));
    }

    #[test]
    fn to_plain_text_ordered_list_prefix() {
        let plain = to_plain_text("1. first\n2. second");
        assert!(plain.contains("1. first"), "got: {plain:?}");
        assert!(plain.contains("2. second"), "got: {plain:?}");
    }

    // ---- 目视检查测试 ----
    // 不是断言, 而是把渲染后的 spans 用真实 ANSI 颜色打印出来, 供人眼检查.
    // 运行方式: `cargo test render_visual -- --nocapture`
    // (复用模块级的 `span_to_ansi`.)

    #[test]
    #[ignore = "目视检查测试, 用 `cargo test render_visual -- --nocapture --ignored` 运行"]
    fn render_visual() {
        let md = r#"# Heading Level 1

Some **bold** text, `inline code`, and *italic*.

## Heading Level 2

- list item one
- list item two

1. ordered first
2. ordered second

> a blockquote line

```rust
fn main() {
    println!("code block");
}
```
"#;
        let text = render(md);
        println!("\n=== markdown::render 目视检查 (需 --nocapture 且在支持 ANSI 的终端) ===\n");
        for line in &text.lines {
            let rendered: String = line.spans.iter().map(span_to_ansi).collect();
            println!("{rendered}");
        }
        println!("\n=== 结束 ===\n");
        // 至少要有若干行, 避免完全静默通过.
        assert!(!text.lines.is_empty(), "render should produce output");
    }
}
