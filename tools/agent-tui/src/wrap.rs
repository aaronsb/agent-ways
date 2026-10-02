//! The one width measure and the one wrap of agent-tui. Text wraps to a
//! width in terminal columns: words move whole to the next row, a word
//! longer than a row is broken, an explicit newline starts a row, the
//! spaces that open a line are kept as its indent, and spaces at a break
//! are dropped. The unit is the grapheme cluster, so a combining mark or a
//! joined emoji is never split, and widths are display widths, so a wide
//! character takes two columns. The feed, the markdown renderer and the
//! text entry all wrap through here.
//!
//! `agent_fmt::width` is another measure, for plain ANSI output: it skips
//! escape sequences and counts no East Asian width. Screens use this one.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The columns `c` takes; a control character takes none.
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// The columns `s` takes.
pub fn str_width(s: &str) -> usize {
    s.width()
}

/// `s` cut to `max` columns, the last kept column a `…` when anything was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if str_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for g in s.graphemes(true) {
        let w = str_width(g);
        if used + w + 1 > max {
            break;
        }
        out.push_str(g);
        used += w;
    }
    if max > 0 {
        out.push('…');
    }
    out
}

/// One grapheme cluster of a line being wrapped, with its style. A
/// `sticky` cell is never a break: a space marked sticky (the input's
/// cursor) stays on its row like a letter.
#[derive(Debug, Clone)]
pub(crate) struct Cell {
    pub text: String,
    pub style: Style,
    pub sticky: bool,
}

impl Cell {
    fn width(&self) -> usize {
        str_width(&self.text)
    }

    fn is_break(&self) -> bool {
        !self.sticky && self.text == " "
    }

    fn is_lone(&self) -> bool {
        self.sticky && self.text == " "
    }
}

/// A row of cells back into a line, joining runs of one style into a span.
pub(crate) fn line_of(cells: &[Cell], base: Style) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for c in cells {
        match spans.last_mut() {
            Some(last) if last.style == c.style => last.content.to_mut().push_str(&c.text),
            _ => spans.push(Span::styled(c.text.clone(), c.style)),
        }
    }
    Line::from(spans).style(base)
}

/// Wrap one line to `width` columns. A newline inside a span starts a new
/// row. An empty line stays one empty row.
pub fn wrap_line(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    let mut logical: Vec<Vec<Cell>> = vec![Vec::new()];
    for span in &line.spans {
        for g in span.content.graphemes(true) {
            if g == "\n" || g == "\r\n" {
                logical.push(Vec::new());
            } else {
                logical.last_mut().expect("one row at least").push(Cell { text: g.to_string(), style: span.style, sticky: false });
            }
        }
    }
    logical.iter().flat_map(|cells| wrap_cells(cells, width, true)).map(|r| line_of(&r, line.style)).collect()
}

/// Wrap a run of cells with no newline in it. With `trim`, as prose wraps,
/// spaces at the end of a row go too; without, as the text entry wraps,
/// every typed space keeps its column.
pub(crate) fn wrap_cells(cells: &[Cell], width: usize, trim: bool) -> Vec<Vec<Cell>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut used = 0;
    let mut i = 0;
    while i < cells.len() {
        // The next word and the breaks before it.
        let start = i;
        while i < cells.len() && cells[i].is_break() {
            i += 1;
        }
        let spaces = &cells[start..i];
        let wstart = i;
        while i < cells.len() && !cells[i].is_break() {
            // A sticky space is a word of its own: it stays where it is
            // without gluing the words either side together.
            if cells[i].is_lone() {
                if i == wstart {
                    i += 1;
                }
                break;
            }
            i += 1;
        }
        let word = &cells[wstart..i];
        if trim && word.is_empty() && !row.is_empty() {
            // Spaces that end the text: nothing follows them.
            break;
        }
        let sw: usize = spaces.iter().map(Cell::width).sum();
        let ww: usize = word.iter().map(Cell::width).sum();
        if used + sw + ww <= width {
            row.extend_from_slice(spaces);
            row.extend_from_slice(word);
            used += sw + ww;
            continue;
        }
        // The word does not fit after the spaces: it starts a row of its
        // own, the spaces dropped, unless the row is empty (leading spaces).
        if used > 0 {
            rows.push(std::mem::take(&mut row));
            used = 0;
        } else {
            for c in spaces {
                let w = c.width();
                if used + w > width {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                row.push(c.clone());
                used += w;
            }
        }
        // A word longer than a row is broken across rows, between clusters.
        for c in word {
            let w = c.width();
            if used + w > width && used > 0 {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(c.clone());
            used += w;
        }
    }
    rows.push(row);
    if trim {
        for r in &mut rows {
            while r.last().is_some_and(Cell::is_break) {
                r.pop();
            }
        }
    }
    rows
}

/// Wrap every line of `lines` to `width` columns.
pub fn wrap_lines(lines: &[Line<'_>], width: usize) -> Vec<Line<'static>> {
    lines.iter().flat_map(|l| wrap_line(l, width)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn words_move_whole_to_the_next_row() {
        let l = Line::from("the quick brown fox");
        assert_eq!(texts(&wrap_line(&l, 10)), ["the quick", "brown fox"]);
    }

    #[test]
    fn a_long_word_is_broken_and_a_newline_starts_a_row() {
        assert_eq!(texts(&wrap_line(&Line::from("abcdefghij"), 4)), ["abcd", "efgh", "ij"]);
        assert_eq!(texts(&wrap_line(&Line::from(vec![Span::raw("a\n\nb")]), 4)), ["a", "", "b"]);
        assert_eq!(texts(&wrap_line(&Line::from(""), 4)), [""]);
    }

    #[test]
    fn styles_survive_the_wrap() {
        let bold = Style::new().add_modifier(ratatui::style::Modifier::BOLD);
        let l = Line::from(vec![Span::raw("plain "), Span::styled("bold words", bold)]);
        let rows = wrap_line(&l, 11);
        assert_eq!(texts(&rows), ["plain bold", "words"]);
        assert_eq!(rows[1].spans[0].style, bold);
    }

    #[test]
    fn wide_characters_take_two_columns() {
        assert_eq!(str_width("日本"), 4);
        assert_eq!(texts(&wrap_line(&Line::from("日本語"), 4)), ["日本", "語"]);
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }

    #[test]
    fn an_opening_indent_is_kept_and_spaces_at_a_break_go() {
        let rows = texts(&wrap_line(&Line::from("  one two three   "), 9));
        assert_eq!(rows, ["  one two", "three"]);
    }

    #[test]
    fn a_cluster_is_never_split() {
        // `e` and a combining acute are one cluster; so is a ZWJ family.
        let decomposed = "cafe\u{301}cafe\u{301}";
        let rows = texts(&wrap_line(&Line::from(decomposed), 4));
        assert_eq!(rows, ["cafe\u{301}", "cafe\u{301}"]);
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        let rows = texts(&wrap_line(&Line::from(format!("ab{family}")), 3));
        assert!(rows.iter().any(|r| r.contains(family)), "{rows:?}");
        assert_eq!(truncate(&format!("abc{family}xyz"), 5).matches('\u{200d}').count() % 2, 0);
    }
}
