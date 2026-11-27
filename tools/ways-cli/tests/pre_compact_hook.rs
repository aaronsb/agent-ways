// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `ways hook pre-compact` as Claude Code runs it (ADR-417 item 3): the
//! payload on stdin, the instructions on stdout, and exit 0 whatever the
//! payload holds. The attend it asks for the wrap id is the one on PATH.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const ID: &str = "20261126T2140Z-3f9a1c2e";
const LEAD: &str = "Summarize this session as the handoff below; keep its TASKS and DO NEXT whole.";
const OUTLINE_START: &str = "Summarize this session as a continuation prompt a stranger could resume from";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let d = std::env::temp_dir().join(format!("ways-pre-compact-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Dir(d)
}

/// A git repository under `dir` with one commit.
fn repo(dir: &Path) -> PathBuf {
    let root = dir.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    for args in [&["init", "-q"][..], &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]] {
        assert!(Command::new("git").arg("-C").arg(&root).args(args).status().unwrap().success());
    }
    root
}

/// A `bin` dir under `dir` holding an attend that prints `id` for
/// `keepwarm wrap-id`, to put first on PATH.
fn attend_bin(dir: &Path, id: &str) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let attend = bin.join("attend");
    std::fs::write(&attend, format!("#!/bin/sh\n[ \"$1 $2\" = \"keepwarm wrap-id\" ] || exit 2\necho {id}\n")).unwrap();
    std::fs::set_permissions(&attend, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn hook(stdin: Option<&str>, path_first: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    cmd.args(["hook", "pre-compact"]).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = path_first {
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}:{path}", dir.display()));
    }
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn().expect("run ways");
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn payload(cwd: &Path, custom: &str) -> String {
    serde_json::json!({ "session_id": "s", "trigger": "manual", "custom_instructions": custom, "cwd": cwd }).to_string()
}

#[test]
fn a_fresh_handoff_is_printed_as_instructions() {
    let d = scratch("fresh");
    let root = repo(&d.0);
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude/handoff.wrap.md"), format!("2026-01-01 abcdef12 wrap={ID}\nTASKS — one.\n")).unwrap();
    let bin = attend_bin(&d.0, ID);
    let out = hook(Some(&payload(&root, "")), Some(&bin));
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with(&format!("{LEAD}\n\n2026-01-01 abcdef12 wrap={ID}")), "{text}");
}

#[test]
fn a_compaction_with_instructions_gets_nothing() {
    let d = scratch("custom");
    let root = repo(&d.0);
    let out = hook(Some(&payload(&root, "focus on the parser")), None);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn bad_or_absent_input_exits_zero_with_the_outline() {
    for input in [Some("{not json"), Some(""), None] {
        let out = hook(input, None);
        assert!(out.status.success(), "{input:?}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stdout).starts_with(OUTLINE_START), "{input:?}");
    }
}
