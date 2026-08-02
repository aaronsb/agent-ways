//! Hard-wrap detection and repair for markdown prose (#415).
//!
//! Agents hard-wrap markdown at ~80 columns by habit. That breaks `Edit`
//! (`old_string` must reproduce arbitrary interior line breaks) and makes
//! diffs non-semantic (three changed words reflow a whole paragraph). This
//! module finds mechanically wrapped prose and flattens it.
//!
//! Three callers share these functions, which is why they live here rather
//! than in a binary: the `ways lint` rule (in-process, no spawn per file),
//! the way's `postcheck.sh` predicate (via the thin binary, on freshly
//! written text), and explicit remediation.
//!
//! # Three layers, because one is not enough
//!
//! - **Eligibility** comes from a real CommonMark parser ([`pulldown_cmark`]),
//!   not from hand-rolled line matching. The hand-rolled version accumulated ten
//!   distinct construct bugs — empty blockquote lines, raw HTML bodies, HTML
//!   comments, tab indentation, reference definitions — each found one review at
//!   a time. That is the signature of reimplementing a specification: the defects
//!   are the parts of the spec you did not know to handle.
//! - **Whether to touch** is [`detect`]'s heuristic, and it stays hand-written
//!   because no parser can answer it. Joining two authored sentences produces an
//!   identical document; only a heuristic can decline to do it.
//! - **Whether the result is sound** is [`structure_preserved`], which reparses
//!   and compares. This is the layer that does not depend on enumerating
//!   constructs correctly — and it caught a live corruption (a multi-line HTML
//!   comment in `attend.md`) that the token check passed.
//!
//! # Detection is a heuristic; the transform is not
//!
//! [`detect`] is deliberately conservative. It cannot *decide* whether a break
//! is mechanical — one-clause-per-line is a legitimate authoring style, and the
//! convention this supports permits it. What it can do is recognize the
//! fingerprint of column wrapping: a run of lines whose lengths cluster just
//! under a fill column, broken mid-phrase. Semantic line breaks land at clause
//! boundaries and vary wildly in length; wrapped prose does neither.
//!
//! Missing a wrapped paragraph is acceptable. Firing on deliberate structure is
//! not — a check that nags trains its reader to ignore it.
//!
//! # Why the transform is scoped to the enclosing paragraph
//!
//! Measured against the 138-file `hooks/ways` corpus, three scopes behave very
//! differently:
//!
//! - Flattening every paragraph modified 16 of the 115 files with no detected
//!   wrap at all — welding deliberately parallel sentences together and
//!   de-indenting nested list items.
//! - Flattening only the detected window left paragraphs half-flat: one joined
//!   line followed by the rest of the paragraph still wrapped.
//! - Flattening the **enclosing paragraph of a detected window** touched all 24
//!   detected files and none of the other 114.
//!
//! So detection is per-window, repair is per-paragraph, and a paragraph with
//! no detected window is never touched. Both sides now work on the same *unit*
//! — a paragraph split further at the boundaries the repair refuses to join
//! across (list items, labels, quote transitions, hard breaks) — so the span
//! detection flags is exactly the span a repair would flatten.

/// How a line participates in wrap detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Structural or literal. Never joined, never a wrap candidate.
    Verbatim,
    /// Blank — a paragraph boundary.
    Blank,
    /// Paragraph, list-item, or blockquote text.
    Prose,
}

/// A paragraph containing at least one detected hard-wrap window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedParagraph {
    /// First line index of the enclosing paragraph (0-based).
    pub start: usize,
    /// Last line index of the enclosing paragraph, inclusive.
    pub end: usize,
    /// First line index of the window that triggered detection.
    pub trigger_start: usize,
    /// Number of lines in the triggering window.
    pub trigger_len: usize,
    /// Mid-phrase breaks counted inside the triggering window.
    pub mid_breaks: usize,
}

/// Tuning for [`detect`]. The defaults are the measured ones; they are exposed
/// so tests can probe the boundaries, not so callers can invent policies.
///
/// Two floor rules soften these bounds where wrapping actually terminates. A
/// unit's final line is exempt from `min_len` and the band — the tail of a
/// wrap is short by construction, and requiring it to sit near the fill
/// column made every two-wrap paragraph with a short last sentence invisible.
/// And a window is only asked for as many mid-phrase breaks as it has line
/// breaks — `min_mid_breaks` is a cap, not a flat requirement — so a two-line
/// window qualifies on its single break.
#[derive(Debug, Clone, Copy)]
pub struct DetectConfig {
    /// Minimum line length to count as "near a fill column".
    pub min_len: usize,
    /// Maximum line length — beyond this it is wide content, not wrapped prose.
    pub max_len: usize,
    /// Maximum spread between the longest and shortest line in a window.
    pub band: usize,
    /// Minimum lines in a window.
    pub min_run: usize,
    /// Minimum mid-phrase breaks in a window, capped at the breaks it contains.
    pub min_mid_breaks: usize,
}

impl Default for DetectConfig {
    fn default() -> Self {
        Self { min_len: 55, max_len: 100, band: 12, min_run: 2, min_mid_breaks: 2 }
    }
}

/// Characters that end a line at a natural boundary. A line ending on one of
/// these was plausibly broken on purpose, so it is not evidence of wrapping.
const BOUNDARY_END: &[char] = &['.', '!', '?', ':', ';', '"', ')', '`', '*', '|', '>', ','];

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}


/// A list item: `- `, `* `, `+ `, `1. `, `2) `.
fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    let mut chars = t.chars();
    match chars.next() {
        Some('-') | Some('*') | Some('+') => chars.next() == Some(' '),
        Some(c) if c.is_ascii_digit() => {
            let rest: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
            let after = &t[rest.len()..];
            let mut a = after.chars();
            matches!(a.next(), Some('.') | Some(')')) && a.next() == Some(' ')
        }
        _ => false,
    }
}

