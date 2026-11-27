// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The wrap's handoff file, `.claude/handoff.wrap.md` at the repository
//! root, as compaction instructions.
//! attend's check builds a relay compaction's instructions from it, and
//! `ways hook pre-compact` builds a native compaction's from it, so both
//! read freshness and cut the text the same way.
//!
//! The wrap skill writes the date, the HEAD commit and `wrap=<id>` on the
//! file's first line. A handoff is fresh for a wrap when that line carries
//! the wrap's id. The PreCompact hook, which asks attend for the id, also
//! takes a handoff whose first line names today's date and the current
//! HEAD, where attend knows no id or names another wrap than the file's.
//!
//! References:
//! - ADR-165: the wrap writes its handoff to `.claude/handoff.wrap.md`.
//! - ADR-417 items 3 and 4: a native compaction and a relay compaction both
//!   take their instructions from the handoff.

use std::path::{Path, PathBuf};

/// The handoff file under the repository root.
pub const FILE: &str = ".claude/handoff.wrap.md";

/// The line the instructions open with.
pub const LEAD: &str = "Summarize this session as the handoff below; keep its TASKS and DO NEXT whole.";

/// The most instructions a PreCompact hook may print, in characters, and
/// what the relay's instructions are held to as well.
pub const CAP: usize = 10_000;

/// The wrap skill's handoff outline (its Step 3 sections), for a compaction
/// with no fresh handoff: the summary is structured even when no wrap ran.
pub const OUTLINE: &str = "Summarize this session as a continuation prompt a stranger could resume from, in these sections:
- The project, its path and purpose, and how to build and test it.
- LANDED: what shipped this session, with PR numbers or commit refs.
- STATE, IN FLIGHT: anything parked, such as a branch, uncommitted files, and why.
- INVARIANTS / GOTCHAS: what git does not show, such as ordering rules and environment quirks.
- TASKS: every task with its number, status, subject and full description.
- DO NEXT: the immediate next step, then one to three alternatives, each specific enough to start.
- DECISIONS: each choice left to the operator, numbered, one line each.
- KEY FILES / BRANCHES: the handful that matter, with paths.
If a /goal is active, restate its condition at the top.";

/// The handoff file of the repository at `root`.
pub fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// The repository root `dir` sits in, by `git rev-parse --show-toplevel`,
/// else the nearest ancestor holding `.git`; `None` outside a repository.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok();
    if let Some(o) = out.filter(|o| o.status.success()) {
        let top = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !top.is_empty() {
            return Some(PathBuf::from(top));
        }
    }
    dir.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

/// The short HEAD commit of the repository at `root`.
pub fn head_short(root: &Path) -> Option<String> {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--short", "HEAD"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let h = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!h.is_empty()).then_some(h)
}

/// Whether `id` has a wrap id's shape: 8 to 64 letters, digits and `-`.
pub fn is_wrap_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The wrap id the handoff's first line carries, as `wrap=<id>`.
pub fn wrap_id_of(text: &str) -> Option<&str> {
    let first = text.lines().next()?;
    first.split_whitespace().find_map(|w| w.strip_prefix("wrap=")).filter(|id| is_wrap_id(id))
}

/// Whether the handoff `text` is the one wrap `id` wrote.
pub fn is_fresh_for(text: &str, id: &str) -> bool {
    is_wrap_id(id) && wrap_id_of(text) == Some(id)
}

/// Whether the handoff `text` names `head` (a short or full commit) and
/// `date` (`YYYY-MM-DD`) on its first line, the freshness test where no
/// wrap id is known.
pub fn is_fresh_by_head(text: &str, head: &str, date: &str) -> bool {
    let Some(first) = text.lines().next() else { return false };
    let words: Vec<&str> = first.split_whitespace().collect();
    let names_head = !head.is_empty() && words.iter().any(|w| w.len() >= 7 && (head.starts_with(*w) || w.starts_with(head)));
    names_head && words.contains(&date)
}

/// Whether `line` opens a handoff section: a markdown heading, or a word in
/// capitals followed by ` —` or `:`, as the skill's `TASKS —` and
/// `DECISIONS:` do.
fn is_section_start(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        return true;
    }
    let head: String = line.chars().take_while(|c| c.is_ascii_uppercase() || matches!(c, ' ' | '/' | ',' | '-')).collect();
    let rest = &line[head.len()..];
    head.chars().filter(|c| c.is_ascii_uppercase()).count() >= 3 && (rest.starts_with(" —") || rest.starts_with('—') || rest.starts_with(':') || head.ends_with(" -"))
}

