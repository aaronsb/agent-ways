//! Wrapping styled text to a width in terminal columns: words move whole
//! to the next row, a word longer than a row is broken, and an explicit
//! newline starts a row. Widths are display widths, so a wide character
//! takes two columns.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

/// The columns `c` takes; a control character takes none.
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// The columns `s` takes.
pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// `s` cut to `max` columns, the last kept column a `…` when anything was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if str_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    if max > 0 {
        out.push('…');
    }
    out
}

/// One styled character of a line being wrapped.
type Cell = (char, Style);

/// Rows of cells back into a line, joining runs of one style into a span.
fn line_of(cells: &[Cell], base: Style) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut style = None;
    for (c, s) in cells {
        if style != Some(*s) {
            if let Some(st) = style {
                spans.push(Span::styled(std::mem::take(&mut run), st));
            }
            style = Some(*s);
        }
        run.push(*c);
    }
    if let Some(st) = style {
        spans.push(Span::styled(run, st));
    }
    Line::from(spans).style(base)
}

/// Wrap one line to `width` columns. A newline inside a span starts a new
/// row. An empty line stays one empty row.
pub fn wrap_line(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let base = line.style;
    // Logical rows: split on newlines first.
    let mut logical: Vec<Vec<Cell>> = vec![Vec::new()];
    for span in &line.spans {
        for c in span.content.chars() {
            if c == '\n' {
                logical.push(Vec::new());
            } else {
                logical.last_mut().expect("one row at least").push((c, span.style));
            }
        }
    }
    let mut out = Vec::new();
    for cells in logical {
        out.extend(wrap_cells(&cells, width).iter().map(|r| line_of(r, base)));
    }
    out
}

/// Wrap a run of cells with no newline in it.
fn wrap_cells(cells: &[Cell], width: usize) -> Vec<Vec<Cell>> {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut used = 0;
    let mut i = 0;
    while i < cells.len() {
        // The next word and the spaces before it.
        let start = i;
        while i < cells.len() && cells[i].0 == ' ' {
            i += 1;
        }
        let spaces = &cells[start..i];
        let wstart = i;
        while i < cells.len() && cells[i].0 != ' ' {
            i += 1;
        }
        let word = &cells[wstart..i];
        let sw: usize = spaces.iter().map(|c| char_width(c.0)).sum();
        let ww: usize = word.iter().map(|c| char_width(c.0)).sum();
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
                let w = char_width(c.0);
                if used + w > width {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                row.push(*c);
                used += w;
            }
        }
        // A word longer than a row is broken across rows.
        for c in word {
            let w = char_width(c.0);
            if used + w > width && used > 0 {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(*c);
            used += w;
        }
    }
    rows.push(row);
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
}
