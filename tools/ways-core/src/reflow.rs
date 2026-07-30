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
//! So detection is per-window, repair is per-paragraph, and a paragraph with no
//! detected window is never touched.

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
    /// Minimum mid-phrase breaks in a window.
    pub min_mid_breaks: usize,
}

impl Default for DetectConfig {
    fn default() -> Self {
        Self { min_len: 55, max_len: 100, band: 12, min_run: 3, min_mid_breaks: 2 }
    }
}

/// Characters that end a line at a natural boundary. A line ending on one of
/// these was plausibly broken on purpose, so it is not evidence of wrapping.
const BOUNDARY_END: &[char] = &['.', '!', '?', ':', ';', '"', ')', '`', '*', '|', '>', ','];

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Leading indent in *columns*, counting a tab as four. CommonMark treats a tab
/// as advancing to the next four-column stop, so counting only spaces would let
/// a tab-indented code block through as prose.
fn leading_columns(line: &str) -> usize {
    let mut cols = 0;
    for c in line.chars() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4 - (cols % 4),
            _ => break,
        }
    }
    cols
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

fn is_heading(line: &str) -> bool {
    if leading_columns(line) > 3 {
        return false;
    }
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && t[hashes..].starts_with(' ')
}

/// A thematic break or a setext underline: `---`, `***`, `___`, `===`.
fn is_rule_or_setext(line: &str) -> bool {
    if leading_columns(line) > 3 {
        return false;
    }
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let first = t.chars().next().unwrap();
    if !matches!(first, '-' | '*' | '_' | '=') {
        return false;
    }
    let count = t.chars().filter(|c| *c == first).count();
    let only = t.chars().all(|c| c == first || c == ' ');
    // `***`/`___` need three; a lone `-` or `=` is already a setext underline.
    (only && count >= 3) || (only && matches!(first, '-' | '=') && count >= 1)
}

fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn is_html(line: &str) -> bool {
    line.trim_start().starts_with('<')
}

/// A link reference or footnote definition — `[label]: target`, `[^note]: text`.
/// These are line-scoped syntax; joined into a paragraph they become literal
/// text, and the token stream is identical either way.
fn is_reference_def(line: &str) -> bool {
    let t = line.trim_start();
    if !t.starts_with('[') {
        return false;
    }
    match t.find("]:") {
        Some(close) => !t[1..close].contains('['),
        None => false,
    }
}

/// HTML elements CommonMark treats as raw text — their bodies are literal, so
/// every line until the closing tag is opaque, not just the opening one.
const RAW_TEXT_TAGS: &[&str] = &["pre", "script", "style", "textarea"];

/// The raw-text tag this line opens, if any and if it does not also close it.
fn opens_raw_html(line: &str) -> Option<&'static str> {
    let lower = line.trim_start().to_ascii_lowercase();
    RAW_TEXT_TAGS.iter().copied().find(|tag| {
        let open = format!("<{tag}");
        lower.starts_with(&open) && !lower.contains(&format!("</{tag}>"))
    })
}

/// Four-space indented block — a code block, not wrapped prose. List
/// continuations are indented too, so this only claims lines indented four or
/// more spaces that are *not* list items.
fn is_indented_block(line: &str) -> bool {
    leading_columns(line) >= 4 && !line.trim().is_empty() && !is_list_item(line)
}

/// A bare label line like `Status:` or `Next steps:` — structure, not prose.
///
/// Bounded by word count: without the cap, any prose sentence ending in a colon
/// and containing no interior punctuation qualifies, which splits its paragraph
/// and leaves the repair half-flat — the exact failure the enclosing-paragraph
/// scope exists to avoid.
const MAX_LABEL_WORDS: usize = 5;

fn is_label_line(line: &str) -> bool {
    let t = line.trim();
    match t.strip_suffix(':') {
        Some(head) if !head.is_empty() => {
            head.split_whitespace().count() <= MAX_LABEL_WORDS
                && head
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '/'))
        }
        _ => false,
    }
}

/// True if the line ends in an explicit hard break — two trailing spaces or a
/// trailing backslash. Joining across one would silently delete a `<br>`, and a
/// token-stream check cannot see it.
pub fn has_hard_break(line: &str) -> bool {
    let no_nl = line.trim_end_matches(['\r', '\n']);
    no_nl.ends_with("  ") || no_nl.ends_with('\\')
}

