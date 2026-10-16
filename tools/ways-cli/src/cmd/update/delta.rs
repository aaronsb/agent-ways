//! The source delta of a what-if: the commits between the app's HEAD and the
//! update's target, grouped by area, with their conventional-commit type and
//! the pull requests they merged.

use crate::cmd::lifecycle::{Mark, Report};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// One non-merge commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub subject: String,
    pub area: String,
}

/// The commits in `range` and the pull requests its merges name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    pub commits: Vec<Commit>,
    pub prs: Vec<u32>,
}

/// Read `from..to` in `app`. `None` when git cannot answer.
pub fn read(app: &Path, from: &str, to: &str) -> Option<Delta> {
    let range = format!("{from}..{to}");
    let log = git(app, &["log", "--no-merges", "--format=%x1e%s", "--name-only", &range])?;
    let merges = git(app, &["log", "--merges", "--format=%s", &range])?;
    let mut prs: BTreeSet<u32> = merges.lines().filter_map(merged_pr).collect();
    let commits: Vec<Commit> = log
        .split('\x1e')
        .filter(|c| !c.trim().is_empty())
        .map(|chunk| {
            let mut lines = chunk.lines();
            let subject = lines.next().unwrap_or("").trim().to_string();
            let paths: Vec<&str> = lines.map(str::trim).filter(|l| !l.is_empty()).collect();
            if let Some(n) = squash_pr(&subject) {
                prs.insert(n);
            }
            classify(&subject, &paths)
        })
        .collect();
    Some(Delta { commits, prs: prs.into_iter().collect() })
}

fn git(app: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("--no-optional-locks").args(args).current_dir(app).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The PR a merge commit's subject names: `Merge pull request #1023 from …`.
pub fn merged_pr(subject: &str) -> Option<u32> {
    subject.strip_prefix("Merge pull request #")?.split_whitespace().next()?.parse().ok()
}

/// The PR a squashed commit names at its end: `feat: x (#12)`.
fn squash_pr(subject: &str) -> Option<u32> {
    subject.trim_end().strip_suffix(')')?.rsplit_once("(#")?.1.parse().ok()
}

/// Split a conventional subject into type, scope and description:
/// `feat(attend)!: x` → (`feat`, `attend`, `x`). A subject that is not
/// conventional has no type.
pub fn conventional(subject: &str) -> (Option<String>, Option<String>, String) {
    let Some((head, rest)) = subject.split_once(": ") else { return (None, None, subject.to_string()) };
    let head = head.trim_end_matches('!');
    let (kind, scope) = match head.split_once('(') {
        Some((k, s)) => (k, s.strip_suffix(')').map(str::to_string)),
        None => (head, None),
    };
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_lowercase()) {
        return (None, None, subject.to_string());
    }
    (Some(kind.to_string()), scope, rest.to_string())
}

/// The area a scope names, when it names one.
fn area_of_scope(scope: &str) -> Option<&'static str> {
    Some(match scope {
        "ways" | "ways-cli" | "cli" | "settings" | "manage" | "update" => "ways",
        "ways-audit" | "audit" => "ways-audit",
        "ways-mcp" | "mcp" => "ways-mcp",
        "ways-agent" | "agent" | "judge" | "gate" => "ways-agent",
        "attend" => "attend",
        "attend-chat" | "chat" => "attend-chat",
        "way-embed" | "embed" => "way-embed",
        "mod" | "attend-mod" | "plugin" | "plugins" => "attend mod",
        "corpus" | "way" | "hooks" | "core" | "skills" => "ways corpus",
        "docs" | "adr" | "readme" => "docs",
        "ci" | "release" | "build" | "make" | "makefile" | "install" | "installer" => "build and release",
        _ => return None,
    })
}

/// The area a path belongs to.
fn area_of_path(path: &str) -> &'static str {
    let crate_area = |c: &str| -> &'static str {
        match c {
            "ways-cli" | "ways-core" => "ways",
            "ways-audit" => "ways-audit",
            "ways-mcp" => "ways-mcp",
            "ways-agent" | "ways-agent-core" => "ways-agent",
            "attend-chat" => "attend-chat",
            "way-embed" => "way-embed",
            c if c.starts_with("attend") || c.starts_with("sensor-") => "attend",
            _ => "shared crates",
        }
    };
    if let Some(rest) = path.strip_prefix("tools/") {
        return crate_area(rest.split('/').next().unwrap_or(""));
    }
    match path.split('/').next().unwrap_or("") {
        "plugins" => "attend mod",
        "hooks" | "skills" | "agents" | "commands" => "ways corpus",
        "docs" => "docs",
        ".github" | "scripts" | "Makefile" => "build and release",
        _ => "other",
    }
}

/// A commit's type, scope and area: the scope's area when it names one,
/// else the area most of its paths fall in.
pub fn classify(subject: &str, paths: &[&str]) -> Commit {
    let (kind, scope, _) = conventional(subject);
    let area = scope.as_deref().and_then(area_of_scope).map(str::to_string).unwrap_or_else(|| {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for p in paths {
            let a = area_of_path(p);
            match counts.iter_mut().find(|(x, _)| *x == a) {
                Some(c) => c.1 += 1,
                None => counts.push((a, 1)),
            }
        }
        counts.into_iter().max_by_key(|(_, n)| *n).map_or("other", |(a, _)| a).to_string()
    });
    Commit { kind, scope, subject: subject.to_string(), area }
}

