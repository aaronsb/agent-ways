//! No caller or text names a flat attend verb that ADR-413 moved under a
//! group (item 6.4). The scan covers plugins/, hooks/, skills/, statusline.sh,
//! tools/attend-chat/src and docs/. Records under docs/architecture/ are
//! history and keep the names current when they were written, except a
//! decision still proposed, whose commands someone will type. Changelogs are
//! append-only. The alias tests and tests/legacy_removed.rs, which name the
//! old verbs on purpose, sit outside the scanned roots.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The verbs ADR-413 moved, as `attend <verb>` would name them.
const FLAT: &[&str] = &[
    "send", "reply", "inbox", "unread", "join", "leave", "channels", "dissolve", "scene", "scenes", "peers", "whoami",
    "status", "sensors", "permissions", "tune", "cleanup",
];

fn repo_root() -> PathBuf {
    // Read at run time: a test binary reused from another checkout keeps the
    // path it was built at.
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into()))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn tracked_files(root: &Path) -> Vec<String> {
    let out = Command::new("git").arg("-C").arg(root).args(["ls-files"]).output().expect("git ls-files");
    assert!(out.status.success(), "git ls-files: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

fn in_scope(path: &str) -> bool {
    let roots = ["plugins/", "hooks/", "skills/", "tools/attend-chat/src/", "docs/"];
    (path == "statusline.sh" || roots.iter().any(|r| path.starts_with(r))) && !path.contains("CHANGELOG")
}

/// A record under docs/architecture/ is scanned only while it is a proposed
/// decision.
fn live_record(text: &str) -> bool {
    let front = text.strip_prefix("---\n").and_then(|t| t.split_once("\n---")).map(|(f, _)| f).unwrap_or("");
    let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_ascii_lowercase());
    field("status:").as_deref() == Some("proposed") && field("kind:").as_deref() == Some("decision")
}

/// Each flat verb a line names as `attend <verb>`, with word boundaries on
/// both sides, as `\battend (verb)\b` matches.
fn flat_verbs(line: &str) -> Vec<&'static str> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    for (at, _) in line.match_indices("attend ") {
        if line[..at].chars().next_back().is_some_and(word) {
            continue;
        }
        let rest = &line[at + "attend ".len()..];
        let end = rest.find(|c: char| !word(c)).unwrap_or(rest.len());
        if let Some(v) = FLAT.iter().find(|v| **v == &rest[..end]) {
            found.push(*v);
        }
    }
    found
}

#[test]
fn the_matcher_finds_flat_verbs_and_passes_grouped_paths() {
    assert_eq!(flat_verbs("run `attend send \"hi\"` then attend peers"), ["send", "peers"]);
    assert_eq!(flat_verbs("exec attend inbox --drain"), ["inbox"]);
    assert_eq!(flat_verbs("/attend status"), ["status"]);
    assert!(flat_verbs("attend message send, attend channel join x, attend peer list").is_empty());
    assert!(flat_verbs("attend sending, reattend send, attend-send, attend run").is_empty());
}

#[test]
fn no_caller_names_a_flat_attend_verb() {
    let root = repo_root();
    let mut scanned = 0;
    let mut hits = Vec::new();
    for path in tracked_files(&root).into_iter().filter(|p| in_scope(p)) {
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else { continue };
        if path.starts_with("docs/architecture/") && !live_record(&text) {
            continue;
        }
        scanned += 1;
        for (n, line) in text.lines().enumerate() {
            for verb in flat_verbs(line) {
                hits.push(format!("{path}:{}: `attend {verb}`", n + 1));
            }
        }
    }
    assert!(scanned > 100, "scanned only {scanned} files: the roots moved?");
    assert!(
        hits.is_empty(),
        "flat attend verbs that ADR-413 moved under a group (see `attend --help`):\n{}",
        hits.join("\n")
    );
}