fn is_blockquote(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

/// A blockquote line with no content — `>` alone. This is a *paragraph
/// boundary* inside the quote, not a wrapped line, and joining across one welds
/// two quoted paragraphs into one. The token-stream check cannot catch that: the
/// only difference is a bare `>`, which the invariant is allowed to drop.
fn is_empty_blockquote(line: &str) -> bool {
    is_blockquote(line) && quote_body(line).trim().is_empty()
}

/// Strip one level of blockquote marker so the body can be inspected.
fn quote_body(line: &str) -> &str {
    let t = line.trim_start();
    match t.strip_prefix('>') {
        Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
        None => t,
    }
}









/// True if the line ends in an explicit hard break — two trailing spaces or a
/// trailing backslash. Joining across one would silently delete a `<br>`, and a
/// token-stream check cannot see it.
pub fn has_hard_break(line: &str) -> bool {
    let no_nl = line.trim_end_matches(['\r', '\n']);
    no_nl.ends_with("  ") || no_nl.ends_with('\\')
}

/// Line-initial delimiters of common non-CommonMark block extensions.
///
/// A CommonMark parser cannot see these — to it, `:::note` is paragraph text —
/// so joining across one silently destroys a directive that MkDocs, Docusaurus
/// or Obsidian would have rendered. This list is the *named* residue left after
/// adopting a parser, and it is short and enumerable, which is the whole reason
/// the parser is worth having.
const EXTENSION_DELIMITERS: &[&str] = &[":::", "!!!", "???", "{%", "%}", "+++", "<%", "%>"];

fn is_extension_delimiter(line: &str) -> bool {
    // Look through blockquote markers, as every other predicate here does
    // (`is_mid_phrase_break`, and the list/label tests in `reflow`). Without
    // this, `> :::note` reads as ordinary prose and gets welded — and neither
    // check downstream can object, because a CommonMark parser sees a directive
    // as paragraph text too. This guard is the only thing standing there.
    // Strip repeatedly so nested quoting (`> > :::`) is covered.
    let mut t = line.trim_start();
    loop {
        if EXTENSION_DELIMITERS.iter().any(|d| t.starts_with(d)) {
            return true;
        }
        if !t.starts_with('>') {
            return false;
        }
        t = quote_body(t).trim_start();
    }
}

/// The parser options, in one place.
///
/// The classifier and the post-condition must agree about what the document
/// *is*; if these drift apart, one decides a construct is opaque while the other
/// does not, and the layering silently stops meaning anything. That is the one
/// coupling in this design that must not break, so it has exactly one source.
///
/// `ENABLE_MATH` and `ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS` are here because
/// `$$…$$` and `+++…+++` bodies are content where a line break carries meaning —
/// joining LaTeX lines can move a `%` comment and swallow what follows. Their
/// delimiters were already guarded by hand; telling the parser about them covers
/// the bodies too.
///
/// `ENABLE_MATH` is not free, though. `$$` is also PostgreSQL dollar-quoting and
/// bash's PID, so prose containing either parses as a math block and everything
/// to the next `$$` goes opaque — two files in this repo's own corpus hit that,
/// and two paragraphs there are no longer repaired. That is coverage lost, not
/// documents damaged, which is the direction to err in: a missed repair is a
/// wrapped paragraph, a bad one is a broken document.
fn md_options() -> pulldown_cmark::Options {
    use pulldown_cmark::Options;
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_MATH);
    opts.insert(Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS);
    opts
}

/// Lines the parser says are inside a construct where breaks are structural or
/// literal: code blocks, tables, metadata blocks, footnote definitions, raw HTML
/// (including multi-line comments), headings, and thematic breaks.
fn parser_opaque_map(src: &str, line_count: usize) -> Vec<bool> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    let starts: Vec<usize> = std::iter::once(0)
        .chain(src.char_indices().filter(|(_, c)| *c == '\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |off: usize| starts.partition_point(|&s| s <= off).saturating_sub(1);

    let mut opaque = vec![false; line_count];
    let mut depth = 0usize;

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);

    let mark = |range: std::ops::Range<usize>, opaque: &mut Vec<bool>| {
        // GFM synthesizes zero-length events (empty trailing table cells occur
        // in real files here). `lo..=hi` would be `n..=n-1`, which only happens
        // to be safe because RangeInclusive degenerates it to empty. Skip
        // explicitly rather than rely on that.
        if range.is_empty() {
            return;
        }
        let lo = line_of(range.start);
        let hi = line_of(range.end.saturating_sub(1)).min(line_count.saturating_sub(1));
        for flag in &mut opaque[lo..=hi] {
            *flag = true;
        }
    };

    for (ev, range) in Parser::new_ext(src, md_options()).into_offset_iter() {
        // Html and Rule are standalone events with no matching End — mark their
        // own range and leave `depth` alone, or everything after the first HTML
        // comment stays opaque for the rest of the document.
        if matches!(ev, Event::Html(_) | Event::Rule) {
            mark(range, &mut opaque);
            continue;
        }
        let opens = matches!(
            ev,
            Event::Start(Tag::CodeBlock(_))
                | Event::Start(Tag::Table(_))
                | Event::Start(Tag::MetadataBlock(_))
                | Event::Start(Tag::FootnoteDefinition(_))
                | Event::Start(Tag::Heading { .. })
        );
        let closes = matches!(
            ev,
            Event::End(TagEnd::CodeBlock)
                | Event::End(TagEnd::Table)
                | Event::End(TagEnd::MetadataBlock(_))
                | Event::End(TagEnd::FootnoteDefinition)
                | Event::End(TagEnd::Heading(_))
        );
        if opens {
            depth += 1;
        }
        if depth > 0 {
            mark(range, &mut opaque);
        }
        if closes {
            depth = depth.saturating_sub(1);
        }
    }
    opaque
}

/// Classify every line, using the parser for eligibility.
///
/// Replaces a hand-rolled matcher that had to know about fences, frontmatter,
/// headings, rules, tables, HTML blocks, HTML comments, indented code, tabs, and
/// reference definitions — and got several of them wrong. The parser knows all of
/// them; this function only adds what the parser does not model: blank lines,
/// empty blockquote lines, and non-CommonMark extension delimiters.
pub fn classify(lines: &[&str]) -> Vec<LineKind> {
    let src = lines.join("\n");
    let opaque = parser_opaque_map(&src, lines.len());

    lines
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            if opaque.get(i).copied().unwrap_or(false) || is_extension_delimiter(raw) {
                LineKind::Verbatim
            } else if raw.trim().is_empty() || is_empty_blockquote(raw) {
                // A bare `>` is a paragraph boundary inside the quote: joining
                // across one welds two quoted paragraphs, and the token check
                // cannot see it because the only difference is a `>`.
                LineKind::Blank
            } else {
                LineKind::Prose
            }
        })
        .collect()
}

