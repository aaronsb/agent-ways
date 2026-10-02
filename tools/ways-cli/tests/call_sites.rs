//! Every `ways …` call site in the repository names a command the CLI accepts
//! (ADR-507). A renamed command leaves no caller behind: hooks, skills,
//! commands, scripts, the Makefiles, CI, settings.json, the docs, and the
//! strings and spawns in the Rust sources.
//!
//! A call site is `ways <command>` in code: a shell or Makefile line, a YAML
//! or JSON value, a fenced block or backticked span in markdown, a quoted
//! string or backticked span in Rust, or a spawn of the ways binary. Prose is
//! skipped, since "how ways match" is English. Each distinct command, and each
//! verb of a group, is checked with `ways <command> [<verb>] --help`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tracked_files(root: &Path) -> Vec<String> {
    let out = Command::new("git").arg("-C").arg(root).args(["ls-files"]).output().expect("git ls-files");
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

/// Files whose `ways` mentions are read. ADRs, changelogs and the rename table
/// are history and keep the names that were current when they were written.
fn in_scope(path: &str) -> bool {
    // agent-tui is a library whose own tests use `ways` as a sample app name.
    let skip = ["docs/architecture/", "tests/fixtures/adr/", "tests/fixtures/docker/scenarios/", "docs/research/", "tools/agent-tui/"];
    if skip.iter().any(|s| path.starts_with(s)) || path.contains("CHANGELOG") || path == "docs/reference/ways-cli-renames.md" {
        return false;
    }
    let roots = ["hooks/", "skills/", "commands/", "scripts/", ".github/", "docs/", "tests/", "tools/"];
    roots.iter().any(|r| path.starts_with(r))
        || matches!(path, "Makefile" | "settings.json" | "README.md")
        || path.ends_with("/Makefile")
}

#[derive(PartialEq)]
enum Kind {
    Shell,
    Markdown,
    Rust,
}

fn kind(path: &str) -> Option<Kind> {
    let name = path.rsplit('/').next().unwrap_or(path);
    if path.ends_with(".md") {
        Some(Kind::Markdown)
    } else if path.ends_with(".rs") {
        Some(Kind::Rust)
    } else if path.ends_with(".sh")
        || path.ends_with(".yml")
        || path.ends_with(".yaml")
        || path.ends_with(".json")
        || name == "Makefile"
        || (path.starts_with("hooks/") && !name.contains('.'))
    {
        Some(Kind::Shell)
    } else {
        None
    }
}

/// The text of a shell-like line that runs: a `#` comment is dropped.
fn shell_code(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return &line[..i];
        }
    }
    line
}

/// The backticked spans of a line.
fn backticked(line: &str) -> Vec<&str> {
    line.split('`').skip(1).step_by(2).collect()
}

fn word(s: &str) -> Option<&str> {
    let end = s.find(|c: char| !(c.is_ascii_lowercase() || c == '-')).unwrap_or(s.len());
    let w = &s[..end];
    (!w.is_empty() && w.as_bytes()[0].is_ascii_lowercase()).then_some(w)
}

/// Whether `ways` at this point of a line is in command position: the start
/// of the line, or after `$(`, a backtick, `&&`, `||`, `|`, `;`, or a
/// `timeout N` / `exec` prefix, and not inside a double-quoted string.
fn command_position(before: &str) -> bool {
    let b = before.trim_end();
    if b.is_empty() || before.ends_with("$(") || before.ends_with('`') {
        return true;
    }
    // Inside a double-quoted string only `$(` and a backtick start a command.
    let segment = before.rsplit("$(").next().unwrap_or(before);
    if segment.matches('"').count() % 2 == 1 {
        return false;
    }
    if ["&&", "||", "|", ";", " then", " do", " exec"].iter().any(|t| b.ends_with(t)) {
        return true;
    }
    // `timeout 10 ways …`
    let mut words = b.rsplitn(3, ' ');
    let (last, prev) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    prev.ends_with("timeout") && !last.is_empty() && last.bytes().all(|c| c.is_ascii_digit())
}

