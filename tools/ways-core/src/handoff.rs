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

use crate::proc::Outcome;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The handoff file under the repository root.
pub const FILE: &str = ".claude/handoff.wrap.md";

/// The line the instructions open with.
pub const LEAD: &str = "Summarize this session as the handoff below; keep its TASKS and DO NEXT whole.";

/// The most instructions a PreCompact hook may print, in characters, and
/// what the relay's instructions are held to as well.
pub const CAP: usize = 10_000;

/// How long one git call may run before it is killed and taken as no
/// answer. The PreCompact hook makes two of them within its 9 seconds,
/// beside its wait on attend.
const GIT_WAIT: Duration = Duration::from_secs(2);

/// The handoff file of the repository at `root`.
pub fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// The repository root `dir` sits in, by `git rev-parse --show-toplevel`,
/// else the nearest ancestor holding `.git`; `None` outside a repository,
/// and `None` when git runs past [`GIT_WAIT`], since a hung git says
/// nothing sure about the repository.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    repo_root_by(OsStr::new("git"), dir, GIT_WAIT)
}

fn repo_root_by(program: &OsStr, dir: &Path, limit: Duration) -> Option<PathBuf> {
    match git(program, dir, &["rev-parse", "--show-toplevel"], limit) {
        Outcome::Stdout(top) if !top.trim().is_empty() => return Some(PathBuf::from(top.trim())),
        Outcome::TimedOut => return None,
        _ => {}
    }
    dir.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

/// The short HEAD commit of the repository at `root`; `None` when git
/// fails or runs past [`GIT_WAIT`].
pub fn head_short(root: &Path) -> Option<String> {
    head_short_by(OsStr::new("git"), root, GIT_WAIT)
}

fn head_short_by(program: &OsStr, root: &Path, limit: Duration) -> Option<String> {
    let Outcome::Stdout(h) = git(program, root, &["rev-parse", "--short", "HEAD"], limit) else { return None };
    let h = h.trim();
    (!h.is_empty()).then(|| h.to_string())
}

/// `git -C dir args`, killed with what it started at `limit`. It takes
/// no optional locks and never prompts for credentials, so it neither
/// holds the index lock of a repository another git is using nor waits
/// on a terminal.
fn git(program: &OsStr, dir: &Path, args: &[&str], limit: Duration) -> Outcome {
    let mut cmd = std::process::Command::new(program);
    cmd.arg("-C").arg(dir).args(args).env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    crate::proc::stdout_within(cmd, limit)
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

/// What a handoff section is to the cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// The lines before the first section: the first line and the opening.
    Lead,
    Tasks,
    DoNext,
    Other,
}

/// The section `line` opens, by its label: `TASKS —`, `## Do next`.
fn part_of(line: &str) -> Part {
    let label = line.trim_start_matches('#').trim_start().to_ascii_uppercase();
    if label.starts_with("TASKS") {
        Part::Tasks
    } else if label.starts_with("DO NEXT") {
        Part::DoNext
    } else {
        Part::Other
    }
}

/// `text` as its parts in order, each with its own lines.
fn parts(text: &str) -> Vec<(Part, String)> {
    let mut out: Vec<(Part, String)> = vec![(Part::Lead, String::new())];
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i > 0 && is_section_start(line.trim_end()) {
            out.push((part_of(line.trim_end()), String::new()));
        }
        if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
        }
    }
    out
}

/// The compaction instructions for the handoff `text` read from `file`:
/// [`LEAD`], then the handoff, within [`CAP`] characters, with a closing
/// line saying the file holds the rest. A handoff over the cap keeps its
/// opening, TASKS and DO NEXT: the other sections go first, the last of
/// them first, then TASKS is cut after a whole task, with a line naming the
/// tasks the file still holds. A handoff with neither section is cut at the
/// last section, else the last line, that fits.
pub fn instructions(text: &str, file: &Path) -> String {
    let text = text.trim_end();
    let whole = format!("{LEAD}\n\n{text}");
    if whole.chars().count() <= CAP {
        return whole;
    }
    let tail = format!("\n\n(The handoff continues in {}; read it there.)", file.display());
    let budget = CAP.saturating_sub(LEAD.chars().count() + 2 + tail.chars().count());
    let body = cut_by_part(text, budget, file).unwrap_or_else(|| cut_prefix(text, budget));
    format!("{LEAD}\n\n{}{tail}", body.trim_end())
}

