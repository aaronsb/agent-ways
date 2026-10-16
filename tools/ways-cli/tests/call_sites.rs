//! Every `ways …` call site in the repository names a command the CLI accepts
//! (ADR-507). A renamed command leaves no caller behind: hooks, skills,
//! commands, scripts, the Makefiles, CI, settings.json, the docs, and the
//! strings and spawns in the Rust sources.
//!
//! A call site is `ways <command>` in code: a shell or Makefile line, a YAML
//! or JSON value, a fenced block or backticked span in markdown, a quoted
//! string or backticked span in Rust, or a spawn of the ways binary. Prose is
//! skipped, since "how ways match" is English. Each distinct command, and each
//! verb of a group, is checked with `ways <command> [<verb>] --help`. The names
//! ADR-509 moved under `ways manage` are matched in messages as well: any
//! `ways <name>` in a shell or Rust line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    // Read at run time: a test binary reused from another checkout keeps the
    // path it was built at.
    std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into())).join("../..").canonicalize().unwrap()
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
        // `"$APP/bin/ways" x`, or `\"$APP/bin/ways\" x` inside an echo.
        let rest = code[end..].trim_start_matches(['\\', '"']);
        let Some(rest) = rest.strip_prefix(' ') else { continue };
        let Some(a) = word(rest) else { continue };
        found.push((a.to_string(), second(&rest[a.len()..])));
    }
    found
}

