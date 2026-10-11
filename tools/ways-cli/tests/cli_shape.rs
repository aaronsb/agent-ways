//! The shape of the command line ADR-507 decides, with ADR-509's `manage`
//! group: the commands `ways --help` lists and their order, the hidden
//! plumbing, the one-release aliases, the exit code of a group run with no
//! verb, and the banner only for a bare `ways` on a terminal.

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
fn help_lists_the_eleven_commands_in_order() {
    let help = stdout(&ways(&["--help"]));
    let want = [
        "status", "settings", "agent", "projects", "session", "context", "author", "tune", "init", "corpus", "manage",
    ];
    assert_eq!(listed(&help), want);
    for line in help.lines() {
        assert!(line.chars().count() <= 80, "wider than 80 columns: {line:?}");
    }
}

#[test]
fn plumbing_is_hidden_and_still_runs() {
    let help = stdout(&ways(&["--help"]));
    for name in [
        "hook", "show", "scan", "lookup", "manifest", "project-slug", "sessions-root", "events-log-path", "reconcile", "update",
        "uninstall", "target",
    ] {
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

/// A bare `ways session`, `ways manage` or `ways manage target` opens its
/// screen on a terminal; in a pipe, as here, it prints its help and exits 2
/// like a group that needs a verb (ADR-507 §7, notes of 2026-10-02).
#[test]
fn a_bare_screen_group_in_a_pipe_prints_its_help_and_exits_2() {
    for (group, verb) in [(&["session"][..], "replay"), (&["manage"], "reinstall"), (&["manage", "target"], "plan")] {
        let out = ways(group);
        let name = group.join(" ");
        assert_eq!(out.status.code(), Some(2), "{name}");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains(&format!(" {name} [COMMAND]")) && text.contains(verb), "{name}: {text}");
        assert!(stdout(&out).is_empty(), "{name}: help goes to stderr, as a usage error's does");
    }
}

/// `ways` with no app installed: data, config, state and cache in a fresh
/// directory.
fn ways_uninstalled(name: &str, args: &[&str]) -> Output {
    let dir = std::env::temp_dir().join(format!("ways-cli-shape-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ways"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("HOME", dir.join("home"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .output()
        .expect("run ways");
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// The one-release aliases of ADR-509 §5, removed by issue #911: each names
/// the new form on stderr, then runs it with the same arguments and exit
/// code. `reconcile` is a fixed name, not an alias, and prints no notice.
#[test]
fn the_old_lifecycle_names_print_the_new_form_and_forward_the_exit_code() {
    for (old, args) in [("update", &["--dry-run"][..]), ("uninstall", &[]), ("target", &["list", "--json"])] {
        let aliased = ways_uninstalled(old, &[&[old][..], args].concat());
        let new = ways_uninstalled(old, &[&["manage", old][..], args].concat());
        let stderr = String::from_utf8_lossy(&aliased.stderr);
        assert_eq!(stderr.lines().next(), Some(format!("note: `ways {old}` is now `ways manage {old}`").as_str()), "{old}");
        assert_eq!(aliased.status.code(), new.status.code(), "{old}: {stderr}");
        assert_eq!(aliased.stdout, new.stdout, "{old}");
        assert!(!String::from_utf8_lossy(&new.stderr).contains("note:"), "{old}: the new form carries no notice");
    }
    // A failing run forwards its failure: no app source to update.
    let failed = ways_uninstalled("alias-fail", &["update"]);
    assert_eq!(failed.status.code(), Some(1), "{}", String::from_utf8_lossy(&failed.stderr));

    let fixed = ways_uninstalled("reconcile", &["reconcile", "--dry-run"]);
    let repair = ways_uninstalled("reconcile", &["manage", "repair", "--dry-run"]);
    assert_eq!(fixed.status.code(), repair.status.code());
    assert_eq!(fixed.stdout, repair.stdout);
    assert!(!String::from_utf8_lossy(&fixed.stderr).contains("note:"), "reconcile prints no notice");
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
