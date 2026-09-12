//! Text editing primitives for the custom Win32 chrome.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretMotion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
}

#[derive(Debug, Clone)]
pub struct TextField {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
    pub password: bool,
    pub multiline: bool,
    pub placeholder: String,
    pub scroll: f32,
}

impl TextField {
    pub fn new(multiline: bool) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            anchor: 0,
            password: false,
            multiline,
            placeholder: String::new(),
            scroll: 0.0,
        }
    }

    pub fn with_text(text: impl Into<String>, multiline: bool) -> Self {
        let text = text.into();
        let caret = text.chars().count();
        Self {
            text,
            caret,
            anchor: caret,
            password: false,
            multiline,
            placeholder: String::new(),
            scroll: 0.0,
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.text.chars().count();
        self.anchor = self.caret;
        self.scroll = 0.0;
    }

    pub fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(index, _)| index)
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let (start, end) = if self.caret < self.anchor {
            (self.caret, self.anchor)
        } else {
            (self.anchor, self.caret)
        };
        (start != end).then_some((start, end))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let characters = self.chars();
        Some(characters[start..end].iter().collect())
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.chars().count();
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        let from = self.byte_index(start);
        let to = self.byte_index(end);
        self.text.replace_range(from..to, "");
        self.caret = start;
        self.anchor = start;
        true
    }

    pub fn insert(&mut self, character: char) {
        self.delete_selection();
        let index = self.byte_index(self.caret);
        self.text.insert(index, character);
        self.caret += 1;
        self.anchor = self.caret;
    }

    pub fn insert_str(&mut self, value: &str) {
        self.delete_selection();
        let index = self.byte_index(self.caret);
        self.text.insert_str(index, value);
        let added = value.chars().count();
        self.caret += added;
        self.anchor = self.caret;
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let to = self.byte_index(self.caret);
        let from = self.byte_index(self.caret - 1);
        self.text.replace_range(from..to, "");
        self.caret -= 1;
        self.anchor = self.caret;
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let length = self.text.chars().count();
        if self.caret >= length {
            return;
        }
        let from = self.byte_index(self.caret);
        let to = self.byte_index(self.caret + 1);
        self.text.replace_range(from..to, "");
    }

    pub fn move_caret(&mut self, motion: CaretMotion, extend: bool) {
        let characters = self.chars();
        let length = characters.len();
        let mut target = self.caret.min(length);
        match motion {
            CaretMotion::Left => target = target.saturating_sub(1),
            CaretMotion::Right => target = (target + 1).min(length),
            CaretMotion::Home => target = 0,
            CaretMotion::End => target = length,
            CaretMotion::WordLeft => {
                let mut index = target;
                while index > 0 && characters[index - 1].is_whitespace() {
                    index -= 1;
                }
                while index > 0 && !characters[index - 1].is_whitespace() {
                    index -= 1;
                }
                target = index;
            }
            CaretMotion::WordRight => {
                let mut index = target;
                while index < length && !characters[index].is_whitespace() {
                    index += 1;
                }
                while index < length && characters[index].is_whitespace() {
                    index += 1;
                }
                target = index;
            }
        }
        self.caret = target;
        if !extend {
            self.anchor = target;
        }
    }

    /// Moves the caret one line up or down, keeping the column where possible.
    pub fn move_vertical(&mut self, delta: i32, extend: bool) {
        let characters = self.chars();
        let (line_start, line_end) = self.line_bounds_at(self.caret);
        let column = self.caret - line_start;
        let target_line = if delta < 0 {
            if line_start == 0 {
                self.caret = 0;
                if !extend {
                    self.anchor = 0;
                }
                return;
            }
            let previous_end = line_start - 1;
            let previous_start = characters[..previous_end]
                .iter()
                .rposition(|character| *character == '\n')
                .map_or(0, |index| index + 1);
            (previous_start, previous_end)
        } else {
            if line_end >= characters.len() {
                self.caret = characters.len();
                if !extend {
                    self.anchor = characters.len();
                }
                return;
            }
            let next_start = line_end + 1;
            let next_end = characters[next_start..]
                .iter()
                .position(|character| *character == '\n')
                .map_or(characters.len(), |index| next_start + index);
            (next_start, next_end)
        };
        self.caret = (target_line.0 + column).min(target_line.1);
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Start and end (exclusive) of the line containing `index`.
    pub fn line_bounds_at(&self, index: usize) -> (usize, usize) {
        let characters = self.chars();
        let index = index.min(characters.len());
        let start = characters[..index]
            .iter()
            .rposition(|character| *character == '\n')
            .map_or(0, |position| position + 1);
        let end = characters[start..]
            .iter()
            .position(|character| *character == '\n')
            .map_or(characters.len(), |position| start + position);
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_backspace_work_on_multibyte_text() {
        let mut field = TextField::new(false);
        field.insert('h');
        field.insert('é');
        field.insert('y');
        assert_eq!(field.text, "héy");
        assert_eq!(field.caret, 3);
        field.backspace();
        assert_eq!(field.text, "hé");
        assert_eq!(field.caret, 2);
        field.move_caret(CaretMotion::Home, false);
        field.delete_forward();
        assert_eq!(field.text, "é");
    }

    #[test]
    fn selection_replaces_text() {
        let mut field = TextField::with_text("SELECT 1", true);
        field.select_all();
        field.insert_str("SELECT 2");
        assert_eq!(field.text, "SELECT 2");
        assert_eq!(field.selection(), None);
    }

    #[test]
    fn vertical_movement_keeps_the_column() {
        let mut field = TextField::with_text("alpha\nbeta\ngamma", true);
        field.caret = 3; // inside "alpha"
        field.anchor = 3;
        field.move_vertical(1, false);
        assert_eq!(field.caret, 9); // same column in "beta"
        field.move_vertical(1, false);
        assert_eq!(field.caret, 14); // clamped to the shorter line
        field.move_vertical(-1, false);
        assert_eq!(field.caret, 9);
    }

    #[test]
    fn word_movement_skips_whitespace() {
        let mut field = TextField::with_text("select * from users", true);
        field.move_caret(CaretMotion::Home, false);
        field.move_caret(CaretMotion::WordRight, false);
        assert_eq!(field.caret, 6);
        field.move_caret(CaretMotion::WordRight, false);
        assert_eq!(field.caret, 8);
        field.move_caret(CaretMotion::End, false);
        field.move_caret(CaretMotion::WordLeft, false);
        assert_eq!(field.caret, 14);
    }
}
