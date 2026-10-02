//! Removed legacy CLI surface stays removed (#692, ADR-505).
//!
//! `ways embed` was an alias for the top-level match command (now `ways
//! author match`), and its `--cosine` flag showed a single-vector view that no
//! longer reflects the fire path. Both are gone;
//! clap must reject them rather than accept and ignore them.

use std::process::Command;

fn ways(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ways"))
        .args(args)
        .output()
        .expect("run ways")
}

#[test]
fn embed_subcommand_is_rejected() {
    let out = ways(&["embed", "some query"]);
    assert!(!out.status.success(), "`ways embed` must be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unrecognized subcommand"), "stderr: {err}");
}

/// `ways rethink`, with its `--list` and `--json`, was a deprecated alias of
/// `ways session replay`, `list` and `dump`. It is gone, with no alias
/// kept (ADR-504 §13, ADR-506): each form is an unknown command.
#[test]
fn rethink_is_an_unknown_command() {
    for args in [&["rethink"][..], &["rethink", "--list"], &["rethink", "--json"], &["rethink", "--list", "--json"]] {
        let out = ways(args);
        assert!(!out.status.success(), "`ways {}` must be rejected", args.join(" "));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("unrecognized subcommand 'rethink'"), "ways {}: {err}", args.join(" "));
    }
    let help = String::from_utf8_lossy(&ways(&["--help"]).stdout).to_string();
    assert!(!help.contains("rethink"), "--help names no rethink:\n{help}");
}

#[test]
fn match_cosine_flag_is_rejected() {
    let out = ways(&["author", "match", "--cosine", "some query"]);
    assert!(!out.status.success(), "`ways author match --cosine` must be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected argument"), "stderr: {err}");
}

#[test]
fn match_corpus_flag_is_rejected() {
    // `--corpus` only fed the removed single-vector view.
    let out = ways(&["author", "match", "--corpus", "/nonexistent.jsonl", "some query"]);
    assert!(!out.status.success(), "`ways author match --corpus` must be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected argument"), "stderr: {err}");
}

/// The pre-1.0 events log (`~/.claude/stats/events.jsonl`) and cache dir
/// (`$XDG_CACHE_HOME/claude-ways`) are no longer read (ADR-506): the events log
/// and the model path resolve under the fixture's XDG dirs even when the old
/// locations exist.
#[test]
fn old_events_log_and_cache_are_not_read() {
    let home = std::env::temp_dir().join(format!("ways-legacy-paths-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".claude/stats")).unwrap();
    std::fs::create_dir_all(home.join(".cache/claude-ways/user")).unwrap();
    std::fs::write(home.join(".claude/stats/events.jsonl"), "{\"event\":\"x\"}\n").unwrap();
    std::fs::write(home.join(".cache/claude-ways/user/minilm-l6-v2.gguf"), "model").unwrap();

    let run = |args: &[&str]| -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(args)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("CLAUDE_PROJECT_DIR", home.join("proj"))
            .output()
            .expect("run ways");
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let log = run(&["events-log-path"]);
    // Compare as paths: Windows prints backslashes where join("a/b") keeps a slash.
    let want = home.join(".local").join("state").join("agent-ways").join("events.jsonl");
    assert_eq!(std::path::PathBuf::from(log.trim()), want, "events log path");
    let status = run(&["status"]);
    let model_line = status.lines().find(|l| l.starts_with("Model:")).expect("a Model: line");
    let model_line = model_line.replace('\\', "/");
    assert!(model_line.contains(".cache/agent-ways/user/"), "{model_line}");
    assert!(model_line.contains("MISSING"), "the old dir's model must not count: {model_line}");

    // Control: a model at the current path is found, so the probe can see one.
    std::fs::create_dir_all(home.join(".cache/agent-ways/user")).unwrap();
    std::fs::write(home.join(".cache/agent-ways/user/minilm-l6-v2.gguf"), "model").unwrap();
    let status = run(&["status"]);
    let model_line = status.lines().find(|l| l.starts_with("Model:")).expect("a Model: line");
    assert!(!model_line.contains("MISSING"), "{model_line}");
    let _ = std::fs::remove_dir_all(&home);
}

/// The pre-1.0 config layers (`~/.claude/ways.json`, `$XDG_CONFIG_HOME/ways/config.yaml`)
/// are no longer read (ADR-506); only `$XDG_CONFIG_HOME/agent-ways/config.yaml` is.
#[test]
fn old_config_layers_are_not_read() {
    let home = std::env::temp_dir().join(format!("ways-legacy-config-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::create_dir_all(home.join(".config/ways")).unwrap();
    std::fs::write(home.join(".claude/ways.json"), r#"{"disabled":["itops"],"output_language":"de"}"#).unwrap();
    std::fs::write(home.join(".config/ways/config.yaml"), "language: fr\ndisabled_domains: [ea]\n").unwrap();

    let get = |home: &std::path::Path, key: &str| -> serde_json::Value {
        let out = Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(["settings", "get", key, "--json"])
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("CLAUDE_PROJECT_DIR", home.join("proj"))
            .output()
            .expect("run ways");
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("json")["value"].clone()
    };
    let show = |home: &std::path::Path| {
        serde_json::json!({ "language": get(home, "ways.language"), "disabled_domains": get(home, "ways.disabled_domains") })
    };

    let before = show(&home);
    assert_eq!(before["disabled_domains"], serde_json::json!([]), "{before}");
    assert_ne!(before["language"], "de", "{before}");
    assert_ne!(before["language"], "fr", "{before}");

    // Control: the current path is read, so the probe can see a config.
    std::fs::create_dir_all(home.join(".config/agent-ways")).unwrap();
    std::fs::write(home.join(".config/agent-ways/config.yaml"), "language: ja\ndisabled_domains: [itops]\n").unwrap();
    let after = show(&home);
    assert_eq!(after["language"], "ja", "{after}");
    assert_eq!(after["disabled_domains"], serde_json::json!(["itops"]), "{after}");
    let _ = std::fs::remove_dir_all(&home);
}