/// How a type is shown: its mark, and whether it is listed by default.
fn mark_of(kind: Option<&str>) -> (Mark, bool) {
    match kind {
        Some("feat") => (Mark::Add, true),
        Some("fix") => (Mark::Fix, true),
        Some("revert") => (Mark::Remove, true),
        _ => (Mark::Info, false),
    }
}

/// Write the delta into `r`: per area, its counts by type, the features and
/// fixes listed, and the rest summarised unless `all`.
/// Which way the commits move: into the checkout, or out of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The commits an update brings in, with the pull requests they merged.
    Incoming,
    /// The commits a `--ref` behind HEAD leaves behind.
    Dropped,
}

pub fn render(r: &mut Report, d: &Delta, all: bool, direction: Direction) {
    let mut areas: Vec<(&str, Vec<&Commit>)> = Vec::new();
    for c in &d.commits {
        match areas.iter_mut().find(|(a, _)| *a == c.area) {
            Some((_, v)) => v.push(c),
            None => areas.push((c.area.as_str(), vec![c])),
        }
    }
    areas.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    let total = d.commits.len();
    match direction {
        Direction::Incoming => r.text(2, format!("{total} commit{}, {} pull request{} merged", plural(total), d.prs.len(), plural(d.prs.len()))),
        Direction::Dropped => r.text(2, format!("{total} commit{} the checkout would leave behind", plural(total))),
    };
    for (area, commits) in &areas {
        let mut kinds: Vec<(String, usize)> = Vec::new();
        for c in commits {
            let k = c.kind.clone().unwrap_or_else(|| "other".to_string());
            match kinds.iter_mut().find(|(x, _)| *x == k) {
                Some(e) => e.1 += 1,
                None => kinds.push((k, 1)),
            }
        }
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let counts: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
        r.blank();
        r.sub(format!("{area} ({})", counts.join(", ")));
        let mut hidden = 0;
        for c in commits {
            let (mark, listed) = mark_of(c.kind.as_deref());
            if listed || all {
                r.tagged(mark, 4, c.kind.as_deref().unwrap_or("·"), 8, &c.subject);
            } else {
                hidden += 1;
            }
        }
        if hidden > 0 {
            r.muted(4, format!("{hidden} other commit{} (--verbose lists them)", plural(hidden)));
        }
    }
    if !d.prs.is_empty() && direction == Direction::Incoming {
        r.blank();
        let list: Vec<String> = d.prs.iter().map(|n| format!("#{n}")).collect();
        r.muted(2, format!("pull requests: {}", list.join(" ")));
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_theme::Painter;

    #[test]
    fn conventional_subjects_split_into_type_scope_and_text() {
        assert_eq!(conventional("feat(attend): wake on mail"), (Some("feat".into()), Some("attend".into()), "wake on mail".into()));
        assert_eq!(conventional("fix!: x"), (Some("fix".into()), None, "x".into()));
        assert_eq!(conventional("Update README"), (None, None, "Update README".into()));
        assert_eq!(conventional("WIP: x"), (None, None, "WIP: x".into()));
    }

    #[test]
    fn the_scope_names_the_area_and_paths_decide_otherwise() {
        assert_eq!(classify("feat(attend-chat): tabs", &["tools/attend/src/x.rs"]).area, "attend-chat");
        assert_eq!(classify("fix: thing", &["tools/attend/src/a.rs", "tools/attend/src/b.rs", "docs/x.md"]).area, "attend");
        assert_eq!(classify("docs(weird-scope): x", &["hooks/ways/core.md"]).area, "ways corpus");
        assert_eq!(classify("chore: bump", &[]).area, "other");
        assert_eq!(classify("feat(mod): x", &[]).area, "attend mod");
        assert_eq!(merged_pr("Merge pull request #1023 from aaronsb/release/x"), Some(1023));
        assert_eq!(merged_pr("Merge branch 'main'"), None);
        assert_eq!(squash_pr("feat: x (#12)"), Some(12));
    }

    #[test]
    fn the_render_lists_feats_and_fixes_and_counts_the_rest() {
        let d = Delta {
            commits: vec![
                classify("feat(attend): one", &[]),
                classify("fix(attend): two", &[]),
                classify("docs(attend): three", &[]),
                classify("test(attend): four", &[]),
                classify("fix(ways): five", &[]),
            ],
            prs: vec![7, 9],
        };
        let mut r = Report::new("t");
        r.section("Changes");
        render(&mut r, &d, false, Direction::Incoming);
        let text = r.render(&Painter::plain());
        let body: Vec<&str> = text.lines().skip(3).collect();
        assert_eq!(
            body.join("\n"),
            "  5 commits, 2 pull requests merged\n\n\
             \x20 attend (1 docs, 1 feat, 1 fix, 1 test)\n    feat     feat(attend): one\n    fix      fix(attend): two\n    2 other commits (--verbose lists them)\n\n\
             \x20 ways (1 fix)\n    fix      fix(ways): five\n\n  pull requests: #7 #9"
        );
        let mut all = Report::new("t");
        render(&mut all, &d, true, Direction::Incoming);
        assert!(all.render(&Painter::plain()).contains("docs     docs(attend): three"));
        let mut gone = Report::new("t");
        render(&mut gone, &Delta { commits: vec![classify("feat(ways): x", &[])], prs: vec![] }, false, Direction::Dropped);
        let gone = gone.render(&Painter::plain());
        assert!(gone.contains("1 commit the checkout would leave behind") && !gone.contains("merged"), "{gone}");
    }
}
