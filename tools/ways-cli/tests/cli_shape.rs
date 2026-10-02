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
    for name in ["hook", "show", "scan", "manifest", "project-slug", "sessions-root", "events-log-path"] {
        assert!(!listed(&help).iter().any(|n| n == name), "{name} is listed");
        assert!(ways(&[name, "--help"]).status.success(), "{name} --help fails");
    }
    assert!(ways(&["sessions-root"]).status.success());
}

#[test]
fn a_group_with_no_verb_prints_its_help_and_exits_2() {
    for group in ["target", "session", "author", "tune"] {
        let out = ways(&[group]);
        assert_eq!(out.status.code(), Some(2), "{group}");
        let text = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
        assert!(text.contains(&format!("Usage: ways {group} <COMMAND>")), "{group}: {text}");
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

#[test]
fn removed_commands_are_unknown() {
    for args in [&["config", "show"][..], &["disable", "x"], &["enable", "x"], &["list"], &["introspect", "list"], &["lint"]] {
        let out = ways(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}