/// `text` within `budget`, keeping its opening, TASKS and DO NEXT; `None`
/// where it has no TASKS or DO NEXT, or where they cannot fit.
fn cut_by_part(text: &str, budget: usize, file: &Path) -> Option<String> {
    let mut parts = parts(text);
    if !parts.iter().any(|(p, _)| matches!(p, Part::Tasks | Part::DoNext)) {
        return None;
    }
    let size = |ps: &[(Part, String)]| ps.iter().map(|(_, b)| b.chars().count()).sum::<usize>();
    while size(&parts) > budget {
        let Some(i) = parts.iter().rposition(|(p, _)| *p == Part::Other) else { break };
        parts.remove(i);
    }
    if size(&parts) > budget {
        let i = parts.iter().position(|(p, _)| *p == Part::Tasks)?;
        let room = budget.checked_sub(size(&parts) - parts[i].1.chars().count())?;
        parts[i].1 = cut_tasks(&parts[i].1, room, file)?;
    }
    Some(parts.into_iter().map(|(_, b)| b).collect())
}

/// Whether `line` opens a task: `#3 [OPEN] ...`, `3. ...`, `- ...`.
fn is_task_start(line: &str) -> bool {
    let digits = |s: &str| s.chars().take_while(char::is_ascii_digit).count();
    match line.as_bytes().first() {
        Some(b'#') => digits(&line[1..]) > 0,
        Some(b'-' | b'*') => line[1..].starts_with(' '),
        Some(c) if c.is_ascii_digit() => matches!(line[digits(line)..].chars().next(), Some('.' | ')')),
        _ => false,
    }
}

/// The task number `#3` opens with, where it has one.
fn task_number(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?;
    let n = rest.chars().take_while(char::is_ascii_digit).count();
    (n > 0).then(|| &rest[..n])
}

/// The TASKS section `body` within `room`: its heading and the whole tasks
/// that fit, then a line naming the tasks the file holds. `None` where not
/// even the heading and that line fit.
fn cut_tasks(body: &str, room: usize, file: &Path) -> Option<String> {
    let mut lines = body.split_inclusive('\n');
    let mut kept = lines.next()?.to_string();
    let mut tasks: Vec<String> = Vec::new();
    for line in lines {
        match tasks.last_mut() {
            Some(t) if !is_task_start(line) => t.push_str(line),
            _ if is_task_start(line) => tasks.push(line.to_string()),
            // Lines under the heading before the first task stay with it.
            _ => kept.push_str(line),
        }
    }
    let note = |from: usize| {
        let first = tasks.get(from).and_then(|t| task_number(t));
        let last = tasks.last().and_then(|t| task_number(t));
        match (first, last) {
            (Some(a), Some(b)) if a == b => format!("(Task #{a} is in {}; read it there.)\n\n", file.display()),
            (Some(a), Some(b)) => format!("(Tasks #{a} to #{b} are in {}; read them there.)\n\n", file.display()),
            _ => format!("(The rest of TASKS is in {}; read it there.)\n\n", file.display()),
        }
    };
    let mut n = 0;
    let mut size = kept.chars().count();
    while n < tasks.len() && size + tasks[n].chars().count() + note(n + 1).chars().count() <= room {
        size += tasks[n].chars().count();
        n += 1;
    }
    let closing = note(n);
    if size + closing.chars().count() > room {
        return None;
    }
    kept.extend(tasks.iter().take(n).map(String::as_str));
    kept.push_str(&closing);
    Some(kept)
}

