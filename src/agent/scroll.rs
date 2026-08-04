use unicode_width::UnicodeWidthChar;

#[derive(Debug)]
pub(super) struct ScrollWindow {
    width: usize,
    content: String,
    cursor: usize,
}

impl ScrollWindow {
    pub(super) fn new(width: usize) -> Self {
        Self {
            width,
            content: String::new(),
            cursor: 0,
        }
    }

    pub(super) fn push(&mut self, text: &str) {
        self.content.push_str(&sanitize(text));
    }

    pub(super) fn advance(&mut self, step: usize) -> String {
        let remaining = &self.content[self.cursor..];
        let appended = prefix_by_width(remaining, step);
        self.cursor += appended.len();
        self.window()
    }

    pub(super) fn finish(&mut self) -> String {
        self.cursor = self.content.len();
        self.window()
    }

    pub(super) fn window(&self) -> String {
        tail_by_width(&self.content[..self.cursor], self.width).to_owned()
    }
}

fn prefix_by_width(text: &str, width: usize) -> &str {
    let mut used = 0;
    let mut end = 0;
    for (index, character) in text.char_indices() {
        let character_width = character.width_cjk().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        end = index + character.len_utf8();
    }
    &text[..end]
}

fn tail_by_width(text: &str, width: usize) -> &str {
    let mut used = 0;
    let mut start = text.len();
    for (index, character) in text.char_indices().rev() {
        let character_width = character.width_cjk().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        start = index;
    }
    &text[start..]
}

fn sanitize(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            if characters.next_if_eq(&'[').is_some() {
                for control in characters.by_ref() {
                    if ('@'..='~').contains(&control) {
                        break;
                    }
                }
            } else if characters.next_if_eq(&']').is_some() {
                while let Some(control) = characters.next() {
                    if control == '\x07' {
                        break;
                    }
                    if control == '\x1b' && characters.next_if_eq(&'\\').is_some() {
                        break;
                    }
                }
            }
            continue;
        }
        if character.is_control() {
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::ScrollWindow;

    #[test]
    fn advances_only_at_complete_display_widths() {
        let mut window = ScrollWindow::new(10);
        window.push("你好世界");

        assert_eq!(window.advance(0), "");
        assert_eq!(window.advance(1), "");
        assert_eq!(window.advance(2), "你");
        assert_eq!(window.advance(6), "你好世界");
    }

    #[test]
    fn keeps_the_tail_inside_the_display_width() {
        let mut window = ScrollWindow::new(10);
        window.push("0123456789abcdef");

        assert_eq!(window.advance(7), "0123456");
        let displayed = window.finish();
        assert_eq!(displayed, "6789abcdef");
        assert_eq!(displayed.width_cjk(), 10);
    }

    #[test]
    fn appends_incrementally_after_reaching_the_end() {
        let mut window = ScrollWindow::new(10);
        window.push("0123456789");
        assert_eq!(window.finish(), "0123456789");

        window.push("abcdef");
        assert_eq!(window.advance(2), "23456789ab");
        assert_eq!(window.finish(), "6789abcdef");
    }

    #[test]
    fn sanitizes_control_and_ansi_sequences() {
        let mut window = ScrollWindow::new(usize::MAX);
        window.push("before\n\t\x1b[31mred\x1b[0m\rafter");
        assert_eq!(window.finish(), "before  red after");
    }
}