/// `ways <a> [<b>]` invocations in a line of code. The binary may be spelled
/// `ways` in command position, a path ending in `/ways`, or a variable such as
/// `$WAYS_BIN`.
fn invocations(code: &str) -> Vec<(String, Option<String>)> {
    let mut ends = Vec::new();
    for (start, _) in code.match_indices("ways") {
        let before = &code[..start];
        // A path to the binary: `"$APP/bin/ways"`, `~/.claude/bin/ways`.
        let path = before.rsplit(|c: char| c.is_whitespace() || c == '"').next().unwrap_or("");
        let after_path = path.ends_with("bin/") && path.starts_with(['$', '/', '~', '.']);
        if command_position(before) || after_path {
            ends.push(start + 4);
        }
    }
    for marker in ["$WAYS_BIN", "$WAYS_TEST_BIN", "$(WAYS_BIN)", "${WAYS_BIN}"] {
        ends.extend(code.match_indices(marker).map(|(at, m)| at + m.len()));
    }
    let mut found = Vec::new();
    for end in ends {
        let rest = code[end..].trim_start_matches('"');
        let Some(rest) = rest.strip_prefix(' ') else { continue };
        let Some(a) = word(rest) else { continue };
        let after = rest[a.len()..].strip_prefix(' ').and_then(word).map(str::to_string);
        found.push((a.to_string(), after));
    }
    found
}

/// `Command::new(<ways binary>).arg("x")` or `.args(["x", "y"])` in Rust.
fn spawns(text: &str) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    let mut i = 0;
    while let Some(off) = text[i..].find("Command::new(") {
        let start = i + off;
        i = start + 13;
        let Some(close) = text[i..].find(')') else { break };
        let target = &text[i..i + close];
        if !(target.contains("ways_bin") || target.contains("\"ways\"") || target.contains("ways_exe")) {
            continue;
        }
        let tail = &text[i + close..(i + close + 200).min(text.len())];
        let tail = tail.trim_start_matches(')').trim_start();
        let args: Vec<String> = if let Some(r) = tail.strip_prefix(".args([") {
            r.split(']').next().unwrap_or("").split(',').map(|s| s.trim().trim_matches('"').to_string()).collect()
        } else if let Some(r) = tail.strip_prefix(".arg(\"") {
            vec![r.split('"').next().unwrap_or("").to_string()]
        } else {
            continue;
        };
        if let Some(a) = args.first().filter(|a| word(a) == Some(a.as_str())) {
            let b = args.get(1).filter(|b| word(b) == Some(b.as_str())).cloned();
            found.push((a.clone(), b));
        }
    }
    found
}

fn call_sites(root: &Path) -> BTreeMap<(String, Option<String>), Vec<String>> {
    let mut sites: BTreeMap<(String, Option<String>), Vec<String>> = BTreeMap::new();
    for path in tracked_files(root) {
        if !in_scope(&path) || path.ends_with("tests/call_sites.rs") {
            continue;
        }
        let Some(k) = kind(&path) else { continue };
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else { continue };
        // Inside a fence: Some(true) for a shell fence, Some(false) for any other.
        let mut fence: Option<bool> = None;
        for (n, line) in text.lines().enumerate() {
            let mut found = Vec::new();
            match k {
                Kind::Shell => found.extend(invocations(shell_code(line))),
                Kind::Markdown => {
                    if let Some(lang) = line.trim_start().strip_prefix("```") {
                        fence = match fence {
                            Some(_) => None,
                            None => Some(matches!(lang.trim(), "" | "bash" | "sh" | "shell" | "zsh" | "console")),
                        };
                        continue;
                    }
                    if let Some(shell) = fence {
                        if shell {
                            found.extend(invocations(shell_code(line)));
                        }
                    } else {
                        for span in backticked(line) {
                            found.extend(invocations(span.trim_start()));
                        }
                    }
                }
                Kind::Rust => {
                    for span in backticked(line) {
                        found.extend(invocations(span.trim_start()));
                    }
                    // An indented string is a help listing: "  ways author lint <path>".
                    for quoted in line.split('"').skip(1).step_by(2) {
                        if quoted.starts_with("  ") {
                            found.extend(invocations(quoted.trim_start()));
                        }
                    }
                }
            }
            for site in found {
                sites.entry(site).or_default().push(format!("{path}:{}", n + 1));
            }
        }
        if k == Kind::Rust {
            for site in spawns(&text) {
                sites.entry(site).or_default().push(format!("{path} (spawn)"));
            }
        }
    }
    sites
}

