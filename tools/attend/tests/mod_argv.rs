//! Every attend argv the mod and the hook scripts build parses against this
//! binary (ADR-413 item 6.2). The argv is read from its source: the
//! `[ATTEND, ...]` literals and `argv.push('--flag', ...)` calls in
//! plugins/attend/hooks/lib.ts, and the `$(attend ...)` and `sl__run attend
//! ...` calls in the scripts. A renamed verb or flag fails here before a
//! session's tools or hooks meet it.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into()))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The single-quoted strings in `s`, in order.
fn quoted(s: &str) -> Vec<String> {
    s.split('\'').skip(1).step_by(2).map(str::to_string).collect()
}

/// One call: the command path and the flags it passes.
#[derive(Debug)]
struct Call {
    origin: String,
    path: Vec<String>,
    flags: Vec<String>,
}

/// lib.ts's calls. Each `export` block is one builder: its `[ATTEND, ...]`
/// literals name the path, and every `--flag` in the block, in a literal or a
/// push, is a flag of that path. A literal's leading words are the path;
/// words after the first flag are values.
fn mod_calls() -> Vec<Call> {
    let src = read("plugins/attend/hooks/lib.ts");
    let mut calls = Vec::new();
    for block in src.split("\nexport ") {
        // Comments may hold apostrophes, which would shift the quoting.
        let block: String = block
            .lines()
            .filter(|l| !["*", "/**", "//"].iter().any(|c| l.trim_start().starts_with(c)))
            .collect::<Vec<_>>()
            .join("\n");
        let block = block.as_str();
        let flags: Vec<String> = quoted(block).into_iter().filter(|t| t.starts_with("--") && t.len() > 2).collect();
        for (at, _) in block.match_indices("[ATTEND,") {
            let lit = &block[at..at + block[at..].find(']').expect("a closed literal")];
            let path: Vec<String> = quoted(lit).into_iter().take_while(|t| !t.starts_with('-')).collect();
            let name = block.split(|c: char| !(c.is_alphanumeric() || c == '_')).find(|w| w.len() > 1 && *w != "function" && *w != "const").unwrap_or("?");
            calls.push(Call { origin: format!("lib.ts {name}"), path, flags: flags.clone() });
        }
    }
    calls
}

/// The scripts' calls: `attend` after `$(` or `sl__run `, up to the end of
/// the command. Every word is passed as written.
fn script_calls() -> Vec<Call> {
    let mut calls = Vec::new();
    for rel in ["hooks/ways/attend-drain-stop.sh", "hooks/ways/softwaredev/environment/attend/macro.sh", "statusline.sh"] {
        for line in read(rel).lines().filter(|l| !l.trim_start().starts_with('#')) {
            for prefix in ["$(attend ", "sl__run attend "] {
                for (at, _) in line.match_indices(prefix) {
                    let rest = &line[at + prefix.len()..];
                    let end = rest.find([')', '|', ';', '"', '$']).unwrap_or(rest.len());
                    let words: Vec<String> = rest[..end].split_whitespace().take_while(|w| !w.starts_with("2>") && !w.starts_with('>')).map(str::to_string).collect();
                    let path = words.iter().take_while(|w| !w.starts_with('-')).cloned().collect();
                    calls.push(Call { origin: rel.to_string(), path, flags: words });
                }
            }
        }
    }
    calls
}

fn help(args: &[String]) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_attend")).args(args).arg("--help").output().expect("run attend");
    (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
}

/// Each word of `path` is a subcommand its parent's help lists. A hidden
/// alias parses but is not listed, so a flat verb fails here (ADR-413 item 5).
fn assert_listed(origin: &str, path: &[String]) {
    for (i, word) in path.iter().enumerate() {
        let (_, parent) = help(&path[..i]);
        let listed = parent.lines().any(|l| l.strip_prefix("  ").is_some_and(|r| r.split_whitespace().next() == Some(word.as_str())));
        assert!(listed, "{origin}: `{word}` is not a listed subcommand of `attend {}`:\n{parent}", path[..i].join(" "));
    }
}

#[test]
fn every_argv_the_mod_builds_parses() {
    let calls = mod_calls();
    assert!(calls.len() >= 10, "found only {} [ATTEND, ...] literals in lib.ts: {calls:?}", calls.len());
    for call in &calls {
        let (code, text) = help(&call.path);
        assert_eq!(code, Some(0), "{}: `attend {}` does not parse:\n{text}", call.origin, call.path.join(" "));
        assert_listed(&call.origin, &call.path);
        for flag in &call.flags {
            assert!(text.contains(&format!("{flag} ")) || text.contains(&format!("{flag}\n")), "{}: `attend {}` has no {flag}:\n{text}", call.origin, call.path.join(" "));
        }
    }
}

#[test]
fn every_argv_the_hook_scripts_run_parses() {
    let calls = script_calls();
    for rel in ["attend-drain-stop.sh", "macro.sh", "statusline.sh"] {
        assert!(calls.iter().any(|c| c.origin.ends_with(rel)), "no attend call found in {rel}: {calls:?}");
    }
    for call in &calls {
        let (code, text) = help(&call.flags);
        assert_eq!(code, Some(0), "{}: `attend {}` does not parse:\n{text}", call.origin, call.flags.join(" "));
        assert!(!call.path.is_empty(), "{}: a call with no verb", call.origin);
        assert_listed(&call.origin, &call.path);
    }
}

/// The reader's control: an unknown verb is refused, a hidden alias is not
/// listed, and a flag absent from a verb's help is caught.
#[test]
fn a_flat_verb_or_an_unknown_flag_fails_the_check() {
    let (code, _) = help(&["frobnicate".into()]);
    assert_eq!(code, Some(2));
    let flat = std::panic::catch_unwind(|| assert_listed("control", &["peers".into()]));
    assert!(flat.is_err(), "a hidden alias passed as listed");
    let (code, text) = help(&["peer".into(), "list".into()]);
    assert_eq!(code, Some(0));
    assert!(!text.contains("--wake "), "the flag test would pass anything: {text}");
}