/// The text up to the last section start that keeps it in `budget`; with
/// none, up to the last line that fits.
fn cut_prefix(text: &str, budget: usize) -> String {
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
    text.chars().take(at).collect()
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

    /// The review's case: a first line, TASKS at 9,800 characters, then DO
    /// NEXT. The cut keeps the first line and DO NEXT whole and as many
    /// whole tasks as fit, with a line naming the file for the rest.
    #[test]
    fn a_long_tasks_section_is_cut_at_a_task_and_do_next_stays() {
        let task = |n: usize| format!("#{n} [OPEN] task {n}\n  {}\n", "d".repeat(90));
        let tasks: String = (1..=100).map(task).collect();
        assert!(tasks.len() > 9_700, "{}", tasks.len());
        let text = format!("2026-11-26 e6bbfbdd wrap=abcdefgh\nContinue work on demo.\n\nTASKS — recreate these.\n{tasks}\nDO NEXT — start #1.\n");
        let s = instructions(&text, Path::new("/r/.claude/handoff.wrap.md"));
        assert!(s.chars().count() <= CAP, "{}", s.chars().count());
        assert!(s.starts_with(&format!("{LEAD}\n\n2026-11-26 e6bbfbdd wrap=abcdefgh\nContinue work on demo.\n\nTASKS — recreate these.\n#1 [OPEN] task 1\n")), "{}", &s[..300]);
        assert!(s.contains("DO NEXT — start #1."), "DO NEXT stays whole");
        assert!(s.contains("#2 [OPEN] task 2\n"), "as many tasks as fit");
        assert!(!s.contains("#100 [OPEN]"), "the tasks that do not fit are cut");
        let kept = s.matches("[OPEN] task").count();
        assert!(s.contains(&format!("(Tasks #{} to #100 are in /r/.claude/handoff.wrap.md; read them there.)", kept + 1)), "{}", &s[s.len() - 300..]);
        // A cut task is cut whole: no task line without its description.
        let last = s.lines().rfind(|l| l.starts_with('#')).unwrap();
        let after = s.split(last).nth(1).unwrap();
        assert!(after.starts_with(&format!("\n  {}", "d".repeat(90))), "{}", &after[..40]);
    }

    /// The other sections go before TASKS is cut, the last first, even one
    /// that comes before TASKS; TASKS and DO NEXT stay whole while they fit.
    #[test]
    fn other_sections_are_cut_before_tasks() {
        let filler = "x".repeat(99) + "\n";
        let text = format!(
            "2026-11-26 e6bbfbdd wrap=abcdefgh\n\nLANDED — a.\n{}\nTASKS — keep.\n#1 [OPEN] one\n{}\nDO NEXT — first.\n\nKEY FILES / BRANCHES — k.\n{}",
            filler.repeat(80),
            filler.repeat(20),
            filler.repeat(10)
        );
        let s = instructions(&text, Path::new("/r/.claude/handoff.wrap.md"));
        assert!(s.chars().count() <= CAP, "{}", s.chars().count());
        assert!(s.contains(&format!("TASKS — keep.\n#1 [OPEN] one\n{}", filler.repeat(20))) && s.contains("DO NEXT — first."), "{s}");
        assert!(!s.contains("Tasks #"), "TASKS is whole");
        assert!(!s.contains("KEY FILES") && !s.contains("LANDED"), "the other sections go");
        assert!(s.starts_with(&format!("{LEAD}\n\n2026-11-26 e6bbfbdd wrap=abcdefgh\n")));
        assert!(s.ends_with("(The handoff continues in /r/.claude/handoff.wrap.md; read it there.)"));
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

    /// A directory of its own under the system temp dir, holding a fake
    /// `git` script with `body`, and the script's path.
    #[cfg(unix)]
    fn fake_git(tag: &str, body: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("ways-core-handoff-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let git = dir.join("git");
        std::fs::write(&git, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, git)
    }

    /// A git that hangs gives no repository root and no HEAD within the
    /// limit, though the directory sits in a repository by its `.git`.
    #[cfg(unix)]
    #[test]
    fn a_hung_git_gives_no_root_and_no_head_within_the_limit() {
        let (dir, git) = fake_git("hung", "sleep 10\necho /from/git");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let limit = Duration::from_millis(300);
        let started = std::time::Instant::now();
        let root = repo_root_by(git.as_os_str(), &dir, limit);
        let head = head_short_by(git.as_os_str(), &dir, limit);
        let took = started.elapsed();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(root, None, "a timeout is no root, not the .git fallback");
        assert_eq!(head, None);
        assert!(took < Duration::from_secs(3), "both calls return near their limit, took {took:?}");
    }

    /// A git that answers is taken, and one that fails falls back to the
    /// nearest `.git`.
    #[cfg(unix)]
    #[test]
    fn a_git_that_answers_or_fails_is_taken_within_the_limit() {
        let (dir, git) = fake_git("answers", "echo /from/git");
        assert_eq!(repo_root_by(git.as_os_str(), &dir, GIT_WAIT), Some(PathBuf::from("/from/git")));
        assert_eq!(head_short_by(git.as_os_str(), &dir, GIT_WAIT).as_deref(), Some("/from/git"));
        let (failing, false_git) = fake_git("fails", "exit 128");
        std::fs::create_dir_all(failing.join(".git")).unwrap();
        let sub = failing.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(repo_root_by(false_git.as_os_str(), &sub, GIT_WAIT), Some(failing.clone()));
        assert_eq!(head_short_by(false_git.as_os_str(), &sub, GIT_WAIT), None);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&failing).ok();
    }
}
