//! Markdown → ANSI renderer for the drill-down way body (ADR-154 §2 viewer).
//!
//! The way body shown in the why-detail panel is authored markdown; printing its
//! raw `#`/`**`/`` ` `` symbols is noisy to read. This maps a CommonMark parse
//! (pulldown-cmark) to SGR-styled display lines. The micro-compositor measures
//! width with `visible_len` (which ignores SGR) and seals styles on truncation,
//! so inline styling composes cleanly with the panel's wrapping and padding.
//!
//! The *parsing* — the fragile part — is pulldown-cmark's; this file only maps
//! its event stream to escape codes. Styles are tracked as a stack and
//! re-materialized (reset + re-apply) on each text run, so nested emphasis never
//! has to reason about SGR off-codes (bold-off `22` also clears dim, etc.).

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

const RST: &str = "\x1b[0m";
const CODE: &str = "\x1b[33m"; // inline `code` / fenced blocks: yellow

fn heading_sgr(level: HeadingLevel) -> &'static str {
    // Typographic ladder: weight for the top levels, italic below, underline
    // marking only H1 — so the levels read distinctly by style, with cyan
    // grouping the headings (H4+ drop the color as they recede).
    match level {
        HeadingLevel::H1 => "\x1b[1;4;36m", // bold + underline + cyan
        HeadingLevel::H2 => "\x1b[1;36m",   // bold + cyan
        HeadingLevel::H3 => "\x1b[3;36m",   // italic + cyan
        _ => "\x1b[3m",                     // italic
    }
}

/// End the current line: seal any open style, then push it and clear `cur`.
fn flush(lines: &mut Vec<String>, cur: &mut String) {
    if cur.contains('\x1b') {
        cur.push_str(RST);
    }
    lines.push(std::mem::take(cur));
}

/// Append a text run showing *exactly* the active styles: reset first, then
/// re-apply the stack. The reset is unconditional on purpose — an inline span's
/// `End` only pops the stack (it never emits an off-code), so a plain run right
/// after a styled span would otherwise inherit the popped style and bleed its
/// color/weight onto following text (and, in a table cell, onto later text in the
/// same cell). Resetting every run keeps each run scoped to its own styles.
fn emit(cur: &mut String, styles: &[&str], text: &str) {
    cur.push_str(RST);
    for s in styles {
        cur.push_str(s);
    }
    cur.push_str(text);
}

/// Inline `code`: distinct color, then restore the surrounding style stack so
/// text after the span keeps its context (e.g. code inside a heading).
fn emit_code(cur: &mut String, styles: &[&str], text: &str) {
    cur.push_str(RST);
    for s in styles {
        cur.push_str(s);
    }
    cur.push_str(CODE);
    cur.push_str(text);
    cur.push_str(RST);
    for s in styles {
        cur.push_str(s);
    }
}

/// Wrap a styled cell to `width` visible columns, carrying the active SGR across
/// each break so a style spanning a wrap resumes on the next line. Breaks at the
/// column edge (not word boundaries) so the width is respected exactly — a narrow
/// column may split a word, which keeps the grid aligned.
fn wrap_cell(cell: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut vis = 0usize; // visible cols on the current line
    let mut active = String::new(); // SGR seen since the last reset, to resume

    let mut it = cell.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            // Consume the whole escape sequence; track it for post-wrap resume.
            let mut esc = String::from(c);
            for d in it.by_ref() {
                esc.push(d);
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            if esc == RST {
                active.clear();
            } else {
                active.push_str(&esc);
            }
            line.push_str(&esc);
        } else {
            if vis == width {
                if line.contains('\x1b') {
                    line.push_str(RST);
                }
                lines.push(std::mem::take(&mut line));
                line.push_str(&active); // resume the open style on the next line
                vis = 0;
            }
            line.push(c);
            vis += 1;
        }
    }
    if vis > 0 || lines.is_empty() {
        if line.contains('\x1b') {
            line.push_str(RST);
        }
        lines.push(line);
    }
    lines
}