/// True if `line` opens a new join block relative to `prev` — the boundaries
/// [`reflow`]'s flush loop refuses to join across: list items, bold label
/// lines, blockquote transitions, and the line after an explicit hard break.
///
/// [`units`] splits on the same predicate so that what [`detect`] measures is
/// exactly what a repair would join. Before they shared it, a run of list items
/// was one "paragraph" to the detector — a wrapped item's short continuation
/// sat mid-run where the tail exemption could not see it.
fn starts_join_block(line: &str, prev: &str) -> bool {
    let body = quote_body(line);
    is_list_item(line)
        || is_list_item(body)
        || body.trim_start().starts_with("**")
        || is_blockquote(line) != is_blockquote(prev)
        || has_hard_break(prev)
}

/// Maximal spans of consecutive [`LineKind::Prose`] lines that a repair would
/// join as one block — paragraphs, split further at [`starts_join_block`]
/// boundaries. Single-line spans are dropped: nothing to join, nothing wrapped.
fn units(kinds: &[LineKind], lines: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let close = |s: usize, e: usize, out: &mut Vec<(usize, usize)>| {
        if e > s {
            out.push((s, e));
        }
    };
    for (i, k) in kinds.iter().enumerate() {
        match (k, start) {
            (LineKind::Prose, None) => start = Some(i),
            (LineKind::Prose, Some(s)) => {
                if starts_join_block(lines[i], lines[i - 1]) {
                    close(s, i - 1, &mut out);
                    start = Some(i);
                }
            }
            (_, Some(s)) => {
                close(s, i - 1, &mut out);
                start = None;
            }
            (_, None) => {}
        }
    }
    if let Some(s) = start {
        close(s, kinds.len() - 1, &mut out);
    }
    out
}

/// True if the break between `a` and `b` reads as mechanical rather than
/// authored — `a` stops on a bare word and `b` resumes mid-phrase.
fn is_mid_phrase_break(a: &str, b: &str) -> bool {
    if has_hard_break(a) {
        return false;
    }
    // Blockquote prose wraps like any other prose, so judge the break on the
    // quoted bodies. Crossing *into* or *out of* a quote is structural, never a
    // wrap, so those cases stay disqualified.
    let (a, b) = match (is_blockquote(a), is_blockquote(b)) {
        (true, true) => (quote_body(a), quote_body(b)),
        (false, false) => (a, b),
        _ => return false,
    };
    let at = a.trim_end();
    match at.chars().last() {
        None => return false,
        Some(c) if BOUNDARY_END.contains(&c) => return false,
        Some(_) => {}
    }
    if is_list_item(b) {
        return false;
    }
    let bt = b.trim_start();
    if bt.starts_with('#') || bt.starts_with('|') || bt.starts_with("**") {
        return false;
    }
    match bt.chars().next() {
        Some(c) => c.is_lowercase() || matches!(c, '\u{201c}' | '"' | '(' | '`'),
        None => false,
    }
}

/// Find paragraphs containing a hard-wrap window.
///
/// One result per paragraph — the transform flattens the whole paragraph, so a
/// second window in the same paragraph adds nothing.
pub fn detect(lines: &[&str]) -> Vec<WrappedParagraph> {
    detect_with(lines, DetectConfig::default())
}

/// [`detect`] with explicit tuning.
pub fn detect_with(lines: &[&str], cfg: DetectConfig) -> Vec<WrappedParagraph> {
    let kinds = classify(lines);
    let mut found = Vec::new();

    for (pstart, pend) in units(&kinds, lines) {
        let n = pend - pstart + 1;
        if n < cfg.min_run {
            continue;
        }
        // Line lengths computed once per unit, not once per window.
        let lens: Vec<usize> = lines[pstart..=pend]
            .iter()
            .map(|l| l.trim_end().chars().count())
            .collect();

        // The unit's final line is exempt from the band when short: a wrap
        // tail is whatever was left of the sentence, so demanding it sit near
        // the fill column hid every wrap whose remainder was short (the old floor). A
        // window never consists of exempt lines alone — the exemption names
        // one line, and every window has at least two.
        let exempt = |i: usize| i == n - 1 && lens[i] < cfg.min_len;

        'windows: for offset in 0..=(n - cfg.min_run) {
            let mut lo = usize::MAX;
            let mut hi = 0usize;
            for len in cfg.min_run..=(n - offset) {
                // Extend the running min/max by the one newly included line.
                let upto = if len == cfg.min_run { 0 } else { len - 1 };
                for (i, l) in lens.iter().enumerate().take(offset + len).skip(offset + upto) {
                    if exempt(i) {
                        continue;
                    }
                    lo = lo.min(*l);
                    hi = hi.max(*l);
                }
                // A window of only the exempt tail (reachable when a config
                // sets min_run to 1) has no banded lines yet — extend it.
                if lo > hi {
                    continue;
                }
                // Each band predicate is monotone in window length: `lo` only
                // falls and `hi` only rises as the window grows, so a failure
                // here fails for every longer window at this offset. Breaking
                // rather than continuing is what keeps this off O(n^3) — a long
                // bullet list is one paragraph, and this runs on a hook that
                // fires on every write.
                if lo < cfg.min_len || hi > cfg.max_len || hi - lo > cfg.band {
                    break;
                }
                let win = &lines[pstart + offset..pstart + offset + len];
                let mids = win
                    .windows(2)
                    .filter(|p| is_mid_phrase_break(p[0], p[1]))
                    .count();
                if mids >= cfg.min_mid_breaks.min(len - 1) {
                    found.push(WrappedParagraph {
                        start: pstart,
                        end: pend,
                        trigger_start: pstart + offset,
                        trigger_len: len,
                        mid_breaks: mids,
                    });
                    break 'windows;
                }
            }
        }
    }
    found
}