/// The word after a command: a verb, or `-` for a flag, which a group rejects.
fn second(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix(' ')?;
    if rest.starts_with('-') {
        return Some("-".into());
    }
    word(rest).map(str::to_string)
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
        let t = target.trim();
        if !(t == "exe" || t.contains("ways_bin") || t.contains("\"ways\"") || t.contains("ways_exe")) {
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

/// Each call site, with the places it appears. A place in shell code is
/// marked: a bare group there runs and exits 2, while a backticked `ways
/// <group>` in prose names the group.
type Sites = BTreeMap<(String, Option<String>), Vec<(String, bool)>>;

fn call_sites(root: &Path) -> Sites {
    let mut sites = Sites::new();
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
            let mut shell = false;
            match k {
                Kind::Shell => {
                    shell = true;
                    found.extend(invocations(shell_code(line)));
                }
                Kind::Markdown => {
                    if let Some(lang) = line.trim_start().strip_prefix("```") {
                        fence = match fence {
                            Some(_) => None,
                            None => Some(matches!(lang.trim(), "" | "bash" | "sh" | "shell" | "zsh" | "console")),
                        };
                        continue;
                    }
                    if let Some(code) = fence {
                        if code {
                            shell = true;
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
                    // A string that starts with `ways` is a command the code prints or
                    // runs; an indented one is a help listing ("  ways author lint").
                    for quoted in line.split('"').skip(1).step_by(2) {
                        let q = if quoted.starts_with("  ") { quoted.trim_start() } else { quoted };
                        if q.starts_with("ways ") {
                            found.extend(invocations(q));
                        }
                    }
                }
            }
            if k != Kind::Markdown && !PROMPTS.contains(&path.as_str()) {
                for name in moved_mentions(line) {
                    if !found.iter().any(|(a, _)| a == name) {
                        found.push((name.to_string(), None));
                    }
                }
            }
            for site in found {
                sites.entry(site).or_default().push((format!("{path}:{}", n + 1), shell));
            }
        }
        if k == Kind::Rust {
            for site in spawns(&text) {
                sites.entry(site).or_default().push((format!("{path} (spawn)"), true));
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

/// Groups that open a screen when run bare on a terminal (ADR-507 §7, note
/// of 2026-10-02). Their usage line reads `[COMMAND]`, but in a script, as a
/// call site is, they still need a verb: bare in a pipe they exit 2.
const SCREEN_GROUPS: &[&str] = &["session", "manage"];

/// The lifecycle names ADR-509 moved under `ways manage`. The one-release
/// aliases keep them running, so the CLI still accepts them, but no caller in
/// the repository may use them. The exception is the updater's spawn of
/// `reconcile`: the fixed name every successor binary keeps, because the
/// previous release's updater calls it (ADR-509 §4).
const MOVED: &[&str] = &["update", "reconcile", "uninstall", "target"];
const UPDATER: &str = "tools/ways-cli/src/cmd/update/mod.rs (spawn)";

/// The installer and check-setup run the app's freshly set-up binary, which
/// `make setup` downloads from the latest release, not from the checkout they
/// ship in. They are cross-release callers like the updater, so they run the
/// fixed name `reconcile` (ADR-509 §4). Each entry is a file and the exact
/// invocation it may use there.
const CROSS_RELEASE: &[(&str, &str)] =
    &[("scripts/install.sh", "\"$APP_DIR/bin/ways\" reconcile"), ("hooks/ways/check-setup.sh", "\"$APP_DIR/bin/ways\" reconcile")];

/// Whether a `reconcile` place is one of the CROSS_RELEASE invocations, in
/// code or in an echoed command.
fn cross_release(root: &Path, place: &str) -> bool {
    let Some((path, line)) = place.rsplit_once(':') else { return false };
    let Some((_, form)) = CROSS_RELEASE.iter().find(|(p, _)| *p == path) else { return false };
    let Ok(n) = line.parse::<usize>() else { return false };
    std::fs::read_to_string(root.join(path)).ok().and_then(|t| t.lines().nth(n - 1).map(|l| l.replace("\\\"", "\"").contains(form))).unwrap_or(false)
}

/// Files that quote what a person types rather than call ways: the firing
/// panel's prompts.
const PROMPTS: &[&str] = &["tools/scripts/fire-panel.json"];

/// The MOVED names after `ways ` anywhere in a line, so a message a person
/// reads is held to them too: an `echo "… ways update"`, or a Rust string
/// that does not start with `ways `. `ways manage update` does not match.
fn moved_mentions(line: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for (start, _) in line.match_indices("ways ") {
        let boundary = line[..start].chars().next_back().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        if !boundary {
            continue;
        }
        let rest = &line[start + 5..];
        if let Some(name) = word(rest).and_then(|w| MOVED.iter().find(|m| **m == w)) {
            found.push(*name);
        }
    }
    found
}

#[test]
fn every_ways_call_site_names_a_command_the_cli_accepts() {
    let root = repo_root();
    let sites = call_sites(&root);
    // Positive control: callers the scan must see, or it found nothing to check.
    for must in [("corpus", None), ("author", Some("lint")), ("settings", Some("set")), ("manage", Some("repair"))] {
        let key = (must.0.to_string(), must.1.map(str::to_string));
        assert!(sites.contains_key(&key), "the scan missed `ways {} {}`", must.0, must.1.unwrap_or(""));
    }
    // The updater's spawn is seen, so the exception above excuses a real site.
    let updater = sites.get(&("reconcile".to_string(), None));
    assert!(updater.is_some_and(|p| p.iter().any(|(at, _)| at == UPDATER)), "the scan missed the updater's spawn");

    // Per command: None when the CLI rejects it, else whether it needs a verb
    // (its usage line reads `<COMMAND>`, as `tune` does, or it opens a screen
    // when bare; `settings` runs bare in a pipe too).
    let mut groups: BTreeMap<String, Option<bool>> = BTreeMap::new();
    let mut bad = Vec::new();
    for ((a, b), places) in &sites {
        // Named as removed, in the docs that record their removal and in the
        // tests that keep them rejected.
        if REMOVED.contains(&a.as_str()) {
            continue;
        }
        if MOVED.contains(&a.as_str()) {
            let left: Vec<&str> = places
                .iter()
                .map(|(p, _)| p.as_str())
                .filter(|p| !(a == "reconcile" && (*p == UPDATER || cross_release(&root, p))))
                .collect();
            if !left.is_empty() {
                let shown = left.iter().take(3).copied().collect::<Vec<_>>().join(", ");
                bad.push(format!("ways {a} moved under `ways manage` (ADR-509)  ({shown})"));
            }
            continue;
        }
        // `ways agent …` hands its arguments to the ways-agent binary.
        let group = groups.entry(a.clone()).or_insert_with(|| {
            if a == "agent" {
                return Some(false);
            }
            let (ok, out) = ways_help(&[a]);
            ok.then(|| SCREEN_GROUPS.contains(&a.as_str()) || out.lines().any(|l| l.starts_with("Usage:") && l.contains("<COMMAND>")))
        });
        let ok = match (*group, b) {
            (None, _) => false,
            (Some(true), Some(b)) => b != "-" && ways_help(&[a, b]).0,
            (Some(true), None) => !places.iter().any(|(_, shell)| *shell),
            (Some(false), _) => true,
        };
        if !ok {
            let shown = places.iter().take(3).map(|(p, _)| p.clone()).collect::<Vec<_>>().join(", ");
            bad.push(format!("ways {a}{}  ({shown})", b.as_ref().map(|b| format!(" {b}")).unwrap_or_default()));
        }
    }
    assert!(bad.is_empty(), "call sites the CLI rejects:\n  {}", bad.join("\n  "));
}

/// The installer and check-setup repair through the fixed name, which every
/// release's binary accepts. `manage repair` there breaks every install whose
/// downloaded release predates `ways manage`.
#[test]
fn cross_release_callers_run_the_fixed_name() {
    let root = repo_root();
    for (path, want) in [("scripts/install.sh", 3), ("hooks/ways/check-setup.sh", 2)] {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        // An echoed command escapes its quotes: `\"$APP_DIR/bin/ways\" reconcile`.
        let text = text.replace("\\\"", "\"");
        // `status` and the `reconcile --help` probe for --commit read, not repair.
        let runs: Vec<&str> = text.lines().filter(|l| l.contains("\"$APP_DIR/bin/ways\" ") && !l.contains("bin/ways\" status") && !l.contains("reconcile --help")).collect();
        let fixed = runs.iter().filter(|l| l.contains("\"$APP_DIR/bin/ways\" reconcile")).count();
        assert!(runs.iter().all(|l| !l.contains("bin/ways\" manage")), "{path} runs `manage` on a release binary:\n{}", runs.join("\n"));
        assert_eq!(fixed, want, "{path}: the repair invocations that run `reconcile`:\n{}", runs.join("\n"));
    }
    // The scan sees them, so the exemption excuses real sites.
    let sites = call_sites(&root);
    let places = sites.get(&("reconcile".to_string(), None)).cloned().unwrap_or_default();
    for (path, _) in CROSS_RELEASE {
        assert!(places.iter().any(|(p, _)| p.starts_with(&format!("{path}:")) && cross_release(&root, p)), "the scan missed {path}");
    }
}

#[test]
fn the_scan_reads_each_call_site_form() {
    assert_eq!(invocations("ways author lint --check"), [("author".into(), Some("lint".into()))]);
    assert_eq!(invocations("\"$WAYS_BIN\" author match \"$1\""), [("author".into(), Some("match".into()))]);
    assert_eq!(invocations("@$(WAYS_BIN) tune language --json"), [("tune".into(), Some("language".into()))]);
    assert_eq!(invocations("x=$(ways session ways --json)"), [("session".into(), Some("ways".into()))]);
    assert_eq!(invocations("> cd ~/.claude && ways author match \"x\""), [("author".into(), Some("match".into()))]);
    assert_eq!(invocations("timeout 10 ways corpus"), [("corpus".into(), None)]);
    assert_eq!(invocations("ways tune --lang es"), [("tune".into(), Some("-".into()))]);
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
    assert_eq!(spawns("Command::new(exe).args([\"manage\", \"target\", \"plan\"])"), [("manage".into(), Some("target".into()))]);
    assert_eq!(spawns("Command::new(ways_bin).arg(\"reconcile\")"), [("reconcile".into(), None)]);
    // An escaped quote closing a path in an `echo -e` message.
    assert_eq!(
        invocations(r#"echo -e "    ${CYAN}\"$APP_DIR/bin/ways\" reconcile${RESET}""#),
        [("reconcile".into(), None)]
    );
}

#[test]
fn the_scan_reads_moved_names_in_messages() {
    // Positive controls: messages a person reads, which `invocations` skips.
    assert_eq!(moved_mentions(r#"echo "  then run ways update to refresh""#), ["update"]);
    assert_eq!(moved_mentions(r#"bail!("run `ways reconcile --force` or ways uninstall")"#), ["reconcile", "uninstall"]);
    assert_eq!(moved_mentions(r#"eprintln!("see ways target list")"#), ["target"]);
    // The new forms, other words, and the skill's name are not moved names.
    assert!(moved_mentions("ways manage update && ways manage target list").is_empty());
    assert!(moved_mentions("always update the cache; ways-update skill; ways updated").is_empty());
}

// --- Applying hints carry --commit (ADR-511) --------------------------------
//
// Every mutating `ways manage` verb reports until `--commit`. A message the
// code prints that tells the operator to run one of them to get something
// done ("run `ways manage repair` once it is") has to say `--commit`, or
// the operator follows it and nothing changes. Code is read as logical
// lines: a Rust or TypeScript string continued with a trailing `\` joins its
// next line. The docs, skills, ways and the mod's markdown are held to a
// stricter rule: every bare mutating verb there is a finding unless its form
// says it is a description, or it is on the allowlist below.

/// The verbs that change the install.
const APPLYING: &[&str] = &["update", "repair", "uninstall", "reinstall", "target add", "target enable", "target disable", "target remove"];

/// Words that, just before a command, make it an instruction to run it.
const CUES: &[&str] = &["run ", "Run ", "run: ", "re-run ", "retry ", "then ", "finish", "To ", "try ", "Try ", "use ", "Use "];

/// Flags that make a mention explicit about applying or reporting.
const EXPLICIT: &[&str] = &["--commit", "--yes", "--what-if", "--dry-run"];

/// Each `ways manage <verb>` on `line` without an explicit flag, with the
/// text before it and the rest of the line after its span.
fn bare_mentions(line: &str) -> Vec<(&str, String, &str)> {
    let mut out = Vec::new();
    for (at, m) in line.match_indices("ways manage ") {
        let rest = &line[at + m.len()..];
        let Some(verb) = APPLYING.iter().find(|v| rest.starts_with(**v) && !rest[v.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-')) else {
            continue;
        };
        let end = rest.find(['`', '"', '\'', ')', ';', '#']).unwrap_or(rest.len());
        let span = &rest[..end];
        if EXPLICIT.iter().any(|f| span.contains(f)) {
            continue;
        }
        debug_assert!(span.starts_with(verb));
        out.push((&line[..at], format!("ways manage {}", span.trim_end().trim_end_matches(['.', ','])), &rest[end..]));
    }
    out
}

/// Each applying hint on a line of code: a bare mention after a cue word,
/// or one the code queues or names as the repair of a settings section.
fn hints_missing_commit(line: &str) -> Vec<String> {
    bare_mentions(line)
        .into_iter()
        .filter(|(before, _, _)| {
            let window: String = before.chars().rev().take(24).collect::<Vec<_>>().into_iter().rev().collect();
            CUES.iter().any(|c| window.contains(c)) || ["Action::new(", "Out::new(", "repair: Some("].iter().any(|c| before.contains(c))
        })
        .map(|(_, hint, _)| hint)
        .collect()
}

/// A line's logical form: a string continued with a trailing `\` takes the
/// next line, its indent dropped.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (n, line) in text.lines().enumerate() {
        let piece = match &open {
            Some(_) => line.trim_start(),
            None => line,
        };
        let (start, mut acc) = open.take().unwrap_or((n, String::new()));
        match piece.strip_suffix('\\') {
            Some(head) => {
                acc.push_str(head);
                open = Some((start, acc));
            }
            None => {
                acc.push_str(piece);
                out.push((start, acc));
            }
        }
    }
    out.extend(open);
    out
}

#[test]
fn an_applying_hint_is_found_and_a_commit_or_a_description_is_not() {
    assert_eq!(hints_missing_commit("re-run `ways manage repair` once it is"), ["ways manage repair"]);
    assert_eq!(hints_missing_commit("echo \"  then run: ways manage repair\""), ["ways manage repair"]);
    assert_eq!(hints_missing_commit("(install a toolchain, then `ways manage update`)"), ["ways manage update"]);
    assert_eq!(hints_missing_commit("To finish, `ways manage repair`"), ["ways manage repair"]);
    assert_eq!(hints_missing_commit("Action::new(\"add\", \"ways manage target add {}\")"), ["ways manage target add {}"]);
    assert!(hints_missing_commit("re-run `ways manage repair --commit` once it is").is_empty());
    assert!(hints_missing_commit("Run `ways manage uninstall --yes` to do this").is_empty());
    assert!(hints_missing_commit("`ways manage repair` failed — see the output above").is_empty(), "a description");
    assert!(hints_missing_commit("Report::new(\"ways manage update: what-if\")").is_empty(), "a title");
    assert!(hints_missing_commit("run `ways manage target list` to see them").is_empty(), "a verb that changes nothing");
    // A wrapped Rust string is read as one line.
    let wrapped = "    bail!(\"…or run \\\n             `ways manage repair --force` to rename\")";
    let joined = logical_lines(wrapped);
    assert_eq!(joined.len(), 1, "{joined:?}");
    assert_eq!(hints_missing_commit(&joined[0].1), ["ways manage repair --force"]);
}

/// Lines of code that name a bare verb on purpose, with the reason.
const CODE_ALLOW: &[(&str, &str)] = &[
    // The hook test expects the bare form a ways from before --commit gets.
    ("tests/hooks-test.sh", "Run `ways manage update` to install it."),
];

/// Whether `path` is code the hint scan reads.
fn hint_code(path: &str) -> bool {
    let code_root = in_scope(path) || path == "Makefile" || (path.starts_with("plugins/") && !path.contains("/node_modules/"));
    let code_kind = path.ends_with(".rs") || path.ends_with(".ts") || path.ends_with(".sh") || path.ends_with("/Makefile") || path == "Makefile" || (path.starts_with("hooks/") && !path.rsplit('/').next().unwrap_or("").contains('.'));
    code_root && code_kind && !path.ends_with("tests/call_sites.rs")
}

/// Every applying hint in the code under `root`, and how many files were read.
fn code_hints(root: &Path) -> (Vec<String>, usize) {
    let mut bad = Vec::new();
    let mut scanned = 0;
    for path in tracked_files(root).into_iter().filter(|p| hint_code(p)) {
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else { continue };
        scanned += 1;
        let slashes = path.ends_with(".rs") || path.ends_with(".ts");
        for (n, line) in logical_lines(&text) {
            let code = if slashes {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                line.as_str()
            } else {
                shell_code(&line)
            };
            if CODE_ALLOW.iter().any(|(p, needle)| *p == path && code.contains(needle)) {
                continue;
            }
            for hint in hints_missing_commit(code) {
                bad.push(format!("  {path}:{}: `{hint}`", n + 1));
            }
        }
    }
    (bad, scanned)
}

#[test]
fn every_applying_hint_in_code_says_commit() {
    let (bad, scanned) = code_hints(&repo_root());
    assert!(scanned > 100, "the scan read {scanned} files; it found nothing to check");
    assert!(bad.is_empty(), "applying hints without --commit (ADR-511):\n{}", bad.join("\n"));
}

/// Words that, right after a bare mention, make it a description of what
/// the verb does rather than an instruction to run it.
const DESCRIBES: &[&str] = &[
    "runs", "reports", "replaces", "records", "converges", "installs", "stops", "pulls", "needs", "does", "is", "projects",
    "refreshes", "re-downloads", "fixes", "prints", "lists", "honours", "points", "would", "puts", "reverses", "relinks",
    "registers", "fetches", "and", "or", "/", "'s", "three-way", "alone", "run", ", which"
];

/// Words that, right before a bare mention, make it a description.
const DESCRIBED_AFTER: &[&str] = &["block", "Whether", "whether", "the next", "via", "after", "as", "by", "of", "Debugging", "same code as", "where", "end of", "with", "(", "pattern (agent-ways,"];

/// Mentions in markdown that read as instructions by form but are
/// descriptions or deliberate what-ifs, with the reason each is kept.
const MD_ALLOW: &[(&str, &str)] = &[
    // The update's stage list names the repair it runs.
    ("docs/reference/ways-cli.md", "rebuild the corpus, run `ways manage repair`"),
    // A Mermaid edge label in the extending diagram.
    ("docs/hooks-and-ways/extending.md", "ways manage update replaces it"),
    // The way's vocabulary line lists words, not commands.
    ("hooks/ways/meta/deployment/deployment.md", "$XDG_DATA_HOME/agent-ways ways manage repair reinstall"),
    // The way's description names what the installer runs.
    ("hooks/ways/meta/deployment/deployment.md", "installer/update/`ways manage repair` work under it"),
];

/// Whether a bare mention in markdown reads as a description, by its form:
/// a heading, a fenced command whose comment says it only reports, a verb
/// or connective right after it, or a describing word right before it.
fn md_describes(line: &str, before: &str, after: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with('#') {
        return true;
    }
    let comment = line.split_once(" # ").map(|(_, c)| c.to_lowercase()).unwrap_or_default();
    if ["what it would", "the plan", "report", "nothing", "preview"].iter().any(|w| comment.contains(w)) {
        return true;
    }
    let next = after.trim_start_matches(['`', '"', ')']).trim_start();
    if DESCRIBES.iter().any(|w| next.starts_with(&format!("{w} ")) || next.starts_with(&format!("{w},")) || next == *w || (*w == "'s" && next.starts_with("'s"))) {
        return true;
    }
    let prev = before.trim_end_matches(['`', '"']).trim_end();
    DESCRIBED_AFTER.iter().any(|w| prev.ends_with(w))
}

/// Every bare mutating verb in the markdown under `root` that is neither
/// described by its form nor allowlisted, and how many files were read.
fn md_hints(root: &Path) -> (Vec<String>, usize) {
    let mut bad = Vec::new();
    let mut scanned = 0;
    for path in tracked_files(root) {
        let md = path.ends_with(".md");
        let roots = ["docs/", "skills/", "hooks/", "plugins/"].iter().any(|r| path.starts_with(r)) || path == "README.md" || path == ".claude/CLAUDE.md";
        let history = path.starts_with("docs/architecture/") || path.starts_with("docs/research/") || path.contains("CHANGELOG") || path.contains("/node_modules/");
        if !md || !roots || history {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else { continue };
        scanned += 1;
        for (n, line) in text.lines().enumerate() {
            for (before, hint, after) in bare_mentions(line) {
                if md_describes(line, before, after) || MD_ALLOW.iter().any(|(p, needle)| *p == path && line.contains(needle)) {
                    continue;
                }
                bad.push(format!("  {path}:{}: `{hint}`", n + 1));
            }
        }
    }
    (bad, scanned)
}

#[test]
fn every_bare_mutating_verb_in_the_docs_is_a_description() {
    let (bad, scanned) = md_hints(&repo_root());
    assert!(scanned > 50, "the scan read {scanned} files; it found nothing to check");
    assert!(bad.is_empty(), "bare mutating verbs in docs that read as instructions (add --commit or --what-if, or allowlist a description in MD_ALLOW):\n{}", bad.join("\n"));
}

#[test]
fn a_markdown_instruction_is_found_and_a_description_is_not() {
    let check = |line: &str| bare_mentions(line).into_iter().filter(|(b, _, a)| !md_describes(line, b, a)).map(|(_, h, _)| h).collect::<Vec<_>>();
    assert_eq!(check("Then run `ways manage update` to finish."), ["ways manage update"]);
    assert_eq!(check("ways manage repair"), ["ways manage repair"], "a bare fenced command");
    assert!(check("ways manage repair                       # what it would do").is_empty());
    assert!(check("`ways manage repair` converges every enabled target").is_empty());
    assert!(check("### `ways manage update`").is_empty());
    assert!(check("Whether `ways manage repair` installs the mod").is_empty());
}