/// Emit a buffered table, fitting it to `width` visible columns. Columns start at
/// their widest cell; if a row exceeds the budget the widest columns are shrunk
/// (down to a floor) and over-long cells wrap across display lines — so a row can
/// become several lines, but the grid stays within `width` and the dividers stay
/// aligned. When the table already fits, columns keep their natural widths.
fn render_table(lines: &mut Vec<String>, rows: &[(bool, Vec<String>)], width: usize) {
    use crate::cmd::compositor::{pad_visible, visible_len};

    const MIN_COL: usize = 4;
    let ncols = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return;
    }

    // Natural column widths (widest visible cell), then shrink to fit the budget.
    let mut w = vec![0usize; ncols];
    for (_, r) in rows {
        for (c, cellstr) in r.iter().enumerate() {
            w[c] = w[c].max(visible_len(cellstr));
        }
    }
    let dividers = ncols.saturating_sub(1) * 3; // " │ " between each column pair
    let budget = width.saturating_sub(dividers);
    let mut sum: usize = w.iter().sum();
    while sum > budget {
        // Shrink the current widest column above the floor; stop if none can give.
        match w
            .iter()
            .enumerate()
            .filter(|(_, &x)| x > MIN_COL)
            .max_by_key(|(_, &x)| x)
            .map(|(i, _)| i)
        {
            Some(i) => {
                w[i] -= 1;
                sum -= 1;
            }
            None => break,
        }
    }
    let total: usize = w.iter().sum::<usize>() + dividers;

    for (i, (is_header, r)) in rows.iter().enumerate() {
        // Wrap each cell to its column width; the row's height is the tallest cell.
        let wrapped: Vec<Vec<String>> = w
            .iter()
            .enumerate()
            .map(|(c, &cw)| wrap_cell(r.get(c).map(String::as_str).unwrap_or(""), cw))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for k in 0..height {
            let mut line = String::new();
            for (c, segs) in wrapped.iter().enumerate() {
                let seg = segs.get(k).map(String::as_str).unwrap_or("");
                line.push_str(&pad_visible(seg, w[c]));
                if c + 1 < ncols {
                    line.push_str(" \x1b[2m│\x1b[0m "); // dim column divider
                }
            }
            if line.contains('\x1b') {
                line.push_str(RST);
            }
            lines.push(line);
        }
        // A dim rule under the (first) header row.
        if *is_header && i == 0 {
            lines.push(format!("\x1b[2m{}\x1b[0m", "─".repeat(total)));
        }
    }
}

