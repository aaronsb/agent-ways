//! Markdown to styled text (ADR-504 §3, the document shape).
//!
//! pulldown-cmark parses; this maps its events to ratatui spans in the
//! theme's roles and wraps them to a width, so a caller knows how many
//! lines a document takes before it scrolls one. The styles are read from
//! the palette set for the frame ([`crate::theme::set`]), so build the
//! lines while drawing.
//!
//! Headings are the accent, bold down to the third level; strong is bold,
//! emphasis italic, strikethrough crossed out; inline code and code blocks
//! take the second accent; links are info; block quotes are muted behind a
//! bar; lists get a bullet or their number and a hanging indent; a rule is
//! a line of the rule colour; tables are laid out to the width, their
//! cells wrapped, the header bold. Prose wraps at spaces, and a word wider
//! than the line is cut; code is cut at the width, never at a space.

use agent_theme::Role;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme;

/// A run of text in one style.
type Run = (String, Style);

/// `md` as lines no wider than `width` columns.
pub fn render(md: &str, width: u16) -> Vec<Line<'static>> {
    let mut r = Renderer { width: (width as usize).max(1), ..Renderer::default() };
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    for ev in Parser::new_ext(md, opts) {
        r.event(ev);
    }
    r.finish()
}

/// A styled line word-wrapped to `width` columns, as [`render`] wraps
/// prose: for text drawn beside a rendered document that has to scroll
/// with it.
pub fn wrap_line(line: &Line, width: u16) -> Vec<Line<'static>> {
    let runs: Vec<Run> = line.spans.iter().map(|s| (s.content.to_string(), line.style.patch(s.style))).collect();
    if runs_width(&runs) == 0 {
        return vec![Line::raw("")];
    }
    wrap(&runs, (width as usize).max(1)).into_iter().map(to_line).collect()
}

/// The second accent, which code takes.
fn code_style() -> Style {
    theme::current().painter.ratatui(Role::Alt)
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 | HeadingLevel::H2 => theme::accent().add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => theme::accent(),
        _ => Style::new().add_modifier(Modifier::BOLD),
    }
}

/// One open list item: the width of its marker, and the marker itself
/// until the item's first line takes it.
struct Item {
    width: usize,
    marker: Option<Vec<Run>>,
}

/// A table being read: its rows of cells, each cell its runs, and whether a
/// row is the header.
#[derive(Default)]
struct Table {
    rows: Vec<(bool, Vec<Vec<Run>>)>,
    row: Vec<Vec<Run>>,
    cell: Vec<Run>,
    head: bool,
}

