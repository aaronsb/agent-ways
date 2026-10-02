//! attend's settings in `ways settings` (ADR-503 §13, #698): its keys are
//! read, set, linted and fixed through the same registry and writer as
//! ways', in attend's own files and paths.
//!
//! Each test runs the built binary with HOME and every XDG directory in a
//! fixture of its own. `SETTINGS_CLI_BIN` runs the suite against another
//! build, such as the parent commit's, to see which tests fail before the
//! change.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ways-settings-attend-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home/.claude", "xdg/config", "proj"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        Fx { root }
    }

    fn user(&self) -> PathBuf {
        self.root.join("xdg/config/attend/config.yaml")
    }

    fn project(&self) -> PathBuf {
        self.root.join("proj/.claude/attend.yaml")
    }

    fn write(&self, p: &PathBuf, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, p: &PathBuf) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let bin = std::env::var_os("SETTINGS_CLI_BIN").map(PathBuf::from).unwrap_or_else(|| env!("CARGO_BIN_EXE_ways").into());
        let o = Command::new(bin)
            .args(args)
            .current_dir(self.root.join("proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("xdg/config"))
            .env("XDG_STATE_HOME", self.root.join("xdg/state"))
            .env("XDG_CACHE_HOME", self.root.join("xdg/cache"))
            .env("XDG_DATA_HOME", self.root.join("xdg/data"))
            .env("CLAUDE_PROJECT_DIR", self.root.join("proj"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let root = self.root.to_string_lossy().to_string();
        let s = |b: &[u8]| String::from_utf8_lossy(b).replace(&root, "<ROOT>");
        (s(&o.stdout), s(&o.stderr), o.status.code().unwrap_or(-1))
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const HAND: &str = "# my attend settings\nengagement:\n  decay_per_minute: 0.1  # tuned\n\nsensors:\n  git:\n    interval: 45   # slower here\n";

#[test]
fn set_writes_one_line_of_attend_s_user_file_and_get_reads_it() {
    let fx = Fx::new();
    fx.write(&fx.user(), HAND);
    let (_, err, code) = fx.run(&["settings", "set", "attend.engagement.decay_per_minute", "0.05"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.user()), HAND.replace("0.1  # tuned", "0.05  # tuned"));
    assert_eq!(fx.run(&["settings", "get", "attend.engagement.decay_per_minute"]).0, "0.05\n");
    assert_eq!(fx.run(&["settings", "get", "attend.sensors.git.interval"]).0, "45\n");
    assert_eq!(fx.run(&["settings", "get", "attend.sensors.peers.requires"]).0, "[\"Read\"]\n");
}

#[test]
fn a_value_outside_its_range_exits_3_and_writes_nothing() {
    let fx = Fx::new();
    fx.write(&fx.user(), HAND);
    let (_, err, code) = fx.run(&["settings", "set", "attend.engagement.decay_per_minute", "3"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("3 is outside 0..1"), "{err}");
    assert_eq!(fx.read(&fx.user()), HAND);
}

#[test]
fn project_writes_the_project_s_attend_yaml_and_overrides_the_user_file() {
    let fx = Fx::new();
    fx.write(&fx.user(), HAND);
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.git.interval", "90", "--project", "."]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.project()), "# attend settings; `ways settings help attend` describes each key\nsensors:\n  git:\n    interval: 90\n");
    // Set in the user file now, the project still wins: exit 4 names it.
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.git.interval", "50"]);
    assert_eq!(code, 4, "{err}");
    assert!(err.contains("proj/.claude/attend.yaml sets attend.sensors.git.interval"), "{err}");
}

#[test]
fn lint_names_attend_s_findings_and_fix_repairs_them() {
    let fx = Fx::new();
    fx.write(&fx.user(), "# mine\ngovernor:\n  base_cooldown: abc   # oops\n  rate_window: 60\n");
    fx.write(&fx.project(), "sensors:\n  -processes:\n  git:\n    interval: 30\n");
    let (out, _, code) = fx.run(&["settings", "lint"]);
    assert_eq!(code, 3);
    assert!(out.contains("<ROOT>/xdg/config/attend/config.yaml:3: [attend.governor] governor.base_cooldown: expected an integer"), "{out}");
    assert!(out.contains("<ROOT>/proj/.claude/attend.yaml:2: [attend.sensors] sensors.-processes: '-processes' is not a sensor name"), "{out}");
    let (_, err, code) = fx.run(&["settings", "fix", "attend.governor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.user()), "# mine\ngovernor:\n  base_cooldown: 15   # oops\n  rate_window: 60\n", "the user file takes the canonical value");
    let (_, err, code) = fx.run(&["settings", "fix", "attend.sensors", "--project", "."]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.project()), "sensors:\n  git:\n    interval: 30\n", "the project file loses the entry that names nothing");
    assert_eq!(fx.run(&["settings", "lint"]).2, 0);
}

#[test]
fn a_copied_attend_file_reads_with_file() {
    let fx = Fx::new();
    let copy = fx.root.join("copy/attend.yaml");
    fx.write(&copy, "sensors:\n  processes:\n    enabled: false\n");
    let (out, err, code) = fx.run(&["settings", "list", "attend.sensors.processes", "--file", copy.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("attend.sensors.processes.enabled=false\n"), "{out}");
    let user = fx.root.join("copy/attend/config.yaml");
    fx.write(&user, "governor:\n  base_cooldown: x\n");
    assert_eq!(fx.run(&["settings", "lint", "--file", user.to_str().unwrap()]).2, 3);
}

#[test]
fn emit_prints_attend_s_canonical_fragment_and_apply_takes_it_back() {
    let fx = Fx::new();
    let (frag, _, code) = fx.run(&["settings", "emit", "attend.engagement"]);
    assert_eq!(code, 0);
    assert_eq!(frag, "engagement:\n  burst_threshold: 3\n  step_multiplier: 1.25\n  absolute_refractory: 60\n  decay_per_minute: 0.1\n  peer_activity_window: 900\n");
    let path = fx.root.join("frag.yaml");
    fx.write(&path, &frag.replace("0.1", "0.2"));
    let (_, err, code) = fx.run(&["settings", "apply", "--file", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.run(&["settings", "get", "attend.engagement.decay_per_minute"]).0, "0.2\n");
}

#[test]
fn attend_reads_the_one_theme_choice_ways_writes() {
    let fx = Fx::new();
    for (k, v) in [("theme.active", "nord"), ("theme.shape", "flame")] {
        let (_, err, code) = fx.run(&["settings", "set", k, v]);
        assert_eq!(code, 0, "{err}");
    }
    let choice = attend_config::theme::choice_in(&fx.root.join("xdg/config/agent-ways"));
    assert_eq!((choice.active.as_deref(), choice.shape.as_deref()), (Some("nord"), Some("flame")));
    // attend has no theme key of its own: the registry knows only ways'.
    let (_, _, code) = fx.run(&["settings", "get", "attend.theme.active"]);
    assert_eq!(code, 2);
}

#[test]
fn help_describes_an_attend_key_and_the_sensors_tab() {
    let fx = Fx::new();
    let (out, _, code) = fx.run(&["settings", "help", "attend.sensors.*.script"]);
    assert_eq!(code, 0);
    assert!(out.contains("type:    path") && out.contains("file:    attend key sensors.*.script"), "{out}");
    let (out, _, code) = fx.run(&["settings", "help", "sensors"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("attend.sensors: "), "{out}");
}
