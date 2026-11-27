//! Bounded editing at UTF-8 character boundaries.

use super::{MAX_LINE_BYTES, disallowed_control};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditError {
    LineFull,
    ControlCharacter,
}

/// Text under construction. Indices measure bytes and stay at character
/// boundaries, including when inserting or deleting multibyte characters.
pub struct LineBuffer {
    bytes: [u8; MAX_LINE_BYTES],
    len: usize,
    cursor: usize,
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineBuffer {
    pub const fn new() -> Self {
        Self {
            bytes: [0; MAX_LINE_BYTES],
            len: 0,
            cursor: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
    }

    /// Inserts a character at the cursor. Errors leave both text and cursor
    /// available for the caller to continue editing.
    pub fn insert(&mut self, ch: char) -> Result<(), EditError> {
        if disallowed_control(ch) {
            return Err(EditError::ControlCharacter);
        }
        let width = ch.len_utf8();
        if self.len + width > self.bytes.len() {
            return Err(EditError::LineFull);
        }
        let next = self.cursor + width;
        self.bytes.copy_within(self.cursor..self.len, next);
        ch.encode_utf8(&mut self.bytes[self.cursor..next]);
        self.cursor = next;
        self.len += width;
        Ok(())
    }

    pub fn move_left(&mut self) {
        if let Some(previous) = self.previous_boundary() {
            self.cursor = previous;
        }
    }

    pub fn move_right(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.cursor = next;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.len;
    }

    pub fn backspace(&mut self) {
        if let Some(previous) = self.previous_boundary() {
            self.remove(previous, self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.remove(self.cursor, next);
        }
    }

    fn remove(&mut self, start: usize, end: usize) {
        self.bytes.copy_within(end..self.len, start);
        self.len -= end - start;
        self.cursor = start;
    }

    fn previous_boundary(&self) -> Option<usize> {
        self.bytes[..self.cursor].iter().rposition(is_lead_byte)
    }

    fn next_boundary(&self) -> Option<usize> {
        if self.cursor == self.len {
            return None;
        }
        let start = self.cursor + 1;
        let offset = self.bytes[start..self.len].iter().position(is_lead_byte);
        Some(offset.map_or(self.len, |offset| start + offset))
    }
}

fn is_lead_byte(byte: &u8) -> bool {
    byte & 0b1100_0000 != 0b1000_0000
}