#[derive(Default)]
struct Renderer {
    width: usize,
    out: Vec<Line<'static>>,
    /// The inline text of the line being built.
    cur: Vec<Run>,
    /// Inline and block styles, patched in order.
    styles: Vec<Style>,
    /// The counter of each open list: `None` for a bulleted one.
    lists: Vec<Option<u64>>,
    items: Vec<Item>,
    quote: usize,
    code: bool,
    table: Option<Table>,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.iter().fold(Style::new(), |s, p| s.patch(*p))
    }

    fn push(&mut self, text: &str, style: Style) {
        if let Some(t) = &mut self.table {
            t.cell.push((text.to_string(), style));
        } else {
            self.cur.push((text.to_string(), style));
        }
    }

    /// A blank line, unless the last line is blank already or nothing is out yet.
    fn blank(&mut self) {
        if self.out.last().is_some_and(|l| l.width() > 0) {
            self.out.push(Line::raw(""));
        }
    }

    /// The prefix of the next line: the quote bars, then each open item's
    /// indent, or its marker if its first line has not been drawn.
    fn prefix(&mut self, first: bool) -> Vec<Run> {
        let mut p: Vec<Run> = Vec::new();
        for _ in 0..self.quote {
            p.push(("│ ".into(), theme::muted()));
        }
        for it in &mut self.items {
            match it.marker.take().filter(|_| first) {
                Some(m) => p.extend(m),
                None => p.push((" ".repeat(it.width), Style::new())),
            }
        }
        p
    }

    /// Emit the line being built, wrapped, under its prefix. An item whose
    /// marker is still waiting gets it even with nothing after it.
    fn flush(&mut self) {
        let waiting = self.items.iter().any(|i| i.marker.is_some());
        if self.cur.is_empty() && !waiting {
            return;
        }
        let runs = std::mem::take(&mut self.cur);
        let first = self.prefix(true);
        let rest = self.prefix(false);
        let room = self.width.saturating_sub(runs_width(&rest)).max(1);
        let lines = if self.code { cut(&runs, room) } else { wrap(&runs, room) };
        for (i, l) in lines.into_iter().enumerate() {
            let mut spans: Vec<Run> = if i == 0 { first.clone() } else { rest.clone() };
            spans.extend(l);
            self.out.push(to_line(spans));
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) if self.code => {
                // A code block's text carries its own line ends.
                let style = code_style();
                let mut parts = t.split('\n').peekable();
                while let Some(seg) = parts.next() {
                    if !seg.is_empty() {
                        self.cur.push((seg.to_string(), style));
                    }
                    if parts.peek().is_some() {
                        if self.cur.is_empty() {
                            let p = self.prefix(true);
                            self.out.push(to_line(p));
                        } else {
                            self.flush();
                        }
                    }
                }
            }
            Event::Text(t) => {
                let s = self.style();
                self.push(&t, s);
            }
            Event::Code(t) => {
                let s = self.style().patch(code_style());
                self.push(&t, s);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let s = self.style().patch(theme::muted());
                self.push(t.trim_end_matches('\n'), s);
            }
            Event::SoftBreak => self.push(" ", Style::new()),
            Event::HardBreak => {
                if self.table.is_some() {
                    self.push(" ", Style::new());
                } else {
                    self.flush();
                }
            }
            Event::Rule => {
                self.flush();
                let w = self.width.saturating_sub(runs_width(&self.prefix(false)));
                let mut l = self.prefix(true);
                l.push(("─".repeat(w), theme::rule()));
                self.out.push(to_line(l));
                self.blank();
            }
            Event::TaskListMarker(done) => {
                let s = if done { theme::ok() } else { theme::muted() };
                self.push(if done { "[x] " } else { "[ ] " }, s);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                // A paragraph inside a tight item follows its marker; elsewhere
                // it starts its own line.
                self.flush();
            }
            Tag::Heading { level, .. } => {
                self.flush();
                self.styles.push(heading_style(level));
            }
            Tag::Strong => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Emphasis => self.styles.push(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strikethrough => self.styles.push(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.styles.push(theme::info()),
            Tag::Image { .. } => self.styles.push(theme::muted()),
            Tag::BlockQuote(_) => {
                self.flush();
                self.quote += 1;
                self.styles.push(theme::muted());
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.code = true;
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = *n;
                        *n += 1;
                        (format!("{m}. "), theme::muted())
                    }
                    _ => ("• ".to_string(), theme::accent()),
                };
                let width = marker.0.width();
                self.items.push(Item { width, marker: Some(vec![marker]) });
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Table::default());
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.head = true;
                    t.row.clear();
                }
                self.styles.push(Style::new().add_modifier(Modifier::BOLD));
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(t) = &mut self.table {
                    t.cell.clear();
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                if self.items.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Heading(_) => {
                self.flush();
                self.styles.pop();
                self.blank();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image => {
                self.styles.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
                self.styles.pop();
                self.blank();
            }
            TagEnd::CodeBlock => {
                self.flush();
                self.code = false;
                self.blank();
            }
            TagEnd::Item => {
                self.flush();
                self.items.pop();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = &mut self.table {
                    let cell = std::mem::take(&mut t.cell);
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                self.styles.pop();
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push((true, row));
                    t.head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push((false, row));
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    let prefix = self.prefix(false);
                    let room = self.width.saturating_sub(runs_width(&prefix));
                    for l in table_lines(&t.rows, room) {
                        let mut spans = prefix.clone();
                        spans.extend(l);
                        self.out.push(to_line(spans));
                    }
                }
                self.blank();
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush();
        while self.out.last().is_some_and(|l| l.width() == 0) {
            self.out.pop();
        }
        self.out
    }
}

fn runs_width(runs: &[Run]) -> usize {
    runs.iter().map(|(t, _)| t.width()).sum()
}

/// Runs as a line, neighbours of one style joined.
fn to_line(runs: Vec<Run>) -> Line<'static> {
    let mut merged: Vec<Run> = Vec::new();
    for (t, s) in runs {
        if t.is_empty() {
            continue;
        }
        match merged.last_mut() {
            Some((lt, ls)) if *ls == s => lt.push_str(&t),
            _ => merged.push((t, s)),
        }
    }
    Line::from(merged.into_iter().map(|(t, s)| Span::styled(t, s)).collect::<Vec<_>>())
}

/// Split runs into words and the spaces between them, each with its style.
fn pieces(runs: &[Run]) -> Vec<(String, Style, bool)> {
    let mut out: Vec<(String, Style, bool)> = Vec::new();
    for (text, style) in runs {
        let mut word = String::new();
        let mut space = false;
        for c in text.chars() {
            let is_space = c == ' ';
            if !word.is_empty() && is_space != space {
                out.push((std::mem::take(&mut word), *style, space));
            }
            space = is_space;
            word.push(c);
        }
        if !word.is_empty() {
            out.push((word, *style, space));
        }
    }
    out
}