/// Commands that no longer exist, which docs and tests name as removed.
const REMOVED: &[&str] = &["migrate", "rethink", "embed", "tune-curves"];

fn ways_help(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ways"))
        .args(args)
        .arg("--help")
        .env("NO_COLOR", "1")
        .output()
        .expect("run ways");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn every_ways_call_site_names_a_command_the_cli_accepts() {
    let root = repo_root();
    let sites = call_sites(&root);
    // Positive control: callers the scan must see, or it found nothing to check.
    for must in [("corpus", None), ("author", Some("lint")), ("settings", Some("set"))] {
        let key = (must.0.to_string(), must.1.map(str::to_string));
        assert!(sites.contains_key(&key), "the scan missed `ways {} {}`", must.0, must.1.unwrap_or(""));
    }

    let mut groups: BTreeMap<String, Option<bool>> = BTreeMap::new();
    let mut bad = Vec::new();
    for ((a, b), places) in &sites {
        // Named as removed, in the docs that record their removal and in the
        // tests that keep them rejected.
        if REMOVED.contains(&a.as_str()) {
            continue;
        }
        // `ways agent …` hands its arguments to the ways-agent binary.
        let group = groups.entry(a.clone()).or_insert_with(|| {
            if a == "agent" {
                return Some(false);
            }
            let (ok, out) = ways_help(&[a]);
            ok.then(|| out.contains("\nCommands:\n"))
        });
        let ok = match (*group, b) {
            (None, _) => false,
            (Some(true), Some(b)) => ways_help(&[a, b]).0,
            (Some(_), _) => true,
        };
        if !ok {
            let shown = places.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            bad.push(format!("ways {a}{}  ({shown})", b.as_ref().map(|b| format!(" {b}")).unwrap_or_default()));
        }
    }
    assert!(bad.is_empty(), "call sites the CLI rejects:\n  {}", bad.join("\n  "));
}

#[test]
fn the_scan_reads_each_call_site_form() {
    assert_eq!(invocations("ways author lint --check"), [("author".into(), Some("lint".into()))]);
    assert_eq!(invocations("\"$WAYS_BIN\" author match \"$1\""), [("author".into(), Some("match".into()))]);
    assert_eq!(invocations("@$(WAYS_BIN) tune language --json"), [("tune".into(), Some("language".into()))]);
    assert_eq!(invocations("x=$(ways session ways --json)"), [("session".into(), Some("ways".into()))]);
    assert_eq!(invocations("> cd ~/.claude && ways author match \"x\""), [("author".into(), Some("match".into()))]);
    assert_eq!(invocations("timeout 10 ways corpus"), [("corpus".into(), None)]);
    assert!(invocations("always lint").is_empty());
    assert!(invocations("echo 'ways binary not found'").is_empty());
    assert!(invocations("how ways match a prompt").is_empty());
    assert!(invocations("echo \"v1; ways ships v2\"").is_empty());
    assert!(invocations("skip \"bin/ways not found\"").is_empty());
    assert_eq!(invocations("\"$APP/bin/ways\" tune language --json"), [("tune".into(), Some("language".into()))]);
    assert!(invocations("ways-agent serve").is_empty());
    assert_eq!(shell_code("ways lint  # ways tree"), "ways lint  ");
    assert_eq!(
        spawns("Command::new(ways_bin).args([\"corpus\", \"--quiet\"])"),
        [("corpus".into(), None)]
    );
}
