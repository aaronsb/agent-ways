//! Output that is not going to a terminal carries no escape sequences.
//!
//! The `meta/start` macro runs `ways context` and injects what it prints into
//! the model's context, so a pipe must get plain text even when the
//! environment names a colour terminal. `CLICOLOR_FORCE` and `FORCE_COLOR`
//! put the colour back for a caller that wants it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(tag: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("ways-piped-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    // Claude Code names a project's transcript directory by mapping every
    // character outside [A-Za-z0-9] to `-`.
    let slug: String = project.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let dir = home.join(".claude/projects").join(slug);
    std::fs::create_dir_all(&dir).unwrap();
    let line = r#"{"type":"assistant","message":{"model":"claude-opus-5-5","usage":{"input_tokens":394000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
    std::fs::write(dir.join("piped-0001.jsonl"), format!("{line}\n")).unwrap();
    (home, project)
}

/// `ways context` for the fixture project, stdout piped, under a terminal
/// environment that names truecolor.
fn context(home: &Path, project: &Path, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    cmd.arg("context")
        .arg("--project")
        .arg(project)
        .current_dir(project)
        .env("HOME", home)
        // home_dir() prefers USERPROFILE on Windows, so set both or the
        // binary looks for transcripts under the runner's real profile.
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor");
    for var in ["NO_COLOR", "CLICOLOR_FORCE", "FORCE_COLOR", "CLAUDE_PROJECT_DIR", "CLAUDE_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
        cmd.env_remove(var);
    }
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("ways runs");
    assert!(out.status.success(), "ways context failed: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

#[test]
fn piped_context_carries_no_escape_sequences() {
    let (home, project) = fixture("plain");
    let out = context(&home, &project, &[]);
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("tokens used"), "the gauge rendered: {text:?}");
    assert!(!out.contains(&0x1b), "piped output carries ESC: {text:?}");
}

#[test]
fn force_colour_restores_escapes_on_a_pipe() {
    let (home, project) = fixture("forced");
    for var in ["CLICOLOR_FORCE", "FORCE_COLOR"] {
        let out = context(&home, &project, &[(var, "1")]);
        assert!(out.contains(&0x1b), "{var}=1 should colour a pipe: {:?}", String::from_utf8_lossy(&out));
    }
    let out = context(&home, &project, &[("FORCE_COLOR", "0")]);
    assert!(!out.contains(&0x1b), "FORCE_COLOR=0 does not force");
}
