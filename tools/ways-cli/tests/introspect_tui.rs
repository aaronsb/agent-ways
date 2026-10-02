//! The introspect screens (ADR-504 §9) through the real binary, headless:
//! `--keys` feeds the real key handler and `--snap WxH` prints the frame in
//! the test kit's format. Each test runs `ways` with HOME and every XDG
//! directory in a fixture of its own holding an event log and a transcript.
//! The golden frames of these screens are checked in the unit tests
//! (`cmd::introspect::tests`); these check the wiring: the log is read, the
//! picker opens a session, the transcript is found, and each mode has its
//! CLI form.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

const OLD: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const NEW: &str = "bbbbbbbb-0000-4000-8000-000000000002";

struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ways-introspect-tui-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        for d in [".config", "my_proj/.git", ".local/state/agent-ways", ".cache", ".local/share"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let proj = home.join("my_proj").display().to_string();
        let mut log = String::new();
        for (id, day) in [(OLD, "01"), (NEW, "02")] {
            log += &format!("{{\"ts\":\"2026-07-{day}T10:00:00Z\",\"event\":\"session_start\",\"session\":\"{id}\",\"project\":\"{proj}\"}}\n");
            log += &format!("{{\"ts\":\"2026-07-{day}T10:00:01Z\",\"event\":\"way_fired\",\"session\":\"{id}\",\"way\":\"softwaredev/code/testing\",\"trigger\":\"keyword\"}}\n");
            log += &format!("{{\"ts\":\"2026-07-{day}T10:01:00Z\",\"event\":\"way_fired\",\"session\":\"{id}\",\"way\":\"softwaredev/docs/adr\",\"trigger\":\"file\"}}\n");
        }
        std::fs::write(home.join(".local/state/agent-ways/events.jsonl"), log).unwrap();
        // Claude Code's transcript of the newer session only, in the
        // directory claude-sessions names for the project (`_` becomes `-`).
        let dir = home.join(".claude/projects").join(proj.replace(['/', '_', '.'], "-"));
        std::fs::create_dir_all(&dir).unwrap();
        let turn = |ts: &str, k: u64| {
            format!("{{\"type\":\"assistant\",\"timestamp\":\"{ts}\",\"message\":{{\"model\":\"claude-opus-4-8\",\"usage\":{{\"input_tokens\":{},\"cache_read_input_tokens\":0}}}}}}\n", k * 1000)
        };
        std::fs::write(dir.join(format!("{NEW}.jsonl")), turn("2026-07-02T10:00:00Z", 40) + &turn("2026-07-02T10:01:00Z", 90)).unwrap();
        Fx { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let home = self.home();
        let o = Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(args)
            .current_dir(home.join("my_proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("CLAUDE_PROJECT_DIR", home.join("my_proj"))
            .output()
            .unwrap();
        (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code().unwrap_or(-1))
    }

    fn snap(&self, args: &[&str]) -> String {
        let (out, err, code) = self.run(args);
        assert_eq!(code, 0, "ways {}: {err}", args.join(" "));
        assert!(out.starts_with("agent-tui frame "), "not a frame: {out}{err}");
        out.split("\nstyles\n").next().unwrap_or(&out).to_string()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_picker_lists_the_project_s_sessions_and_opens_one() {
    let fx = Fx::new();
    let picker = fx.snap(&["introspect", "replay", "--depth", "none", "--snap", "100x12"]);
    assert!(picker.contains("sessions in") && picker.contains("(2)"), "{picker}");
    let newest = picker.find("bbbbbbbb-000").expect("the newer session");
    let older = picker.find("aaaaaaaa-000").expect("the older session");
    assert!(newest < older, "newest first: {picker}");
    // The transcript is found for the newer session only.
    let row = |id: &str| picker.lines().find(|l| l.contains(id)).unwrap_or("").to_string();
    assert!(row("bbbbbbbb").contains("yes") && row("aaaaaaaa").contains("gone"), "{picker}");

    // Enter opens the selected session, at its first frame; the transcript
    // gives the token position and the model's window.
    let replay = fx.snap(&["introspect", "replay", "--depth", "none", "--keys", "enter right", "--snap", "100x20"]);
    assert!(replay.contains(&format!("Session {NEW}")), "{replay}");
    assert!(replay.contains("epoch 2 · 1000K ctx · 2 ways"), "{replay}");
    assert!(replay.contains("(90K / 1000K)"), "the gauge reads the transcript: {replay}");
    assert!(replay.contains("softwaredev/docs/adr"), "{replay}");

    // Esc goes back to the picker; Down then Enter opens the older one.
    let back = fx.snap(&["introspect", "replay", "--depth", "none", "--keys", "enter esc down enter", "--snap", "100x20"]);
    assert!(back.contains(&format!("Session {OLD}")), "{back}");
}

#[test]
fn a_session_opens_directly_and_live_follows_the_newest() {
    let fx = Fx::new();
    let replay = fx.snap(&["introspect", "replay", "--session", OLD, "--depth", "none", "--snap", "100x20"]);
    assert!(replay.contains(&format!("Session {OLD}")) && replay.contains("1/2"), "{replay}");
    let live = fx.snap(&["introspect", "live", "--depth", "none", "--snap", "100x20"]);
    assert!(live.contains(&format!("Session {NEW}")) && live.contains("● LIVE") && live.contains("2/2"), "{live}");
    let why = fx.snap(&["introspect", "live", "--depth", "none", "--keys", "tab", "--snap", "100x20"]);
    assert!(why.contains("why it fired") && why.contains("Trigger"), "{why}");
}

#[test]
fn without_a_terminal_the_screens_say_so_and_json_needs_none() {
    let fx = Fx::new();
    let (out, err, code) = fx.run(&["introspect", "replay"]);
    assert_ne!(code, 0, "{out}");
    assert!(err.contains("needs a terminal") && err.contains("replay --json"), "{err}");

    let (out, err, code) = fx.run(&["introspect", "replay", "--session", NEW, "--json"]);
    assert_eq!(code, 0, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["session"], NEW);
    assert_eq!(v["frames"].as_array().map(Vec::len), Some(2), "{v}");
    assert_eq!(v["summary"]["distinct_ways"], 2, "{v}");

    let (out, err, code) = fx.run(&["introspect", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Transcript") && out.contains("bbbbbbbb-000"), "{out}");
}