/// True if any paragraph is hard-wrapped. The predicate the postcheck and the
/// lint rule both reduce to.
pub fn is_wrapped(lines: &[&str]) -> bool {
    !detect(lines).is_empty()
}

/// Flatten the paragraphs [`detect`] identifies, leaving everything else
/// byte-identical.
///
/// A flattened paragraph is split again at boundaries that carry meaning at the
/// start of a line — list items, blockquote bold labels (`**Status:**`), and
/// explicit hard breaks — because joining across one of those changes the
/// rendered structure while leaving the token stream untouched.
pub fn reflow(lines: &[&str]) -> Vec<String> {
    reflow_with_stats(lines).0
}

/// [`reflow`], plus the number of blockquote continuation markers the transform
/// dropped.
///
/// The count is what lets [`token_verdict_accounted`] treat a missing `>` as an
/// *accounted* loss rather than a blanket exemption. Without it, "any difference
/// consisting only of `>` tokens is fine" cannot distinguish dropping the
/// continuation markers of a joined quote from dropping the bare `>` that was a
/// paragraph boundary.
pub fn reflow_with_stats(lines: &[&str]) -> (Vec<String>, usize) {
    let hot = detect(lines);
    if hot.is_empty() {
        return (lines.iter().map(|s| s.to_string()).collect(), 0);
    }
    let mut quote_joins = 0usize;
    let spans: std::collections::HashMap<usize, usize> =
        hot.iter().map(|w| (w.start, w.end)).collect();

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let Some(&end) = spans.get(&i) else {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        let mut block: Vec<&str> = Vec::new();
        let flush = |block: &mut Vec<&str>, out: &mut Vec<String>, joins: &mut usize| {
            if block.is_empty() {
                return;
            }
            let indent = indent_of(block[0]).to_string();
            // A joined blockquote keeps one leading `>`; the continuation
            // markers would otherwise land mid-line as literal text. This is
            // the sole token the invariant is allowed to lose.
            let quoted = is_blockquote(block[0]);
            if quoted {
                *joins += block.iter().skip(1).filter(|l| is_blockquote(l)).count();
            }
            let joined = block
                .iter()
                .enumerate()
                .map(|(n, l)| if quoted && n > 0 { quote_body(l).trim() } else { l.trim() })
                .collect::<Vec<_>>()
                .join(" ");
            let mut line = indent + &joined;
            // `trim` above eats the two trailing spaces that *are* the hard
            // break, so put them back.
            if block.last().is_some_and(|l| l.trim_end_matches(['\r', '\n']).ends_with("  ")) {
                line.push_str("  ");
            }
            // Likewise the `\r` of a CRLF file. Rewriting only the repaired
            // paragraphs would otherwise leave mixed endings behind, and the
            // next normalization churns the whole file.
            if block.last().is_some_and(|l| l.ends_with('\r')) {
                line.push('\r');
            }
            out.push(line);
            block.clear();
        };

        for line in &lines[i..=end] {
            let line = *line;
            // The same boundary predicate `units` splits on, so detection scope
            // and join scope cannot drift apart. With an empty block there is
            // nothing to flush, so the prev-dependent clauses need no special
            // case beyond picking the line itself as its own predecessor.
            if block.last().is_some_and(|p| starts_join_block(line, p))
                || (block.is_empty() && starts_join_block(line, line))
            {
                flush(&mut block, &mut out, &mut quote_joins);
            }
            block.push(line);
            if has_hard_break(line) {
                flush(&mut block, &mut out, &mut quote_joins);
            }
        }
        flush(&mut block, &mut out, &mut quote_joins);
        i = end + 1;
    }
    (out, quote_joins)
}

/// Result of the token-stream invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenVerdict {
    /// Byte-for-byte identical token streams.
    Identical,
    /// Identical except for bare `>` markers dropped where wrapped blockquote
    /// lines were joined. `dropped` counts them.
    QuoteMarkersDropped { dropped: usize },
    /// Words were lost, gained, or reordered. Never acceptable.
    Mismatch { before: usize, after: usize },
}

/// Compare whitespace-delimited tokens before and after a reflow.
///
/// This is a cheap total check against *loss*, and it is the reason bulk repair
/// is tractable at all. It is **not** sufficient on its own: joining two lines
/// that should have stayed separate produces an identical token stream, and
/// leading whitespace is not a token, so this check passes a nested list that
/// has been de-indented. Pair it with the structural guarantees in [`reflow`]
/// and with reading the diff.
pub fn token_verdict(before: &str, after: &str) -> TokenVerdict {
    let b: Vec<&str> = before.split_whitespace().collect();
    let a: Vec<&str> = after.split_whitespace().collect();
    if b == a {
        return TokenVerdict::Identical;
    }
    let bq: Vec<&str> = b.iter().copied().filter(|t| *t != ">").collect();
    let aq: Vec<&str> = a.iter().copied().filter(|t| *t != ">").collect();
    if bq == aq {
        let before_markers = b.iter().filter(|t| **t == ">").count();
        let after_markers = a.iter().filter(|t| **t == ">").count();
        // Joining a wrapped blockquote can only ever *drop* markers. Gaining
        // one means the transform invented structure, which is a mismatch —
        // and subtracting the other way would underflow.
        if let Some(dropped) = before_markers.checked_sub(after_markers) {
            return TokenVerdict::QuoteMarkersDropped { dropped };
        }
        return TokenVerdict::Mismatch { before: b.len(), after: a.len() };
    }
    TokenVerdict::Mismatch { before: b.len(), after: a.len() }
}

/// [`token_verdict`], but the dropped `>` markers must match `expected` exactly.
///
/// This is the form callers that write files should use. `token_verdict` accepts
/// *any* difference consisting solely of `>` tokens, which is an exemption rather
/// than an invariant: on its own it cannot tell the continuation markers of a
/// joined quote from the bare `>` that separated two quoted paragraphs. Pairing
/// the check with the transform's own join count closes that gap.
pub fn token_verdict_accounted(before: &str, after: &str, expected: usize) -> TokenVerdict {
    match token_verdict(before, after) {
        TokenVerdict::QuoteMarkersDropped { dropped } if dropped != expected => {
            TokenVerdict::Mismatch {
                before: before.split_whitespace().count(),
                after: after.split_whitespace().count(),
            }
        }
        other => other,
    }
}