/// Render `body` (a way's markdown) into ANSI-styled display lines, fitting
/// tables to `width` visible columns. Line-level structure (headings, list items,
/// rules, code fences, paragraph breaks) drives line breaks; inline structure
/// (strong/emphasis/strikethrough/links/code) drives styling within a line.
/// `width` bounds table layout only — prose/code lines are left for the panel to
/// truncate as before.
pub(super) fn render_markdown(body: &str, width: usize) -> Vec<String> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(body, opts);

    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut styles: Vec<&str> = Vec::new(); // active inline/block SGR sequences
    let mut list_ctr: Vec<Option<u64>> = Vec::new(); // ordered-list counters by depth
    let mut in_code_block = false;
    // Tables are buffered (a table's rows accumulate) so columns can be measured
    // and aligned in a second pass at TagEnd::Table.
    let mut in_table = false;
    let mut trows: Vec<(bool, Vec<String>)> = Vec::new(); // (is_header, styled cells)
    let mut trow: Vec<String> = Vec::new();
    let mut tcell = String::new();

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    if !cur.is_empty() {
                        flush(&mut lines, &mut cur);
                    }
                    styles.push(heading_sgr(level));
                }
                Tag::Strong => styles.push("\x1b[1m"),
                Tag::Emphasis => styles.push("\x1b[3m"),
                Tag::Strikethrough => styles.push("\x1b[9m"),
                Tag::Link { .. } => styles.push("\x1b[4;36m"), // underline + cyan
                Tag::List(start) => list_ctr.push(start),
                Tag::Item => {
                    let indent = "  ".repeat(list_ctr.len().saturating_sub(1));
                    match list_ctr.last_mut() {
                        Some(Some(n)) => {
                            let m = *n;
                            *n = m + 1;
                            cur.push_str(&format!("{indent}\x1b[2m{m}.\x1b[0m "));
                        }
                        _ => cur.push_str(&format!("{indent}\x1b[36m•\x1b[0m ")),
                    }
                }
                Tag::BlockQuote(_) => styles.push("\x1b[2m"), // dim
                Tag::CodeBlock(_) => {
                    if !cur.is_empty() {
                        flush(&mut lines, &mut cur);
                    }
                    in_code_block = true;
                }
                Tag::Table(_) => {
                    if !cur.is_empty() {
                        flush(&mut lines, &mut cur);
                    }
                    in_table = true;
                    trows.clear();
                }
                Tag::TableHead => {
                    trow.clear();
                    styles.push("\x1b[1m"); // header cells bold
                }
                Tag::TableRow => trow.clear(),
                Tag::TableCell => tcell.clear(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    styles.pop();
                    flush(&mut lines, &mut cur);
                    lines.push(String::new());
                }
                TagEnd::Strong
                | TagEnd::Emphasis
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::BlockQuote(_) => {
                    styles.pop();
                }
                TagEnd::Paragraph => {
                    flush(&mut lines, &mut cur);
                    lines.push(String::new());
                }
                TagEnd::Item => flush(&mut lines, &mut cur),
                TagEnd::List(_) => {
                    list_ctr.pop();
                    if list_ctr.is_empty() {
                        lines.push(String::new());
                    }
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    // Flush a final code line with no trailing newline; guarded so an
                    // already-empty buffer doesn't emit a spurious blank.
                    if !cur.is_empty() {
                        flush(&mut lines, &mut cur);
                    }
                    lines.push(String::new());
                }
                TagEnd::TableCell => {
                    if tcell.contains('\x1b') {
                        tcell.push_str(RST); // seal the cell before it's padded
                    }
                    trow.push(std::mem::take(&mut tcell));
                }
                TagEnd::TableHead => {
                    styles.pop();
                    trows.push((true, std::mem::take(&mut trow)));
                }
                TagEnd::TableRow => trows.push((false, std::mem::take(&mut trow))),
                TagEnd::Table => {
                    render_table(&mut lines, &trows, width);
                    in_table = false;
                    lines.push(String::new());
                }
                _ => {}
            },
            Event::Text(t) => {
                if in_table {
                    emit(&mut tcell, &styles, &t);
                } else if in_code_block {
                    // Fenced blocks carry their own newlines; one display line each.
                    // The split's trailing "" (from a terminating '\n') must not push
                    // a bare CODE, or it dangles into the next block (yellow bleed).
                    for (i, seg) in t.split('\n').enumerate() {
                        if i > 0 {
                            flush(&mut lines, &mut cur);
                        }
                        if !seg.is_empty() {
                            cur.push_str(CODE);
                            cur.push_str(seg);
                        }
                    }
                } else {
                    emit(&mut cur, &styles, &t);
                }
            }
            Event::Code(t) => {
                let target = if in_table { &mut tcell } else { &mut cur };
                emit_code(target, &styles, &t);
            }
            Event::SoftBreak => {
                if in_table {
                    tcell.push(' ');
                } else {
                    cur.push(' ');
                }
            }
            Event::HardBreak => {
                if in_table {
                    tcell.push(' ');
                } else {
                    flush(&mut lines, &mut cur);
                }
            }
            Event::Rule => {
                if !cur.is_empty() {
                    flush(&mut lines, &mut cur);
                }
                lines.push("\x1b[2m────────────────\x1b[0m".to_string());
            }
            _ => {}
        }
    }
    if !cur.is_empty() {
        flush(&mut lines, &mut cur);
    }
    // Drop trailing blank lines the block-close pushes (paragraph/list/heading).
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::compositor::visible_len;

    /// Render at a generous width so non-table content is unaffected.
    fn render(md: &str) -> Vec<String> {
        render_markdown(md, 100)
    }

    /// The visible (SGR-stripped) text of a rendered line.
    fn visible(line: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in line.chars() {
            if in_esc {
                if c.is_ascii_alphabetic() {
                    in_esc = false;
                }
            } else if c == '\x1b' {
                in_esc = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn heading_strips_hashes_and_styles() {
        let out = render("# Title\n\nbody");
        assert_eq!(visible(&out[0]), "Title", "hashes gone, text kept");
        assert!(out[0].contains("\x1b["), "heading is styled");
        assert!(out[0].ends_with(RST), "line seals its style");
    }

    #[test]
    fn inline_emphasis_is_stripped_from_visible_text() {
        let out = render("a **bold** and *italic* and `code` here");
        let v = visible(&out[0]);
        assert_eq!(v, "a bold and italic and code here", "markers gone: {v:?}");
        assert!(out[0].contains("\x1b[1m"), "bold SGR present");
        assert!(out[0].contains("\x1b[3m"), "italic SGR present");
    }

    #[test]
    fn styled_width_matches_plain_text() {
        // The compositor measures with visible_len; styling must not change width.
        let out = render("**bold** text");
        assert_eq!(visible_len(&out[0]), "bold text".len());
    }

    #[test]
    fn bullets_render_a_marker_not_a_dash() {
        let out = render("- first\n- second");
        assert!(visible(&out[0]).contains("first"));
        assert!(out[0].contains('•'), "unordered item shows a bullet glyph");
    }

    #[test]
    fn ordered_list_numbers_increment() {
        let out = render("1. one\n1. two");
        assert!(visible(&out[0]).starts_with("1."), "got: {:?}", visible(&out[0]));
        assert!(visible(&out[1]).starts_with("2."), "counter advanced: {:?}", visible(&out[1]));
    }

    #[test]
    fn empty_body_is_empty() {
        assert!(render("").is_empty());
    }

    #[test]
    fn inline_style_does_not_bleed_onto_following_text() {
        // Text after a **bold** span must not stay bold. The bold run is sealed
        // with a reset before the trailing plain text.
        let line = &render("a **b** c")[0];
        let bold_at = line.find("\x1b[1m").expect("bold present");
        let c_at = line.find(" c").expect("trailing text present");
        let between = &line[bold_at..c_at];
        assert!(
            between.contains(RST),
            "bold must be reset before the trailing text: {line:?}"
        );
    }

    #[test]
    fn paragraph_after_code_block_is_not_code_colored() {
        // The trailing empty segment of a fenced block must not leave CODE (yellow)
        // dangling into the next paragraph.
        let out = render("```\ncode line\n```\n\nafter para");
        let after = out
            .iter()
            .find(|l| visible(l).contains("after para"))
            .expect("paragraph rendered");
        assert!(
            !after.contains(CODE),
            "paragraph after a code block must not be code-colored: {after:?}"
        );
    }

    #[test]
    fn table_columns_align_on_the_divider() {
        let md = "| A | Bee |\n|---|---|\n| x | yy |\n| zzz | w |";
        let out = render(md);
        // Every rendered row that has a divider must place it at the same column
        // (the first column is padded to its widest cell: "zzz").
        let bars: Vec<usize> = out
            .iter()
            .filter_map(|l| visible(l).find('│'))
            .collect();
        assert!(bars.len() >= 3, "header + body rows carry a divider: {bars:?}");
        assert!(
            bars.windows(2).all(|w| w[0] == w[1]),
            "dividers vertically aligned across rows: {bars:?}"
        );
        // The header cell keeps its text (bold stripped from the visible form).
        assert!(visible(&out[0]).starts_with("A"), "got: {:?}", visible(&out[0]));
    }

    #[test]
    fn wide_table_wraps_and_stays_within_width() {
        // A 2-col table wider than the budget: the long cell must wrap across
        // display lines, no rendered line exceeds `width`, and no content is lost.
        let md = "| Key | Value |\n|---|---|\n| k | one two three four five six seven eight |";
        let width = 24;
        let out = render_markdown(md, width);
        for l in &out {
            assert!(
                visible_len(l) <= width,
                "line exceeds width {width}: {:?} (len {})",
                visible(l),
                visible_len(l)
            );
        }
        // No content is lost. Char-level wrap can split a word across lines, so
        // check the alphanumeric letter-stream (which reunites split words) rather
        // than whole-word survival.
        let letters: String = out
            .iter()
            .flat_map(|l| visible(l).chars().collect::<Vec<_>>())
            .filter(|c| c.is_alphanumeric())
            .collect();
        for word in ["one", "four", "eight"] {
            assert!(letters.contains(word), "wrapped cell dropped {word:?}: {letters:?}");
        }
    }

    #[test]
    fn narrow_table_dividers_still_align() {
        let md = "| A | Bee |\n|---|---|\n| xx | yyyy |";
        let out = render_markdown(md, 16);
        let bars: Vec<usize> = out.iter().filter_map(|l| visible(l).find('│')).collect();
        assert!(bars.len() >= 2, "rows carry a divider: {bars:?}");
        assert!(
            bars.windows(2).all(|w| w[0] == w[1]),
            "dividers aligned even when shrunk: {bars:?}"
        );
    }

    #[test]
    fn table_fits_naturally_when_it_has_room() {
        // With ample width, each row is a single line (no wrapping).
        let md = "| A | Bee |\n|---|---|\n| x | yy |";
        let out = render_markdown(md, 100);
        // header, rule, one body row, trailing blank trimmed → the body row is one line.
        let body_rows = out.iter().filter(|l| visible(l).contains('│')).count();
        assert_eq!(body_rows, 2, "header + 1 body row, each one line: {out:?}");
    }
}
