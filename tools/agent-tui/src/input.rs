//! A text entry of one or more lines: the buffer, a cursor, the editing
//! keys, and its rows at a width with the cursor drawn as a reverse-video
//! cell. Enter is the application's; Shift-Enter and Alt-Enter insert a
//! newline (Shift-Enter reaches only terminals that speak the kitty keyboard
//! protocol, Alt-Enter every terminal).
//!
//! The cursor is counted in characters, so callers can splice the text by
//! character, but it only ever rests between grapheme clusters: moving and
//! deleting go a whole cluster at a time, so a combining mark or a joined
//! emoji is never split.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation;

use crate::wrap::{line_of, wrap_cells, Cell};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    text: String,
    /// In characters, on a cluster boundary, from 0 to the character count.
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

    /// The cursor, in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The cluster boundaries, in characters: 0, each cluster's end.
    fn bounds(&self) -> Vec<usize> {
        let mut out = vec![0];
        let mut n = 0;
        for g in self.text.graphemes(true) {
            n += g.chars().count();
            out.push(n);
        }
        out
    }

    /// The first boundary at or after `n`.
    fn snap(&self, n: usize) -> usize {
        self.bounds().into_iter().find(|b| *b >= n).unwrap_or_else(|| self.text.chars().count())
    }

    /// Replace the text and put the cursor at `cursor` characters, kept
    /// inside it and on a cluster boundary.
    pub fn set(&mut self, text: impl Into<String>, cursor: usize) {
        self.text = text.into();
        self.cursor = self.snap(cursor);
    }

    pub fn clear(&mut self) {
        self.set(String::new(), 0);
    }

    pub fn insert(&mut self, c: char) {
        let at = byte_at(&self.text, self.cursor);
        self.text.insert(at, c);
        // A combining mark joins the cluster before it; the cursor stays
        // after whatever cluster the character ended up in.
        self.cursor = self.snap(self.cursor + 1);
    }

    pub fn newline(&mut self) {
        self.insert('\n');
    }

    /// Remove the characters from `a` to `b`.
    fn cut(&mut self, a: usize, b: usize) {
        let (x, y) = (byte_at(&self.text, a), byte_at(&self.text, b));
        self.text.replace_range(x..y, "");
    }

    /// Drop the cluster left of the cursor.
    pub fn backspace(&mut self) {
        if let Some(prev) = self.bounds().into_iter().rev().find(|b| *b < self.cursor) {
            self.cut(prev, self.cursor);
            self.cursor = prev;
        }
    }

    /// Drop the cluster under the cursor.
    pub fn delete(&mut self) {
        if let Some(next) = self.bounds().into_iter().find(|b| *b > self.cursor) {
            self.cut(self.cursor, next);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.bounds().into_iter().rev().find(|b| *b < self.cursor).unwrap_or(0);
    }

    pub fn right(&mut self) {
        self.cursor = self.bounds().into_iter().find(|b| *b > self.cursor).unwrap_or(self.cursor);
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

    /// Where the cursor is drawn in [`Input::rows`] at `width`: its row,
    /// and its column in display cells.
    fn drawn_at(&self, width: u16) -> (usize, usize) {
        let rows = self.rows(width, Style::new());
        for (r, line) in rows.iter().enumerate() {
            let mut col = 0;
            for span in &line.spans {
                if span.style.add_modifier.contains(Modifier::REVERSED) {
                    return (r, col);
                }
                col += span.width();
            }
        }
        (rows.len().saturating_sub(1), 0)
    }

    /// Put the cursor where a click at `row` and display column `col` of
    /// [`Input::rows`] at `width` lands: on the cluster there, or past the
    /// end of the row when the click is beyond it. The cursor only moves.
    pub fn click(&mut self, width: u16, row: usize, col: usize) {
        // Where the cursor draws grows with it: search the boundaries for
        // the last one drawn at or before the click.
        let bounds = self.bounds();
        let mut probe = self.clone();
        let mut at = |b: usize| {
            probe.cursor = b;
            probe.drawn_at(width)
        };
        let (mut lo, mut hi) = (0, bounds.len() - 1);
        if at(bounds[0]) > (row, col) {
            self.cursor = 0;
            return;
        }
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if at(bounds[mid]) <= (row, col) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        self.cursor = bounds[lo];
    }

    /// The text word-wrapped to rows of `width` columns, an explicit newline
    /// starting a row, with the cursor's cluster in reverse video (a space
    /// past the end of a line). `style` is the text's.
    pub fn rows(&self, width: u16, style: Style) -> Vec<Line<'static>> {
        let cursor_style = style.add_modifier(Modifier::REVERSED);
        let mut logical: Vec<Vec<Cell>> = vec![Vec::new()];
        let mut at = 0;
        let cursor_cell = || Cell { text: " ".into(), style: cursor_style, sticky: true };
        for g in self.text.graphemes(true) {
            let here = at == self.cursor;
            at += g.chars().count();
            if g == "\n" || g == "\r\n" {
                if here {
                    logical.last_mut().expect("one row at least").push(cursor_cell());
                }
                logical.push(Vec::new());
                continue;
            }
            let st = if here { cursor_style } else { style };
            logical.last_mut().expect("one row at least").push(Cell { text: g.to_string(), style: st, sticky: here });
        }
        if self.cursor >= at {
            logical.last_mut().expect("one row at least").push(cursor_cell());
        }
        logical.iter().flat_map(|cells| wrap_cells(cells, width as usize, false)).map(|r| line_of(&r, Style::new())).collect()
    }
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

    fn texts(rows: &[Line]) -> Vec<String> {
        rows.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
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
    fn moves_and_deletes_go_by_grapheme_cluster() {
        // A decomposed é: the cursor never rests on the combining mark,
        // and backspace takes the letter and its mark together.
        let mut i = at("cafe\u{301}", 5);
        i.left();
        assert_eq!(i.cursor(), 3, "one step back crosses e and its accent");
        i.right();
        assert_eq!(i.cursor(), 5);
        i.backspace();
        assert_eq!(state(&i), ("caf", 3));
        // A ZWJ family is one cluster.
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        let mut i = at(&format!("a{family}b"), 0);
        i.right();
        i.right();
        assert_eq!(i.cursor(), 1 + family.chars().count());
        i.left();
        i.delete();
        assert_eq!(state(&i), ("ab", 1));
        // A cursor set inside a cluster snaps to its end.
        assert_eq!(at("cafe\u{301}", 4).cursor(), 5);
        // A combining mark typed after a letter joins it; the cursor follows.
        let mut i = at("e", 1);
        i.insert('\u{301}');
        assert_eq!(i.cursor(), 2);
        i.left();
        assert_eq!(i.cursor(), 0);
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

    /// A click puts the cursor on the cluster under it, at the end of a
    /// row clicked past its end, and at the end of the text below it.
    #[test]
    fn a_click_puts_the_cursor_under_it() {
        let mut i = at("hello there world", 0);
        // "hello there" / "world" at 12 columns.
        i.click(12, 0, 6);
        assert_eq!(i.cursor(), 6, "on the t");
        i.click(12, 1, 2);
        assert_eq!(i.cursor(), 14, "on the r of world");
        i.click(12, 0, 30);
        assert_eq!(i.cursor(), 11, "past the end of the first row: its end");
        i.click(12, 9, 0);
        assert_eq!(i.cursor(), 17, "below the text: its end");
        let mut i = at("ab\ncd", 0);
        i.click(10, 1, 1);
        assert_eq!(i.cursor(), 4);
        i.click(10, 0, 0);
        assert_eq!(i.cursor(), 0);
        let mut i = at("日本語", 0);
        i.click(10, 0, 3);
        assert_eq!(i.cursor(), 1, "a wide character takes two columns");
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
    fn rows_word_wrap_and_mark_the_cursor() {
        let rows = at("abcdef", 6).rows(4, Style::new());
        assert_eq!(texts(&rows), ["abcd", "ef "]);
        let last = rows[1].spans.last().expect("the cursor");
        assert!(last.style.add_modifier.contains(Modifier::REVERSED));
        let rows = at("ab\ncd", 1).rows(10, Style::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].spans[1].content, "b");
        assert!(rows[0].spans[1].style.add_modifier.contains(Modifier::REVERSED));
        // Words move whole, as the feed wraps them.
        assert_eq!(texts(&at("hello there world", 0).rows(12, Style::new())), ["hello there", "world"]);
        // The cursor on the space at a break stays visible.
        let rows = at("hello world", 5).rows(8, Style::new());
        assert_eq!(texts(&rows), ["hello ", "world"]);
        assert!(rows[0].spans[1].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(texts(&at("hello world", 6).rows(8, Style::new())), ["hello", "world"], "a cursor on a letter splits nothing");
        // A cluster under the cursor is drawn whole.
        let rows = at("cafe\u{301}!", 3).rows(10, Style::new());
        assert!(rows[0].spans.iter().any(|s| s.content == "e\u{301}" && s.style.add_modifier.contains(Modifier::REVERSED)));
    }
}