/// Outcome of a bounded repair: the result, and whether it settled.
#[derive(Debug, Clone)]
pub struct RepairOutcome {
    /// The repaired lines.
    pub lines: Vec<String>,
    /// Blockquote continuation markers dropped, summed across passes.
    pub quote_joins: usize,
    /// Passes actually run.
    pub passes: usize,
    /// Whether the detector reports the result clean.
    ///
    /// `false` means the transform could not reconcile its own output — the
    /// output would immediately re-trigger the detector, producing a file that
    /// nags forever. Callers should treat that as a finding and decline to write,
    /// not as something to paper over.
    pub converged: bool,
}

/// Default pass ceiling for [`repair`]. One pass suffices on every file measured;
/// the budget exists so a transform bug bails instead of spinning.
pub const MAX_REPAIR_PASSES: usize = 3;

/// Repair until the detector reports clean, or the pass budget runs out.
///
/// A single pass flattens each detected paragraph to one line, which is then too
/// long to sit in the detection band — so convergence normally happens on the
/// first pass and the second is just the confirming check. Running the detector
/// against our own output is what turns "we think we fixed it" into evidence, and
/// it is the same discipline the caller applies to structure: verify, then write.
pub fn repair(lines: &[&str], max_passes: usize) -> RepairOutcome {
    let mut current: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut quote_joins = 0usize;
    let mut passes = 0usize;

    for _ in 0..max_passes.max(1) {
        let refs: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
        if detect(&refs).is_empty() {
            return RepairOutcome { lines: current, quote_joins, passes, converged: true };
        }
        let (next, joins) = reflow_with_stats(&refs);
        passes += 1;
        quote_joins += joins;
        if next == current {
            // A fixed point the detector still flags: another pass cannot help.
            break;
        }
        current = next;
    }

    let refs: Vec<&str> = current.iter().map(|s| s.as_str()).collect();
    let converged = detect(&refs).is_empty();
    RepairOutcome { lines: current, quote_joins, passes, converged }
}