/// Word-wrap runs to `width` columns. Spaces at a break are dropped; a word
/// wider than the line is cut.
fn wrap(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut w = 0usize;
    // Spaces that open the text are kept, as an indent; spaces at a break
    // are not.
    let mut broke = false;
    for (text, style, space) in pieces(runs) {
        let tw = text.width();
        if space {
            if w + tw <= width && (w > 0 || !broke) {
                lines.last_mut().expect("a line").push((text, style));
                w += tw;
            } else if w > 0 {
                lines.push(Vec::new());
                w = 0;
                broke = true;
            }
            continue;
        }
        broke = true;
        if w + tw > width && w > 0 {
            // Spaces left at the end of the line go with the break.
            trim_end(lines.last_mut().expect("a line"));
            lines.push(Vec::new());
            w = 0;
        }
        if tw <= width {
            lines.last_mut().expect("a line").push((text, style));
            w += tw;
            continue;
        }
        for c in text.chars() {
            let cw = c.width().unwrap_or(0);
            if w + cw > width && w > 0 {
                lines.push(Vec::new());
                w = 0;
            }
            lines.last_mut().expect("a line").push((c.to_string(), style));
            w += cw;
        }
    }
    if let Some(l) = lines.last_mut() {
        trim_end(l);
    }
    lines
}

fn trim_end(line: &mut Vec<Run>) {
    while line.last().is_some_and(|(t, _)| t.trim_end_matches(' ').is_empty()) {
        line.pop();
    }
    if let Some((t, _)) = line.last_mut() {
        let k = t.trim_end_matches(' ').len();
        t.truncate(k);
    }
}

/// Cut runs at `width` columns, spaces kept: code.
fn cut(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut w = 0usize;
    for (text, style) in runs {
        for c in text.chars() {
            let cw = c.width().unwrap_or(0);
            if w + cw > width && w > 0 {
                lines.push(Vec::new());
                w = 0;
            }
            lines.last_mut().expect("a line").push((c.to_string(), *style));
            w += cw;
        }
    }
    lines
}

