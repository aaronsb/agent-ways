//! No caller or text names a flat attend verb that ADR-413 moved under a
//! group (item 6.4). The scan covers plugins/, hooks/, skills/, statusline.sh,
//! README.md, docs/, tests/, attend's own crate and every crate attend names
//! by path in its Cargo.toml. Records under docs/architecture/ are history
//! and keep the names current when they were written, except a decision
//! still proposed, whose commands someone will type. Changelogs are
//! append-only, and the record fixtures under tests/fixtures/ are frozen
//! copies of records. tests/legacy_removed.rs names the old verbs on purpose,
//! to show each one is refused (#982). A skill invocation such as
//! `/attend status` is the skill's argument, not the CLI.

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

/// attend's crate and the crates it names by path in its Cargo.toml.
fn attend_crates(root: &Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(root.join("tools/attend/Cargo.toml")).expect("attend's Cargo.toml");
    let mut crates = vec!["tools/attend/".to_string()];
    for (at, _) in manifest.match_indices("path = \"../") {
        let rest = &manifest[at + "path = \"../".len()..];
        crates.push(format!("tools/{}/", &rest[..rest.find('"').unwrap()]));
    }
    crates
}

fn in_scope(path: &str, crates: &[String]) -> bool {
    const EXEMPT: &[&str] = &[
        "tools/attend/tests/legacy_removed.rs",
        "tests/fixtures/adr/",
        "tests/fixtures/docker/scenarios/adr-migrate/",
    ];
    let roots = ["plugins/", "hooks/", "skills/", "docs/", "tests/"];
    let scanned = matches!(path, "statusline.sh" | "README.md")
        || roots.iter().any(|r| path.starts_with(r))
        || crates.iter().any(|c| path.starts_with(c.as_str()));
    scanned && !path.contains("CHANGELOG") && !EXEMPT.iter().any(|e| path.starts_with(e))
}

/// A record under docs/architecture/ is scanned only while it is a proposed
/// decision.
fn live_record(text: &str) -> bool {
    let front = text.strip_prefix("---\n").and_then(|t| t.split_once("\n---")).map(|(f, _)| f).unwrap_or("");
    let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_ascii_lowercase());
    field("status:").as_deref() == Some("proposed") && field("kind:").as_deref() == Some("decision")
}

/// Each flat verb a line names as `attend <verb>`, with word boundaries on
/// both sides, as `\battend (verb)\b` matches. A `/` right before `attend`
/// at the start of a word is a skill invocation and is passed over; one
/// inside a path (`~/.cargo/bin/attend`) is the binary.
fn flat_verbs(line: &str) -> Vec<&'static str> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    for (at, _) in line.match_indices("attend ") {
        let mut before = line[..at].chars().rev();
        match before.next() {
            Some(c) if word(c) => continue,
            Some('/') if !before.next().is_some_and(|c| word(c) || matches!(c, '.' | '-' | '~' | '/')) => continue,
            _ => {}
        }
        let rest = &line[at + "attend ".len()..];
        let end = rest.find(|c: char| !word(c)).unwrap_or(rest.len());
        if let Some(v) = FLAT.iter().find(|v| **v == &rest[..end]) {
            found.push(*v);
        }
    }
    found
}

// The samples are built with concat! so this file's own text holds no flat
// verb for the scan to find.
#[test]
fn the_matcher_finds_flat_verbs_and_passes_grouped_paths() {
    assert_eq!(flat_verbs(concat!("run `attend ", "send \"hi\"` then attend ", "peers")), ["send", "peers"]);
    assert_eq!(flat_verbs(concat!("exec attend ", "inbox --drain")), ["inbox"]);
    assert_eq!(flat_verbs(concat!("~/.cargo/bin/attend ", "status")), ["status"]);
    assert!(flat_verbs(concat!("- `/attend ", "status` runs it via Bash")).is_empty());
    assert!(flat_verbs("attend message send, attend channel join x, attend peer list").is_empty());
    assert!(flat_verbs(concat!("attend sending, reattend ", "send, attend-send, attend run")).is_empty());
}

#[test]
fn no_caller_names_a_flat_attend_verb() {
    let root = repo_root();
    let crates = attend_crates(&root);
    assert!(crates.len() > 5, "attend names its crates by path: {crates:?}");
    let mut scanned = 0;
    let mut hits = Vec::new();
    for path in tracked_files(&root).into_iter().filter(|p| in_scope(p, &crates)) {
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
