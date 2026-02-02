//! InlineEdit: Text editing with cursor position

use unicode_width::UnicodeWidthStr;

/// Inline text editor state
#[derive(Debug, Clone)]
pub struct InlineEdit {
    /// The text being edited
    buffer: String,
    /// Cursor position (character index, not byte)
    cursor: usize,
    /// Original value (for cancel/restore)
    #[allow(dead_code)]
    original: String,
}

impl InlineEdit {
    /// Create a new inline editor with initial value
    pub fn new(initial: &str) -> Self {
        let len = initial.chars().count();
        Self {
            buffer: initial.to_string(),
            cursor: len,
            original: initial.to_string(),
        }
    }

    /// Get the current buffer contents
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Get the cursor position (character index)
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Get display width up to cursor position
    #[allow(dead_code)]
    pub fn cursor_display_width(&self) -> usize {
        let text_before_cursor: String = self.buffer.chars().take(self.cursor).collect();
        text_before_cursor.width()
    }

    /// Get the original value
    #[allow(dead_code)]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Move cursor left
    pub fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right
    pub fn right(&mut self) {
        let len = self.buffer.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    /// Move cursor to start
    pub fn home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end
    pub fn end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }

    /// Insert a character at cursor position
    pub fn insert(&mut self, c: char) {
        let byte_pos = self.cursor_byte_position();
        self.buffer.insert(byte_pos, c);
        self.cursor += 1;
    }

    /// Insert a string at cursor position
    #[allow(dead_code)]
    pub fn insert_str(&mut self, s: &str) {
        let byte_pos = self.cursor_byte_position();
        self.buffer.insert_str(byte_pos, s);
        self.cursor += s.chars().count();
    }

    /// Delete character before cursor (backspace)
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_pos = self.cursor_byte_position();
            let char_len = self.buffer[byte_pos..]
                .chars()
                .next()
                .map_or(0, |c| c.len_utf8());
            self.buffer.drain(byte_pos..byte_pos + char_len);
        }
    }

    /// Delete character at cursor (delete key)
    pub fn delete(&mut self) {
        let len = self.buffer.chars().count();
        if self.cursor < len {
            let byte_pos = self.cursor_byte_position();
            let char_len = self.buffer[byte_pos..]
                .chars()
                .next()
                .map_or(0, |c| c.len_utf8());
            self.buffer.drain(byte_pos..byte_pos + char_len);
        }
    }

    /// Delete word before cursor (ctrl+backspace)
    #[allow(dead_code)]
    pub fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let chars: Vec<char> = self.buffer.chars().collect();
        let mut new_cursor = self.cursor;

        // Skip any trailing spaces
        while new_cursor > 0 && chars[new_cursor - 1].is_whitespace() {
            new_cursor -= 1;
        }

        // Delete until start of word
        while new_cursor > 0 && !chars[new_cursor - 1].is_whitespace() {
            new_cursor -= 1;
        }

        // Remove the characters
        let start_byte = self.char_to_byte_position(new_cursor);
        let end_byte = self.cursor_byte_position();
        self.buffer.drain(start_byte..end_byte);
        self.cursor = new_cursor;
    }

    /// Clear the entire buffer
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    /// Confirm edit and return the final value
    pub fn confirm(self) -> String {
        self.buffer
    }

    /// Cancel edit and return the original value
    #[allow(dead_code)]
    pub fn cancel(self) -> String {
        self.original
    }

    /// Check if buffer has been modified
    #[allow(dead_code)]
    pub fn is_modified(&self) -> bool {
        self.buffer != self.original
    }

    /// Get byte position for current cursor
    fn cursor_byte_position(&self) -> usize {
        self.char_to_byte_position(self.cursor)
    }

    /// Convert character position to byte position
    fn char_to_byte_position(&self, char_pos: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.buffer.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_editor() {
        let edit = InlineEdit::new("hello");
        assert_eq!(edit.buffer(), "hello");
        assert_eq!(edit.cursor(), 5); // Cursor at end
    }

    #[test]
    fn test_insert() {
        let mut edit = InlineEdit::new("hllo");
        edit.cursor = 1;
        edit.insert('e');
        assert_eq!(edit.buffer(), "hello");
        assert_eq!(edit.cursor(), 2);
    }

    #[test]
    fn test_backspace() {
        let mut edit = InlineEdit::new("hello");
        edit.backspace();
        assert_eq!(edit.buffer(), "hell");
        assert_eq!(edit.cursor(), 4);
    }

    #[test]
    fn test_delete() {
        let mut edit = InlineEdit::new("hello");
        edit.cursor = 0;
        edit.delete();
        assert_eq!(edit.buffer(), "ello");
        assert_eq!(edit.cursor(), 0);
    }

    #[test]
    fn test_cursor_movement() {
        let mut edit = InlineEdit::new("hello");
        assert_eq!(edit.cursor(), 5);

        edit.left();
        assert_eq!(edit.cursor(), 4);

        edit.home();
        assert_eq!(edit.cursor(), 0);

        edit.right();
        assert_eq!(edit.cursor(), 1);

        edit.end();
        assert_eq!(edit.cursor(), 5);
    }

    #[test]
    fn test_unicode() {
        let mut edit = InlineEdit::new("héllo");
        assert_eq!(edit.cursor(), 5);

        edit.backspace();
        assert_eq!(edit.buffer(), "héll");

        edit.cursor = 1;
        edit.delete();
        assert_eq!(edit.buffer(), "hll");
    }

    #[test]
    fn test_confirm_cancel() {
        let edit = InlineEdit::new("original");
        assert!(!edit.is_modified());

        let mut edit = InlineEdit::new("original");
        edit.clear();
        edit.insert_str("modified");
        assert!(edit.is_modified());
        assert_eq!(edit.confirm(), "modified");

        let mut edit = InlineEdit::new("original");
        edit.insert_str("_modified");
        assert_eq!(edit.cancel(), "original");
    }

    #[test]
    fn test_delete_word() {
        let mut edit = InlineEdit::new("hello world");
        edit.delete_word();
        assert_eq!(edit.buffer(), "hello ");

        edit.delete_word();
        assert_eq!(edit.buffer(), "");
    }
}