/// Classify every line. Fenced code and leading YAML frontmatter are opaque.
pub fn classify(lines: &[&str]) -> Vec<LineKind> {
    let mut out = Vec::with_capacity(lines.len());
    let mut fence: Option<&str> = None;
    let mut in_frontmatter = false;
    let mut raw_html: Option<&str> = None;

    for (i, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim();

        if i == 0 && trimmed == "---" {
            in_frontmatter = true;
            out.push(LineKind::Verbatim);
            continue;
        }
        if in_frontmatter {
            out.push(LineKind::Verbatim);
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }

        let opener = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        match (fence, opener) {
            (None, Some(tok)) => {
                fence = Some(tok);
                out.push(LineKind::Verbatim);
                continue;
            }
            (Some(tok), Some(seen)) if seen == tok => {
                fence = None;
                out.push(LineKind::Verbatim);
                continue;
            }
            (Some(_), _) => {
                out.push(LineKind::Verbatim);
                continue;
            }
            (None, None) => {}
        }

        // Raw-text HTML: the body is literal, so every line through the closing
        // tag is opaque — not just the opening one.
        if let Some(tag) = raw_html {
            out.push(LineKind::Verbatim);
            if trimmed.to_ascii_lowercase().contains(&format!("</{tag}>")) {
                raw_html = None;
            }
            continue;
        }
        if let Some(tag) = opens_raw_html(raw) {
            raw_html = Some(tag);
            out.push(LineKind::Verbatim);
            continue;
        }

        if trimmed.is_empty() || is_empty_blockquote(raw) {
            // A bare `>` is a paragraph boundary inside the quote. Treating it as
            // Blank both stops the weld and lets the paragraph after it be
            // detected on its own.
            out.push(LineKind::Blank);
        } else if is_heading(raw)
            || is_rule_or_setext(raw)
            || is_table_row(raw)
            || is_html(raw)
            || is_reference_def(raw)
            || is_indented_block(raw)
            || is_label_line(raw)
        {
            out.push(LineKind::Verbatim);
        } else {
            out.push(LineKind::Prose);
        }
    }
    out
}

/// Maximal spans of consecutive [`LineKind::Prose`] lines — the paragraphs.
fn paragraphs(kinds: &[LineKind]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, k) in kinds.iter().enumerate() {
        match (k, start) {
            (LineKind::Prose, None) => start = Some(i),
            (LineKind::Prose, Some(_)) => {}
            (_, Some(s)) => {
                if i - s > 1 {
                    out.push((s, i - 1));
                }
                start = None;
            }
            (_, None) => {}
        }
    }
    if let Some(s) = start {
        if kinds.len() - s > 1 {
            out.push((s, kinds.len() - 1));
        }
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

    for (pstart, pend) in paragraphs(&kinds) {
        let n = pend - pstart + 1;
        if n < cfg.min_run {
            continue;
        }
        // Line lengths computed once per paragraph, not once per window.
        let lens: Vec<usize> = lines[pstart..=pend]
            .iter()
            .map(|l| l.trim_end().chars().count())
            .collect();

        'windows: for offset in 0..=(n - cfg.min_run) {
            let mut lo = usize::MAX;
            let mut hi = 0usize;
            for len in cfg.min_run..=(n - offset) {
                // Extend the running min/max by the one newly included line.
                let upto = if len == cfg.min_run { 0 } else { len - 1 };
                for l in &lens[offset + upto..offset + len] {
                    lo = lo.min(*l);
                    hi = hi.max(*l);
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
                if mids >= cfg.min_mid_breaks {
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
            out.push(line);
            block.clear();
        };

        for line in &lines[i..=end] {
            let line = *line;
            let body = quote_body(line);
            let starts_block = is_list_item(line)
                || is_list_item(body)
                || body.trim_start().starts_with("**")
                || (is_blockquote(line) && block.last().is_some_and(|p| !is_blockquote(p)))
                || (!is_blockquote(line) && block.last().is_some_and(|p| is_blockquote(p)));
            if starts_block {
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
    fn a_prose_sentence_ending_in_a_colon_is_not_a_label() {
        // Review finding 4. The unbounded predicate split the paragraph and left
        // the repair half-flat — the failure enclosing-paragraph scope avoids.
        let long = "so we ended up considering the following three alternatives:";
        assert!(!is_label_line(long));
        assert!(is_label_line("Status:"));
        assert!(is_label_line("Next steps:"));
    }

    #[test]
    fn link_reference_definitions_survive() {
        // Review finding 5. `[label]: target` is line-scoped syntax; joined into
        // a paragraph it becomes literal text, token stream unchanged.
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
