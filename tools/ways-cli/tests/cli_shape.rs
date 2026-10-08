//! The shape of the command line ADR-507 decides: the commands `ways --help`
//! lists and their order, the hidden plumbing, the exit code of a group run
//! with no verb, and the banner only for a bare `ways` on a terminal.

use std::process::{Command, Output};

fn ways(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ways")).args(args).env("NO_COLOR", "1").output().expect("run ways")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The command names in a help page's `Commands:` block, in order.
fn listed(help: &str) -> Vec<String> {
    help.lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .filter(|name| name != "help")
        .collect()
}

#[test]
fn help_lists_the_fourteen_commands_in_order() {
    let help = stdout(&ways(&["--help"]));
    let want = [
        "status", "settings", "target", "agent", "projects", "session", "context", "author", "tune", "init", "corpus",
        "update", "reconcile", "uninstall",
    ];
    assert_eq!(listed(&help), want);
    for line in help.lines() {
        assert!(line.chars().count() <= 80, "wider than 80 columns: {line:?}");
    }
}

#[test]
fn plumbing_is_hidden_and_still_runs() {
    let help = stdout(&ways(&["--help"]));
    for name in ["hook", "show", "scan", "lookup", "manifest", "project-slug", "sessions-root", "events-log-path"] {
        assert!(!listed(&help).iter().any(|n| n == name), "{name} is listed");
        assert!(ways(&[name, "--help"]).status.success(), "{name} --help fails");
    }
    assert!(ways(&["sessions-root"]).status.success());
}

#[test]
fn a_group_with_no_verb_prints_its_help_and_exits_2() {
    for group in ["author", "tune"] {
        let out = ways(&[group]);
        assert_eq!(out.status.code(), Some(2), "{group}");
        let text = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
        // The binary is `ways.exe` on Windows.
        assert!(text.contains(&format!(" {group} <COMMAND>")), "{group}: {text}");
    }
}

/// A bare `ways session` or `ways target` opens its screen on a terminal; in
/// a pipe, as here, it prints its help and exits 2 like a group that needs a
/// verb (ADR-507 §7, notes of 2026-10-02).
#[test]
fn a_bare_screen_group_in_a_pipe_prints_its_help_and_exits_2() {
    for (group, verb) in [("session", "replay"), ("target", "plan")] {
        let out = ways(&[group]);
        assert_eq!(out.status.code(), Some(2), "{group}");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains(&format!(" {group} [COMMAND]")) && text.contains(verb), "{group}: {text}");
        assert!(stdout(&out).is_empty(), "{group}: help goes to stderr, as a usage error's does");
    }
}

#[test]
fn a_bare_ways_in_a_pipe_prints_help_without_the_banner() {
    let out = ways(&[]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("Unified CLI for ways"), "{text}");
    assert_eq!(listed(&text), listed(&stdout(&ways(&["--help"]))));
}

/// `ways` with its config and state in a fresh directory and no key in the environment.
fn ways_isolated(name: &str, agent_yaml: Option<&str>, args: &[&str]) -> String {
    let dir = std::env::temp_dir().join(format!("ways-cli-shape-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config/agent-ways")).unwrap();
    if let Some(text) = agent_yaml {
        std::fs::write(dir.join("config/agent-ways/agent.yaml"), text).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_ways"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENROUTER_API_KEY")
        .output()
        .expect("run ways");
    let _ = std::fs::remove_dir_all(&dir);
    stdout(&out)
}

#[test]
fn help_warns_when_the_judge_cannot_gate_and_only_there() {
    for args in [&[][..], &["--help"], &["help"]] {
        let help = ways_isolated("nokey", None, args);
        assert!(help.contains("Ways is degraded") && help.contains("key add"), "{args:?}: {help}");
        assert!(help.lines().all(|l| l.chars().count() <= 80), "{help}");
    }
    let off = ways_isolated("off", Some("mode: off\n"), &["--help"]);
    assert!(!off.contains("degraded"), "gate.mode off is the operator's choice: {off}");
    for args in [&["help", "status"][..], &["status", "--help"], &["scan", "--help"], &["sessions-root"]] {
        assert!(!ways_isolated("other", None, args).contains("degraded"), "{args:?}");
    }
}

#[test]
fn removed_commands_are_unknown() {
    for args in [&["config", "show"][..], &["disable", "x"], &["enable", "x"], &["list"], &["introspect", "list"], &["lint"]] {
        let out = ways(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}