/// A table laid out in `width` columns: each column as wide as its widest
/// cell, the widest shrunk first while the row is too wide, down to a floor;
/// cells wrap within their column, and ` │ ` divides the columns.
fn table_lines(rows: &[(bool, Vec<Vec<Run>>)], width: usize) -> Vec<Vec<Run>> {
    const FLOOR: usize = 4;
    let n = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
    if n == 0 {
        return Vec::new();
    }
    let mut w = vec![0usize; n];
    for (_, r) in rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(runs_width(c));
        }
    }
    let dividers = (n - 1) * 3;
    let budget = width.saturating_sub(dividers);
    while w.iter().sum::<usize>() > budget {
        let Some(i) = (0..n).filter(|i| w[*i] > FLOOR).max_by_key(|i| w[*i]) else { break };
        w[i] -= 1;
    }
    let mut out = Vec::new();
    for (k, (head, r)) in rows.iter().enumerate() {
        let empty = Vec::new();
        let cells: Vec<Vec<Vec<Run>>> = (0..n).map(|i| wrap(r.get(i).unwrap_or(&empty), w[i])).collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            let mut l: Vec<Run> = Vec::new();
            for i in 0..n {
                let seg = cells[i].get(line).cloned().unwrap_or_default();
                let pad = w[i].saturating_sub(runs_width(&seg));
                l.extend(seg);
                if i + 1 < n {
                    l.push((" ".repeat(pad), Style::new()));
                    l.push((" │ ".into(), theme::rule()));
                }
            }
            out.push(l);
        }
        if *head && k == 0 {
            let rule: Vec<String> = w.iter().map(|cw| "─".repeat(*cw)).collect();
            out.push(vec![(rule.join("─┼─"), theme::rule())]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{set, Palette};
    use agent_theme::ColorDepth;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn md(src: &str, width: u16) -> Vec<Line<'static>> {
        set(Palette::terminal(ColorDepth::TrueColor));
        render(src, width)
    }

    fn span<'a>(l: &'a Line, needle: &str) -> &'a Span<'a> {
        l.spans.iter().find(|s| s.content.contains(needle)).unwrap_or_else(|| panic!("no span with {needle:?} in {:?}", text(l)))
    }

    #[test]
    fn a_heading_loses_its_hashes_and_takes_the_accent_in_bold() {
        let out = md("# Title\n\nbody", 40);
        assert_eq!(text(&out[0]), "Title");
        assert_eq!(out[0].spans[0].style, theme::accent().add_modifier(Modifier::BOLD));
        assert_eq!(text(&out[1]), "", "a blank line after the heading");
        assert_eq!(text(&out[2]), "body");
    }

    #[test]
    fn inline_marks_become_styles_and_leave_the_text() {
        let out = md("a **bold** and *it* and `code` and ~~gone~~ [link](http://x)", 80);
        assert_eq!(text(&out[0]), "a bold and it and code and gone link");
        assert!(span(&out[0], "bold").style.add_modifier.contains(Modifier::BOLD));
        assert!(span(&out[0], "it").style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(span(&out[0], "code").style, code_style());
        assert!(span(&out[0], "gone").style.add_modifier.contains(Modifier::CROSSED_OUT));
        assert_eq!(span(&out[0], "link").style, theme::info());
        // The text after a styled span is plain again.
        assert_eq!(span(&out[0], " and ").style, Style::new());
    }

    #[test]
    fn prose_wraps_at_spaces_to_the_width() {
        let out = md("one two three four five six seven", 12);
        let lines: Vec<String> = out.iter().map(text).collect();
        assert_eq!(lines, ["one two", "three four", "five six", "seven"]);
        assert!(out.iter().all(|l| l.width() <= 12));
    }

    #[test]
    fn a_word_wider_than_the_line_is_cut() {
        let out = md("abcdefghij", 4);
        let lines: Vec<String> = out.iter().map(text).collect();
        assert_eq!(lines, ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn list_items_hang_under_their_marker_and_numbers_count_up() {
        let out = md("- first item wraps here\n- second\n\n1. one\n1. two\n   - nested", 14);
        let lines: Vec<String> = out.iter().map(text).collect();
        assert_eq!(lines[0], "• first item");
        assert_eq!(lines[1], "  wraps here", "the wrap hangs under the text, not the bullet");
        assert_eq!(lines[2], "• second");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "1. one");
        assert_eq!(lines[5], "2. two");
        assert_eq!(lines[6], "   • nested");
        assert_eq!(out[0].spans[0].style, theme::accent(), "the bullet is the accent");
    }

    #[test]
    fn a_code_block_keeps_its_lines_and_spaces_and_is_cut_not_wrapped() {
        let out = md("```\nlet x  = 1;\nlong_line_here\n```\n\nafter", 10);
        let lines: Vec<String> = out.iter().map(text).collect();
        assert_eq!(lines[..4], ["let x  = 1", ";", "long_line_", "here"]);
        assert_eq!(out[0].spans[0].style, code_style());
        let after = out.iter().find(|l| text(l) == "after").expect("the paragraph after");
        assert_eq!(after.spans[0].style, Style::new(), "the code colour stops with the block");
    }

    #[test]
    fn a_quote_is_muted_behind_a_bar() {
        let out = md("> quoted words", 40);
        assert_eq!(text(&out[0]), "│ quoted words");
        assert!(out[0].spans.iter().all(|s| s.style == theme::muted()));
    }

    #[test]
    fn a_table_aligns_its_dividers_and_fits_the_width() {
        let out = md("| A | Bee |\n|---|---|\n| x | yy |\n| zzz | a long cell that wraps |", 24);
        let lines: Vec<String> = out.iter().map(text).collect();
        let bars: Vec<usize> = lines.iter().filter_map(|l| l.find('│')).collect();
        assert!(bars.len() >= 4, "{lines:?}");
        assert!(bars.windows(2).all(|p| p[0] == p[1]), "dividers align: {lines:?}");
        assert!(out.iter().all(|l| l.width() <= 24), "{lines:?}");
        assert!(lines[1].contains('┼'), "a rule under the header: {lines:?}");
        assert!(out[0].spans[0].style.add_modifier.contains(Modifier::BOLD), "the header is bold");
        let all: String = lines.join(" ");
        for w in ["long", "cell", "wraps"] {
            assert!(all.contains(w), "{w} kept: {lines:?}");
        }
    }

    #[test]
    fn a_rule_spans_the_width_and_nothing_trails() {
        let out = md("a\n\n---\n\nb\n\n", 8);
        let lines: Vec<String> = out.iter().map(text).collect();
        assert_eq!(lines, ["a", "", "────────", "", "b"]);
        assert_eq!(out[2].spans[0].style, theme::rule());
    }

    #[test]
    fn a_line_wraps_with_its_styles_kept() {
        set(Palette::terminal(ColorDepth::TrueColor));
        let l = Line::from(vec![Span::styled("  vocabulary: ", theme::muted()), Span::raw("alpha beta gamma")]);
        let out = wrap_line(&l, 20);
        let lines: Vec<String> = out.iter().map(text).collect();
        assert_eq!(lines, ["  vocabulary: alpha", "beta gamma"], "the indent that opens the line is kept");
        assert_eq!(out[0].spans[0].style, theme::muted());
        assert_eq!(wrap_line(&Line::raw(""), 5).len(), 1, "a blank line stays one line");
    }

    #[test]
    fn empty_input_is_no_lines() {
        assert!(md("", 10).is_empty());
    }
}