/// Structural fingerprint of a document: the parser's event stream, with inline
/// prose whitespace collapsed and code-block content kept byte-exact.
///
/// Joining wrapped lines inside a paragraph is the *only* change a reflow should
/// make, and it is invisible at this level — a soft break and a space are the
/// same thing inside a paragraph. So any difference in this fingerprint means the
/// transform altered the document's structure.
fn structure_fingerprint(src: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let mut out: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut in_code = false;

    for ev in Parser::new_ext(src, md_options()) {
        if matches!(ev, Event::Start(Tag::CodeBlock(_))) {
            in_code = true;
        }
        if matches!(ev, Event::End(TagEnd::CodeBlock)) {
            in_code = false;
        }
        // Inside a code block whitespace *is* content, so a joined fence body
        // must not be normalized away.
        if in_code {
            if let Event::Text(t) = &ev {
                out.push(format!("CODE:{t:?}"));
                continue;
            }
        }
        match ev {
            // Inline code is its own token, not prose. Folding it into the
            // whitespace-collapsed text buffer would blind the post-condition to
            // a span appearing, vanishing, or having its interior collapsed —
            // and this layer's value is precisely that it does not depend on
            // enumerating constructs correctly.
            Event::Code(t) => {
                if !text.trim().is_empty() {
                    out.push(format!(
                        "TEXT:{}",
                        text.split_whitespace().collect::<Vec<_>>().join(" ")
                    ));
                    text.clear();
                }
                out.push(format!("SPAN:{t:?}"));
            }
            Event::Text(t) => {
                text.push(' ');
                text.push_str(&t);
            }
            Event::SoftBreak => text.push(' '),
            other => {
                if !text.trim().is_empty() {
                    out.push(format!(
                        "TEXT:{}",
                        text.split_whitespace().collect::<Vec<_>>().join(" ")
                    ));
                    text.clear();
                }
                out.push(format!("{other:?}"));
            }
        }
    }
    if !text.trim().is_empty() {
        out.push(format!(
            "TEXT:{}",
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
    }
    out
}

/// Whether a reflow left the document structurally identical.
///
/// This is the post-condition worth trusting, because unlike the classifier it
/// does not depend on having enumerated markdown's constructs correctly — it asks
/// the parser what changed. It caught a live corruption the token check passed: a
/// multi-line HTML comment in the shipped corpus whose body was being joined.
///
/// Two things it cannot see, both principled:
///
/// - **Welding authored line breaks.** Two sentences on separate lines and the
///   same two joined are the *same* CommonMark document. No verifier can catch
///   that; declining to do it is [`detect`]'s job.
/// - **Non-CommonMark extensions.** A `:::note` directive is paragraph text to
///   any CommonMark parser. That residue is covered by `EXTENSION_DELIMITERS`.
pub fn structure_preserved(before: &str, after: &str) -> bool {
    structure_diff(before, after).is_none()
}

/// Where a reflow changed the document's structure, if it did.
///
/// Returns the first diverging event as `(index, before, after)`. A tripped
/// post-condition is the most informative output this module produces — it names
/// a construct the classifier misreads — so callers should *report* it rather
/// than discarding it. Refusing to write and saying nothing loses the finding.
pub fn structure_diff(before: &str, after: &str) -> Option<(usize, String, String)> {
    let (a, b) = (structure_fingerprint(before), structure_fingerprint(after));
    if a == b {
        return None;
    }
    let idx = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let pick = |v: &Vec<String>, i: usize| {
        v.get(i).cloned().unwrap_or_else(|| "<end of document>".to_string())
    };
    Some((idx, pick(&a, idx), pick(&b, idx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<&str> {
        s.split('\n').collect()
    }

    fn round_trip(src: &str) -> String {
        reflow(&split(src)).join("\n")
    }

    const WRAPPED: &str = "\
In 1.0, `~/.claude` is a thin projection of an XDG application, not the app
itself (ADR-142). The source lives in `$XDG_DATA_HOME/agent-ways`; the home dir
gets symlinks to the projected roots plus a three-way merge into settings, and
everything else the app ships stays where it is.";

    #[test]
    fn detects_a_hard_wrapped_paragraph() {
        let lines = split(WRAPPED);
        let hits = detect(&lines);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].start, hits[0].end), (0, 3));
    }

    #[test]
    fn flattens_the_whole_enclosing_paragraph() {
        let out = reflow(&split(WRAPPED));
        assert_eq!(out.len(), 1, "expected one line, got {out:#?}");
        assert!(out[0].starts_with("In 1.0,"));
        assert!(out[0].ends_with("stays where it is."));
    }

    #[test]
    fn token_stream_survives_a_reflow() {
        assert_eq!(token_verdict(WRAPPED, &round_trip(WRAPPED)), TokenVerdict::Identical);
    }

    #[test]
    fn detects_a_two_line_wrap_with_a_short_tail() {
        // The most common wrap shape: one break at the fill column, the
        // remainder short. The old floor (3-line windows, tail inside the
        // band) could not see it — this was the live gap that let wrapped
        // markdown ship while the postcheck stayed silent.
        let src = "\
The most common wrap shape is a paragraph broken exactly once at the
fill column.";
        let hits = detect(&split(src));
        assert_eq!(hits.len(), 1);
        assert_eq!(round_trip(src).lines().count(), 1);
    }

    #[test]
    fn detects_a_three_line_wrap_with_a_short_tail() {
        // Two breaks, then the sentence ends short. The tail sat inside every
        // candidate window, and its length failed the band for all of them.
        let src = "\
A slightly longer paragraph that the model wrapped twice while it
was writing, so there are two interior breaks before the sentence
finally ends here.";
        let hits = detect(&split(src));
        assert_eq!(hits.len(), 1);
        assert_eq!(round_trip(src).lines().count(), 1);
    }

    #[test]
    fn detects_a_wrapped_list_item_between_flat_siblings() {
        // A wrapped bullet mid-list. Before units, the whole list was one
        // "paragraph": breaks at item boundaries are never mid-phrase, so the
        // count could not reach its floor, and the wrapped item's short
        // continuation failed the band besides.
        let src = "\
- first item stays short
- A list item whose text the model wrapped onto a continuation line
  past the fill column
- another short item";
        let out = round_trip(src);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "expected the wrapped item joined: {out:?}");
        assert_eq!(lines[0], "- first item stays short");
        assert!(lines[1].ends_with("past the fill column"));
        assert_eq!(lines[2], "- another short item");
    }

    #[test]
    fn leaves_an_authored_two_line_break_alone() {
        // A two-line paragraph whose break lands at a clause boundary — the
        // comma says the author paused there. The single-break rule must not
        // sweep this in.
        let src = "\
When the sentence pauses at a natural boundary like a comma or colon,
the continuation is judged authored and the paragraph stays untouched.";
        assert!(detect(&split(src)).is_empty());
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn a_capitalized_second_line_is_not_a_wrap() {
        let src = "\
A first line that is long enough to sit inside the detection band here
But the next line starts a new sentence with a capital letter after it.";
        assert!(detect(&split(src)).is_empty());
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn leaves_parallel_one_sentence_lines_alone() {
        // Deliberate semantic breaks: each line is a complete clause. The
        // real corpus case was meta/think/strategies/self-consistency.md.
        let src = "\
If majority agrees: that's the answer, with the divergent path noted as caveat.
If split: present the split to the user. The uncertainty beats a forced answer.";
        assert!(detect(&split(src)).is_empty());
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn leaves_bold_definition_lines_alone() {
        // meta/knowledge/knowledge.md — welding these was a real regression.
        let src = "\
**Skills** = semantically-discovered (Claude decides based on intent here)
**Ways** = triggered (patterns, commands, file edits, or state conditions)";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn never_deindents_a_nested_list() {
        // ea/email/drafting/drafting.md. The token oracle passes this
        // corruption, which is why indentation is a structural guarantee.
        let src = "\
1. Ask the operator for the missing pieces before drafting anything at all:
   - What is the intent? (accepting, declining, requesting, informing)
   - Any specific points to include, and anything to leave unsaid?
   - Tone preference, if it differs from their usual register for this?";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn preserves_tables_fences_and_headings() {
        let src = "\
## Heading

| Content | Diagram |
|---------|---------|
| Temporal sequences that run long enough to wrap in a narrow band | sequence |

```text
this fenced line is long enough to look wrapped but must not be touched here
also this one, which continues the thought and would otherwise be joined up
and a third, so the fence interior would qualify as a run if it were prose
```

Trailing paragraph.";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn keeps_a_blockquote_status_block_on_separate_lines() {
        let src = "\
> **Status:** the projection is live and the binaries are linked onto PATH now
> **Next:** reconcile is idempotent, so re-running it changes nothing at all
> **Owner:** whoever holds the migration window for this particular release";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn joins_a_wrapped_blockquote_and_reports_dropped_markers() {
        let src = "\
> Messaging guidance lives in three synchronized sources. When you edit
> the peer-messaging section, the autonomy paragraph, or the silence-is-valid
> callout in this file, update the other two in the very same commit.";
        let out = round_trip(src);
        assert_eq!(out.lines().count(), 1);
        match token_verdict(src, &out) {
            TokenVerdict::QuoteMarkersDropped { dropped } => assert_eq!(dropped, 2),
            other => panic!("expected dropped quote markers, got {other:?}"),
        }
    }

    #[test]
    fn preserves_an_explicit_hard_break() {
        // The two trailing spaces are the whole point, so build the input
        // explicitly rather than trusting them to survive a source literal.
        let src = [
            "This line ends with a deliberate hard break, long enough to wrap,  ",
            "and this line continues below it, also long enough to sit in band,",
            "and a third line to give the window the length detection requires.",
        ]
        .join("\n");
        let out = round_trip(&src);
        assert!(
            out.lines().next().unwrap().ends_with("  "),
            "hard break was eaten: {out:?}"
        );
    }

    #[test]
    fn does_not_weld_a_paragraph_onto_a_setext_underline() {
        let src = "\
Some paragraph text that is long enough to look like it might be wrapped here
and a continuation line that sits comfortably inside the detection band too
and a third line so that the window has enough members to actually trigger.
---";
        let out = round_trip(src);
        assert_eq!(out.lines().last().unwrap().trim(), "---", "hr consumed: {out:?}");
    }

    #[test]
    fn ignores_short_and_wide_prose() {
        let short = "one\ntwo\nthree\nfour";
        assert!(detect(&split(short)).is_empty());
        let wide = format!("{}\n{}\n{}", "x".repeat(200), "y".repeat(201), "z".repeat(199));
        assert!(detect(&split(&wide)).is_empty());
    }

    #[test]
    fn frontmatter_is_opaque() {
        let src = "\
---
description: a description long enough to look wrapped when read as prose here
vocabulary: markdown wrap unwrap reflow line length column paragraph prose fill
files: \\.md$
---

Body.";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn reflow_is_idempotent() {
        let once = round_trip(WRAPPED);
        assert_eq!(round_trip(&once), once);
    }

    #[test]
    fn a_bare_quote_marker_separates_two_quoted_paragraphs() {
        // Review finding 1. Joining across a bare `>` welds two quoted
        // paragraphs, and the token oracle calls it clean because the only
        // difference is a `>`. Detection must see the boundary.
        let src = "\
> The first quoted paragraph runs long enough to sit inside the detect band
> and it continues here mid-phrase so the mid-break count reaches its floor
> and a third line keeps the window at the minimum run length it requires.
>
> A second quoted paragraph, which must remain a separate paragraph always.";
        let out = round_trip(src);
        assert!(
            out.contains("\n>\n"),
            "the bare `>` boundary was swallowed: {out:?}"
        );
        assert_eq!(out.lines().count(), 3, "{out:?}");
        assert!(out.lines().last().unwrap().starts_with("> A second"));
    }

    #[test]
    fn accounted_verdict_catches_an_unaccounted_marker_drop() {
        // The exemption made an invariant: a `>`-only difference is only clean
        // when the count matches the joins the transform actually performed.
        let before = "> alpha beta\n>\n> gamma delta";
        let welded = "> alpha beta gamma delta";
        assert!(matches!(
            token_verdict(before, welded),
            TokenVerdict::QuoteMarkersDropped { .. }
        ));
        assert!(matches!(
            token_verdict_accounted(before, welded, 1),
            TokenVerdict::Mismatch { .. }
        ));
    }

    #[test]
    fn raw_text_html_bodies_are_opaque() {
        // Review finding 3. `is_html` matched only the opening line, so a <pre>
        // body was joined — and preformatted text is precisely where the line
        // break *is* the rendering.
        let src = "\
<pre>
  some preformatted text line one that is long enough to be within band ok
  more preformatted text line two continuing mid phrase to trigger detect
  third preformatted line so the run length reaches the configured minimum
</pre>";
        assert!(detect(&split(src)).is_empty());
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn tab_indented_code_is_opaque() {
        // A tab advances to the next four-column stop, so counting only spaces
        // let a tab-indented code block through as prose.
        let src = "\
\tsome preformatted text line one that is long enough to be within the band
\tmore preformatted text line two continuing mid phrase to trigger detection
\tthird preformatted line so that the run length reaches the configured min";
        assert!(detect(&split(src)).is_empty());
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn a_prose_sentence_ending_in_a_colon_does_not_split_its_paragraph() {
        // Review finding 4. The hand-rolled label predicate marked any
        // colon-terminated line as structure, splitting the paragraph and leaving
        // the repair half-flat. The parser has no such notion — it is all one
        // paragraph — so the predicate is gone and this asserts the behavior.
        let src = "\
Messaging guidance lives in three synchronized sources, so when you edit one
of them you should update the other two so agents receive consistent output:
and this trailing clause continues the very same paragraph without a break
and one more line so the window still clears the minimum run length here.";
        let out = round_trip(src);
        assert_eq!(out.lines().count(), 1, "paragraph repaired half-flat: {out:?}");
    }

    #[test]
    fn extension_delimiters_are_seen_through_blockquote_markers() {
        // Review defect 1: the guard did not look through `>`, so `> :::note`
        // read as prose and welded. Neither downstream check can object — a
        // CommonMark parser sees a directive as paragraph text too — so this
        // guard is the only protection this class has.
        let src = "\
> :::note
> some directive body text long enough to sit inside the detect band ok
> and continuing here mid-phrase so the mid-break count reaches its floor
> and a third body line so the window reaches the minimum run length now
> :::";
        let out = round_trip(src);
        assert_eq!(out.lines().next().unwrap(), "> :::note", "welded: {out:?}");
        assert_eq!(out.lines().last().unwrap(), "> :::", "welded: {out:?}");
    }

    #[test]
    fn crlf_line_endings_survive_a_repair() {
        // Review defect 2: `trim()` stripped the `\r` off the last joined line,
        // leaving a CRLF file with mixed endings — invisible to both the token
        // and structural checks, and whole-file churn at the next normalization.
        let src = "In 1.0 the projection is thin, not the app itself, and long enough to wrap\r\n\
across lines here so that the detector actually fires on this paragraph ok\r\n\
and a third line so the run reaches the minimum length that it requires.\r\n";
        let out = round_trip(src);
        for line in out.split('\n').filter(|l| !l.is_empty()) {
            assert!(line.ends_with('\r'), "lost CRLF on: {line:?}");
        }
    }

    #[test]
    fn extension_delimiters_are_not_joined() {
        // `:::` is a MkDocs/Docusaurus directive — invisible to CommonMark, so no
        // parser will protect it. This is the named residue the guard list covers.
        let src = "\
:::note
some directive body text that is long enough to sit inside the detect band
and continuing here mid-phrase so that the mid-break count reaches a floor
and a third body line so the window reaches the minimum run length needed
:::";
        let out = round_trip(src);
        assert!(out.lines().next().unwrap() == ":::note", "directive welded: {out:?}");
        assert!(out.lines().last().unwrap() == ":::", "closing delimiter welded: {out:?}");
    }

    #[test]
    fn a_multi_line_html_comment_body_is_never_joined() {
        // Found by the structural post-condition on the real corpus
        // (attend.md), after the token check passed it. The hand-rolled
        // classifier handled <pre>/<script>/<style>/<textarea> but not comments.
        let src = "\
<!--
  Messaging guidance lives in three synchronized sources. When you edit
  the peer-messaging section, the autonomy paragraph, the silence-is-valid
  callout, or the CLI-is-the-contract note in this file, update the
  other two so agents receive consistent guidance at every point.
-->";
        assert!(detect(&split(src)).is_empty(), "html comment treated as prose");
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn a_real_link_reference_definition_survives() {
        // Review finding 5, corrected by the parser. CommonMark says a link
        // reference definition *cannot interrupt a paragraph*, so a `[x]: url`
        // line directly under prose is already paragraph text and joining it
        // changes nothing. A definition after a blank line is a real definition,
        // is its own block, and is never inside a detected paragraph. The
        // hand-rolled predicate was protecting against the wrong case.
        let src = "\
Some wrapped prose here that is long enough to fall inside the detect band
and continuing mid-phrase so that the mid-break count reaches its floor now
and a third line so the window reaches the minimum run length it requires.

[adr-142]: https://github.com/aaronsb/agent-ways/blob/main/docs/adr142.md";
        let out = round_trip(src);
        assert_eq!(
            out.lines().last().unwrap(),
            "[adr-142]: https://github.com/aaronsb/agent-ways/blob/main/docs/adr142.md"
        );
        assert!(structure_preserved(src, &out), "structure changed");
    }

    #[test]
    fn long_paragraph_detection_stays_fast() {
        // Review finding 2: the band never closes on this input, so before the
        // monotone break this was O(n^3) — 1600 lines took ~6s on a hook that
        // runs on every write.
        let body: Vec<String> = (0..1600)
            .map(|i| format!("- item {i} with enough text to vary the length {}", "x".repeat(i % 90)))
            .collect();
        let refs: Vec<&str> = body.iter().map(|s| s.as_str()).collect();
        let start = std::time::Instant::now();
        let _ = detect(&refs);
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 500,
            "detect took {elapsed:?} on 1600 lines — the monotone break regressed"
        );
    }

    #[test]
    fn gaining_a_quote_marker_is_a_mismatch() {
        // Guards the checked_sub: an invented marker must not underflow.
        assert!(matches!(
            token_verdict("> alpha beta", "> > alpha beta"),
            TokenVerdict::Mismatch { .. }
        ));
    }

    #[test]
    fn mismatch_is_reported_when_words_are_lost() {
        assert!(matches!(
            token_verdict("alpha beta gamma", "alpha gamma"),
            TokenVerdict::Mismatch { .. }
        ));
    }
}

#[cfg(test)]
mod postcondition_tests {
    use super::*;

    fn split(s: &str) -> Vec<&str> {
        s.split('\n').collect()
    }

    const WRAPPED: &str = "\
In 1.0, `~/.claude` is a thin projection of an XDG application, not the app
itself (ADR-142). The source lives in `$XDG_DATA_HOME/agent-ways`; the home dir
gets symlinks to the projected roots plus a three-way merge into settings, and
everything else the app ships stays where it is.";

    #[test]
    fn repair_converges_in_one_pass_and_reports_it() {
        let out = repair(&split(WRAPPED), MAX_REPAIR_PASSES);
        assert!(out.converged);
        assert_eq!(out.passes, 1, "expected a single pass, got {}", out.passes);
        assert_eq!(out.lines.len(), 1);
    }

    #[test]
    fn repair_of_clean_input_runs_no_passes() {
        let src = "A single flat paragraph with nothing at all to repair here.";
        let out = repair(&split(src), MAX_REPAIR_PASSES);
        assert!(out.converged);
        assert_eq!(out.passes, 0);
    }

    #[test]
    fn structure_diff_names_the_diverging_event() {
        // A tripped post-condition must be reportable, not just true/false —
        // the divergence is the finding, and discarding it loses the bug.
        let before = "Some paragraph text here\n---";
        let welded = "Some paragraph text here ---";
        let d = structure_diff(before, welded);
        assert!(d.is_some(), "structural change not detected");
        let (_idx, b, a) = d.unwrap();
        assert_ne!(b, a);
        assert!(structure_diff(before, before).is_none());
    }

    #[test]
    fn post_condition_rejects_a_structural_change() {
        // A join that welds a paragraph onto a thematic break changes structure.
        let before = "Some paragraph text here\n---";
        let welded = "Some paragraph text here ---";
        assert!(!structure_preserved(before, welded));
    }

    #[test]
    fn post_condition_accepts_a_pure_line_join() {
        let before = "In 1.0 the projection is thin, not the app\nitself, and source lives elsewhere.";
        let joined = "In 1.0 the projection is thin, not the app itself, and source lives elsewhere.";
        assert!(structure_preserved(before, joined));
    }

    #[test]
    fn post_condition_sees_an_inline_code_span_change() {
        // Advisory 1: folding Event::Code into the prose buffer made the check
        // blind to a span appearing or vanishing. Latent rather than live, but
        // this layer's value is not depending on enumeration being right.
        assert!(!structure_preserved("a `b` c", "a b c"));
        assert!(structure_preserved("a `b` c", "a  `b`   c"));
    }

    #[test]
    fn post_condition_sees_a_joined_code_fence_body() {
        let before = "```\nline one here\nline two here\n```";
        let welded = "```\nline one here line two here\n```";
        assert!(!structure_preserved(before, welded));
    }

    #[test]
    fn post_condition_sees_a_deindented_nested_list() {
        let before = "1. Ask first:\n   - What is the intent?\n   - Any points to include?";
        let flat = "1. Ask first:\n- What is the intent?\n- Any points to include?";
        assert!(!structure_preserved(before, flat));
    }
}
