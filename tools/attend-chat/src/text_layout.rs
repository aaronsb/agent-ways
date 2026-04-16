//! Char-boundary-aware text helpers for the compose box.
//!
//! These are the bits of `App` that are pure string math — no iocraft
//! types, no state handles. Extracting them keeps `app.rs` focused on
//! the component + event wiring, and makes the behaviour easy to test
//! without spinning up a terminal.

/// Count how many *rendered* rows `text` occupies inside a box of
/// `interior` columns. Approximate: counts chars, ignores grapheme
/// width — good enough for sizing a chat compose box where the exact
/// last-column edge doesn't matter.
pub fn visual_line_count(text: &str, interior: usize) -> usize {
    if interior == 0 {
        return 1;
    }
    let mut rows = 0usize;
    for line in text.split('\n') {
        let len = line.chars().count();
        rows += if len == 0 { 1 } else { len.div_ceil(interior) };
    }
    rows.max(1)
}

/// Split a string at a *char* offset (not a byte offset). If the
/// offset runs past the end of the string, the tail is empty.
pub fn split_at_char(s: &str, n: usize) -> (&str, &str) {
    match s.char_indices().nth(n) {
        Some((byte, _)) => s.split_at(byte),
        None => (s, ""),
    }
}

/// Render the compose buffer with a block cursor at `pos`. The cursor
/// sits *on* the char at `pos` (that char is replaced visually by the
/// block) or past the end if `pos == len`. Matches how most terminal
/// editors render a block cursor.
pub fn render_cursor(text: &str, pos: usize) -> String {
    let total = text.chars().count();
    if pos >= total {
        return format!("{}\u{2588}", text);
    }
    let (before, rest) = split_at_char(text, pos);
    // Skip the char under the cursor so the block doesn't overlap it.
    let after: String = rest.chars().skip(1).collect();
    format!("{}\u{2588}{}", before, after)
}

#[cfg(test)]
mod layout_tests {
    use super::visual_line_count;

    #[test]
    fn short_line_is_one_row() {
        assert_eq!(visual_line_count("hi", 20), 1);
    }

    #[test]
    fn wrap_on_width() {
        // 21 chars in a 10-wide box → 3 rows.
        assert_eq!(visual_line_count("abcdefghijklmnopqrstu", 10), 3);
    }

    #[test]
    fn explicit_newlines_count() {
        assert_eq!(visual_line_count("a\nb\nc", 20), 3);
    }

    #[test]
    fn combined_wrap_and_newline() {
        // "abcdefghij" (10 chars) wraps once in 5-wide → 2 rows
        // followed by "x" on its own line → 1 row = 3 total.
        assert_eq!(visual_line_count("abcdefghij\nx", 5), 3);
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::{render_cursor, split_at_char};

    #[test]
    fn split_within_ascii() {
        assert_eq!(split_at_char("hello", 2), ("he", "llo"));
    }

    #[test]
    fn split_past_end() {
        assert_eq!(split_at_char("hi", 10), ("hi", ""));
    }

    #[test]
    fn split_respects_char_boundaries() {
        // "héllo" — 'é' is 2 bytes. Splitting at char 2 should land
        // between 'é' and 'l', not mid-byte.
        let (before, after) = split_at_char("héllo", 2);
        assert_eq!(before, "hé");
        assert_eq!(after, "llo");
    }

    #[test]
    fn cursor_at_end_appends_block() {
        assert_eq!(render_cursor("hi", 2), "hi\u{2588}");
    }

    #[test]
    fn cursor_mid_text_replaces_char_visually() {
        // Cursor at position 1 over "abc" → 'a' + block + 'c'.
        assert_eq!(render_cursor("abc", 1), "a\u{2588}c");
    }

    #[test]
    fn cursor_on_empty_buffer() {
        assert_eq!(render_cursor("", 0), "\u{2588}");
    }
}
