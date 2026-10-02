//! A text entry of one or more lines: the buffer, a cursor counted in
//! characters, the editing keys, and its rows at a width with the cursor
//! drawn as a reverse-video cell. Enter is the application's; Shift-Enter
//! and Alt-Enter insert a newline (Shift-Enter reaches only terminals that
//! speak the kitty keyboard protocol, Alt-Enter every terminal).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::wrap::char_width;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    text: String,
    /// In characters, from 0 to the character count.
    cursor: usize,
}

/// The byte offset of character `n` of `s`, or its length past the end.
fn byte_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(b, _)| b)
}

impl Input {
    pub fn new() -> Input {
        Input::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Replace the text and put the cursor at `cursor`, kept inside it.
    pub fn set(&mut self, text: impl Into<String>, cursor: usize) {
        self.text = text.into();
        self.cursor = cursor.min(self.len());
    }

    pub fn clear(&mut self) {
        self.set(String::new(), 0);
    }

    pub fn insert(&mut self, c: char) {
        let at = byte_at(&self.text, self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    pub fn newline(&mut self) {
        self.insert('\n');
    }

    /// Drop the character left of the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let at = byte_at(&self.text, self.cursor - 1);
        self.text.remove(at);
        self.cursor -= 1;
    }

    /// Drop the character under the cursor.
    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            let at = byte_at(&self.text, self.cursor);
            self.text.remove(at);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    /// The start of the cursor's line.
    pub fn home(&mut self) {
        let before = &self.text[..byte_at(&self.text, self.cursor)];
        self.cursor = before.rfind('\n').map_or(0, |i| before[..=i].chars().count());
    }

    /// The end of the cursor's line.
    pub fn end(&mut self) {
        let after = &self.text[byte_at(&self.text, self.cursor)..];
        self.cursor += after.find('\n').map_or(after.chars().count(), |b| after[..b].chars().count());
    }

    /// Apply an editing key. Returns whether the key was one: a character
    /// typed without Ctrl or Alt, Backspace, Delete, the arrows left and
    /// right, Home, End, and Shift- or Alt-Enter for a newline.
    pub fn key(&mut self, k: KeyEvent) -> bool {
        let m = k.modifiers;
        match k.code {
            KeyCode::Enter if m.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.newline(),
            KeyCode::Char(c) if !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            _ => return false,
        }
        true
    }

    /// The text broken into rows of `width` columns at any character, an
    /// explicit newline starting a row, with the cursor's cell in reverse
    /// video (a space past the end of a line). `style` is the text's.
    pub fn rows(&self, width: u16, style: Style) -> Vec<Line<'static>> {
        let width = (width as usize).max(1);
        let cursor_style = style.add_modifier(Modifier::REVERSED);
        let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
        let mut used = 0;
        let push = |rows: &mut Vec<Vec<Span<'static>>>, used: &mut usize, text: String, st: Style, w: usize| {
            if *used + w > width && *used > 0 {
                rows.push(Vec::new());
                *used = 0;
            }
            rows.last_mut().expect("one row at least").push(Span::styled(text, st));
            *used += w;
        };
        for (i, c) in self.text.chars().enumerate() {
            let here = i == self.cursor;
            if c == '\n' {
                if here {
                    push(&mut rows, &mut used, " ".into(), cursor_style, 1);
                }
                rows.push(Vec::new());
                used = 0;
                continue;
            }
            push(&mut rows, &mut used, c.to_string(), if here { cursor_style } else { style }, char_width(c));
        }
        if self.cursor >= self.len() {
            push(&mut rows, &mut used, " ".into(), cursor_style, 1);
        }
        rows.into_iter().map(merge).collect()
    }
}

/// Neighbouring spans of one style as one.
fn merge(spans: Vec<Span<'static>>) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(last) if last.style == s.style => last.content.to_mut().push_str(&s.content),
            _ => out.push(s),
        }
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, cursor: usize) -> Input {
        let mut i = Input::new();
        i.set(text, cursor);
        i
    }

    fn state(i: &Input) -> (&str, usize) {
        (i.text(), i.cursor())
    }

    #[test]
    fn typing_inserts_at_the_cursor_and_moves_it() {
        let mut i = at("ac", 1);
        i.insert('b');
        assert_eq!(state(&i), ("abc", 2));
        i.newline();
        assert_eq!(state(&i), ("ab\nc", 3));
    }

    #[test]
    fn backspace_and_delete_respect_the_ends_and_multibyte_characters() {
        let mut i = at("héllo", 0);
        i.backspace();
        assert_eq!(state(&i), ("héllo", 0), "backspace at the start does nothing");
        i.set("héllo", 2);
        i.backspace();
        assert_eq!(state(&i), ("hllo", 1));
        i.delete();
        assert_eq!(state(&i), ("hlo", 1));
        i.set("ab", 2);
        i.delete();
        assert_eq!(state(&i), ("ab", 2), "delete at the end does nothing");
    }

    #[test]
    fn home_and_end_stay_on_the_cursors_line() {
        let mut i = at("ab\ncde\nf", 4);
        i.home();
        assert_eq!(i.cursor(), 3);
        i.end();
        assert_eq!(i.cursor(), 6);
        i.set("abc", 1);
        i.home();
        assert_eq!(i.cursor(), 0);
        i.end();
        assert_eq!(i.cursor(), 3);
    }

    #[test]
    fn left_and_right_stop_at_the_ends() {
        let mut i = at("ab", 0);
        i.left();
        assert_eq!(i.cursor(), 0);
        i.right();
        i.right();
        i.right();
        assert_eq!(i.cursor(), 2);
    }

    #[test]
    fn editing_keys_are_taken_and_others_are_left() {
        let mut i = Input::new();
        assert!(i.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)));
        assert!(i.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)));
        assert_eq!(state(&i), ("x\n", 2));
        assert!(!i.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), "Enter is the application's");
        assert!(!i.key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT)), "Alt-digit is the application's");
        assert!(!i.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    }

    #[test]
    fn rows_break_at_the_width_and_mark_the_cursor() {
        let rows = at("abcdef", 6).rows(4, Style::new());
        let text: Vec<String> = rows.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert_eq!(text, ["abcd", "ef "]);
        let last = rows[1].spans.last().expect("the cursor");
        assert!(last.style.add_modifier.contains(Modifier::REVERSED));
        let rows = at("ab\ncd", 1).rows(10, Style::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].spans[1].content, "b");
        assert!(rows[0].spans[1].style.add_modifier.contains(Modifier::REVERSED));
    }
}
