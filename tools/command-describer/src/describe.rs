//! Segment descriptions from tldr summaries (ADR-706 item 7) and the
//! ordered described block the Bash lane's judge reads (ADR-188 item 3c).
//!
//! The summaries are data the caller loads once: the `summaries.tsv` that
//! `scripts/tldr-extract.py` writes under `catalog/commands/tldr/` in the
//! app directory, one `page<TAB>first description line` per tldr page
//! (`git-push`, `gh-pr-create`, `grep`), `#` lines a header. The crate never
//! fetches, runs a tldr client, `man` or `whatis`, and does not compile the
//! data in: ADR-706 keeps it a file a refresh can replace.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::{Connector, Role, Segment};

/// tldr page summaries keyed by page name, each cut to its first sentence
/// with the trailing period dropped. Build it once and reuse it: a lookup is
/// a hash probe per candidate page.
#[derive(Debug, Clone, Default)]
pub struct Summaries {
    pages: HashMap<Box<str>, Box<str>>,
}

impl Summaries {
    /// Read a `summaries.tsv`. Blank lines, `#` lines and lines without a
    /// tab are skipped.
    pub fn load(path: &Path) -> io::Result<Summaries> {
        Ok(Summaries::parse(&std::fs::read_to_string(path)?))
    }

    /// Parse the text of a `summaries.tsv`.
    pub fn parse(text: &str) -> Summaries {
        let pages = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .filter_map(|(page, line)| {
                let summary = first_sentence(line);
                (!page.is_empty() && !summary.is_empty()).then(|| (page.into(), summary.into()))
            })
            .collect();
        Summaries { pages }
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// The summary of the most specific page for a program and its
    /// subcommand words: `gh pr create` tries `gh-pr-create`, then `gh-pr`,
    /// then `gh`. At most the first two subcommand words are used.
    pub fn lookup(&self, program: &str, subcommand: Option<&str>) -> Option<&str> {
        let words: Vec<&str> = subcommand
            .map(|s| s.split_whitespace().take(2).collect())
            .unwrap_or_default();
        (0..=words.len()).rev().find_map(|k| {
            let page = std::iter::once(program)
                .chain(words[..k].iter().copied())
                .collect::<Vec<_>>()
                .join("-")
                .to_lowercase();
            self.pages.get(page.as_str()).map(|s| &**s)
        })
    }
}

/// The first sentence of a description line, whitespace collapsed, without
/// its closing period: `The Silver Searcher. Like ack…` gives `The Silver
/// Searcher`.
fn first_sentence(line: &str) -> String {
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = line.split(". ").next().unwrap_or("");
    first.trim_end_matches('.').trim().to_string()
}

/// One segment with its label and summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described<'a> {
    pub segment: &'a Segment,
    /// `program subcommand`, or `?` for a segment with neither.
    pub label: String,
    /// An operative segment's most specific page summary, or a filter's
    /// program summary. Always `None` for navigation, and for a program the
    /// data does not hold.
    pub summary: Option<&'a str>,
}

impl Described<'_> {
    /// `label (summary)`, or the label alone with no summary.
    pub fn text(&self) -> String {
        match self.summary {
            Some(s) => format!("{} ({s})", self.label),
            None => self.label.clone(),
        }
    }
}

/// Describe each segment, in order, one [`Described`] per segment.
pub fn describe<'a>(segments: &'a [Segment], data: &'a Summaries) -> Vec<Described<'a>> {
    segments
        .iter()
        .map(|seg| {
            let label = [seg.program.as_deref(), seg.subcommand.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            let summary = match (seg.role, seg.program.as_deref()) {
                (Role::Operative, Some(p)) => data.lookup(p, seg.subcommand.as_deref()),
                (Role::Filter, Some(p)) => data.lookup(p, None),
                _ => None,
            };
            Described {
                segment: seg,
                label: if label.is_empty() { "?".into() } else { label },
                summary,
            }
        })
        .collect()
}

/// The block ADR-188 item 3c gives the judge, in the form ADR-707 measured:
/// `About to run: <description>`, then one line per unit the call splits
/// into at `;`, `&&`, `&`, a newline, a loop body, a subshell or a
/// substitution, in order. Segments joined by `|` or `||` share a line with
/// the symbol between them (`git log (…) | grep (…) | head (…)`). Each
/// segment reads as [`Described::text`]. Navigation segments are dropped;
/// one still ends the line before it unless it joins by `|` or `||`. The
/// description's whitespace is collapsed. Nothing is escaped: the judge's
/// prompt renderer neutralizes markup.
pub fn described_block(description: &str, segments: &[Segment], data: &Summaries) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for d in describe(segments, data) {
        let inline = match d.segment.connector {
            Connector::Pipe => Some(" | "),
            Connector::Or => Some(" || "),
            _ => None,
        };
        if inline.is_none() && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        if d.segment.role == Role::Navigation {
            continue;
        }
        if let (Some(sym), false) = (inline, cur.is_empty()) {
            cur.push_str(sym);
        }
        cur.push_str(&d.text());
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let head = format!(
        "About to run: {}",
        description.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    std::iter::once(head)
        .chain(lines)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sentence_trims() {
        assert_eq!(
            first_sentence("Push commits to a remote repository."),
            "Push commits to a remote repository"
        );
        assert_eq!(
            first_sentence("The Silver Searcher. Like `ack`, but faster."),
            "The Silver Searcher"
        );
        assert_eq!(first_sentence("  Spaced   out  "), "Spaced out");
    }

    #[test]
    fn parse_skips_header_and_malformed_lines() {
        let s = Summaries::parse(
            "# header\tnot a page\n\ngit\tVersion control.\nno tab here\nempty\t\n",
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s.lookup("git", None), Some("Version control"));
    }

    #[test]
    fn lookup_falls_back_from_subcommand_to_program() {
        let s = Summaries::parse("gh\tGitHub CLI.\ngh-pr\tManage pull requests.\n");
        assert_eq!(
            s.lookup("gh", Some("pr create")),
            Some("Manage pull requests")
        );
        assert_eq!(s.lookup("gh", Some("repo view")), Some("GitHub CLI"));
        assert_eq!(s.lookup("GH", None), Some("GitHub CLI"));
        assert_eq!(s.lookup("nope", Some("x")), None);
    }
}
