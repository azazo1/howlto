use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};

#[derive(Debug, Clone)]
pub struct EditorState {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
        }
    }
}

impl EditorState {
    #[allow(dead_code)]
    pub fn from_text(text: &str) -> Self {
        let mut lines = text
            .split('\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<String>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let row = lines.len().saturating_sub(1);
        let col = lines[row].chars().count();
        Self { lines, row, col }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    pub fn handle_event(&mut self, event: Event, allow_newline: bool) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        self.handle_key(key, allow_newline)
    }

    pub fn insert_newline(&mut self) {
        self.insert_newline_at_cursor();
    }

    pub fn lines_with_cursor(&self) -> Text<'static> {
        let lines: Vec<Line<'static>> = self
            .lines
            .iter()
            .enumerate()
            .map(|(row, line)| {
                if row != self.row {
                    return Line::from(line.clone());
                }
                let mut spans = Vec::new();
                let prefix: String = line.chars().take(self.col).collect();
                let current = line
                    .chars()
                    .nth(self.col)
                    .map(|ch| ch.to_string())
                    .unwrap_or_else(|| " ".to_string());
                let suffix: String = line
                    .chars()
                    .skip(self.col + usize::from(self.col < line.chars().count()))
                    .collect();
                if !prefix.is_empty() {
                    spans.push(Span::raw(prefix));
                }
                spans.push(Span::styled(
                    current,
                    Style::new().bg(Color::White).fg(Color::Black),
                ));
                if !suffix.is_empty() {
                    spans.push(Span::raw(suffix));
                }
                Line::from(spans)
            })
            .collect();
        Text::from(lines)
    }

    fn handle_key(&mut self, key: KeyEvent, allow_newline: bool) -> bool {
        if key.modifiers == KeyModifiers::CONTROL {
            return false;
        }
        match key.code {
            KeyCode::Char(ch) => {
                self.insert_char(ch);
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Left => {
                self.move_left();
                true
            }
            KeyCode::Right => {
                self.move_right();
                true
            }
            KeyCode::Up => {
                self.move_up();
                true
            }
            KeyCode::Down => {
                self.move_down();
                true
            }
            KeyCode::Home => {
                self.col = 0;
                true
            }
            KeyCode::End => {
                self.col = self.line_len(self.row);
                true
            }
            KeyCode::Enter if allow_newline => {
                self.insert_newline_at_cursor();
                true
            }
            _ => false,
        }
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
    }

    fn byte_index(line: &str, col: usize) -> usize {
        if col == 0 {
            return 0;
        }
        line.char_indices()
            .nth(col)
            .map(|(idx, _)| idx)
            .unwrap_or(line.len())
    }

    fn insert_char(&mut self, ch: char) {
        let idx = Self::byte_index(&self.lines[self.row], self.col);
        self.lines[self.row].insert(idx, ch);
        self.col += 1;
    }

    fn insert_newline_at_cursor(&mut self) {
        let idx = Self::byte_index(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(idx);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, tail);
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            let end = Self::byte_index(&self.lines[self.row], self.col);
            let start = Self::byte_index(&self.lines[self.row], self.col - 1);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
            return;
        }
        if self.row == 0 {
            return;
        }
        let current = self.lines.remove(self.row);
        self.row -= 1;
        self.col = self.line_len(self.row);
        self.lines[self.row].push_str(&current);
    }

    fn delete(&mut self) {
        let len = self.line_len(self.row);
        if self.col < len {
            let start = Self::byte_index(&self.lines[self.row], self.col);
            let end = Self::byte_index(&self.lines[self.row], self.col + 1);
            self.lines[self.row].replace_range(start..end, "");
            return;
        }
        if self.row + 1 >= self.lines.len() {
            return;
        }
        let next = self.lines.remove(self.row + 1);
        self.lines[self.row].push_str(&next);
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len(self.row);
        }
    }

    fn move_right(&mut self) {
        let len = self.line_len(self.row);
        if self.col < len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.line_len(self.row));
        }
    }

    fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.line_len(self.row));
        }
    }
}