/// The compaction instructions for the handoff `text` read from `file`:
/// [`LEAD`], then the handoff, cut at a section boundary so the whole stays
/// within [`CAP`] characters, with a closing line saying the file holds the
/// rest.
pub fn instructions(text: &str, file: &Path) -> String {
    let text = text.trim_end();
    let whole = format!("{LEAD}\n\n{text}");
    if whole.chars().count() <= CAP {
        return whole;
    }
    let tail = format!("\n\n(The handoff continues in {}; read it there.)", file.display());
    let budget = CAP.saturating_sub(LEAD.chars().count() + 2 + tail.chars().count());
    // The text up to the last section start that keeps it in budget; with
    // none, up to the last line that fits.
    let mut kept = 0usize;
    let mut cut = 0usize;
    let mut last_line = 0usize;
    for line in text.split_inclusive('\n') {
        let n = line.chars().count();
        if kept > 0 && is_section_start(line.trim_end()) {
            cut = kept;
        }
        if kept + n > budget {
            break;
        }
        kept += n;
        last_line = kept;
    }
    let at = if cut > 0 { cut } else { last_line };
    let body: String = text.chars().take(at).collect();
    format!("{LEAD}\n\n{}{tail}", body.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDOFF: &str = "2026-11-26 e6bbfbdd wrap=20261126T2140Z-3f9a1c2e\nContinue work on demo.\n\nTASKS — recreate these.\n#1 [OPEN] one\n\nDO NEXT — start #1.\n";

    #[test]
    fn the_wrap_id_on_the_first_line_decides_freshness() {
        assert_eq!(wrap_id_of(HANDOFF), Some("20261126T2140Z-3f9a1c2e"));
        assert!(is_fresh_for(HANDOFF, "20261126T2140Z-3f9a1c2e"));
        assert!(!is_fresh_for(HANDOFF, "20261126T2100Z-3f9a1c2e"), "another wrap's id");
        assert!(!is_fresh_for("Continue work\nwrap=20261126T2140Z-3f9a1c2e\n", "20261126T2140Z-3f9a1c2e"), "only the first line counts");
        assert!(!is_fresh_for(HANDOFF, "short"), "an id under 8 characters is no id");
        assert_eq!(wrap_id_of("2026-11-26 e6bbfbdd wrap=bad/id!\n"), None);
    }

    #[test]
    fn without_an_id_head_and_date_decide_freshness() {
        assert!(is_fresh_by_head(HANDOFF, "e6bbfbdd", "2026-11-26"));
        assert!(is_fresh_by_head(HANDOFF, "e6bbfbdd0123", "2026-11-26"), "a longer HEAD");
        assert!(!is_fresh_by_head(HANDOFF, "e6bbfbdd", "2026-11-27"), "another day");
        assert!(!is_fresh_by_head(HANDOFF, "abcdef12", "2026-11-26"), "another commit");
        assert!(!is_fresh_by_head("", "e6bbfbdd", "2026-11-26"));
    }

    #[test]
    fn a_short_handoff_is_given_whole_after_the_lead() {
        let s = instructions(HANDOFF, Path::new("/r/.claude/handoff.wrap.md"));
        assert!(s.starts_with(&format!("{LEAD}\n\n2026-11-26")), "{s}");
        assert!(s.ends_with("DO NEXT — start #1."), "{s}");
    }

    #[test]
    fn a_long_handoff_is_cut_at_a_section_boundary_within_the_cap() {
        let filler = "x".repeat(99) + "\n";
        let text = format!("2026-11-26 e6bbfbdd wrap=abcdefgh\n\nTASKS — keep.\n{}\nDO NEXT — first.\n{}\nKEY FILES / BRANCHES — tail.\n{}", filler.repeat(40), filler.repeat(40), filler.repeat(40));
        let s = instructions(&text, Path::new("/r/.claude/handoff.wrap.md"));
        assert!(s.chars().count() <= CAP, "{}", s.chars().count());
        assert!(s.contains("DO NEXT — first."), "the sections that fit stay");
        assert!(!s.contains("KEY FILES"), "the section that does not fit is cut whole");
        assert!(s.ends_with("(The handoff continues in /r/.claude/handoff.wrap.md; read it there.)"), "{}", &s[s.len() - 80..]);
    }

    #[test]
    fn a_handoff_with_no_sections_is_cut_at_a_line() {
        let text = "y".repeat(50) + "\n";
        let s = instructions(&text.repeat(400), Path::new("/f"));
        assert!(s.chars().count() <= CAP);
        assert!(s.contains("yyyy"));
    }

    #[test]
    fn section_starts_are_headings_and_capital_labels() {
        assert!(is_section_start("TASKS — recreate"));
        assert!(is_section_start("DECISIONS: none"));
        assert!(is_section_start("KEY FILES / BRANCHES — a"));
        assert!(is_section_start("## Tasks"));
        assert!(!is_section_start("Continue work on x"));
        assert!(!is_section_start("#1 [OPEN] x"), "a task line is no heading");
    }
}
