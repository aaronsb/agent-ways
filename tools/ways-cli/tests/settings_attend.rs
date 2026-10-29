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
        for d in ["home/.claude", "xdg/config", "proj/.claude"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // proj is a project root, as `--project .` resolves it.
        std::fs::write(root.join("proj/CLAUDE.md"), "").unwrap();
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
        // The root as written and as resolved: macOS's temp directory sits
        // behind a link, and the binary prints a `--project` it resolved.
        let root = self.root.to_string_lossy().to_string();
        let real = std::fs::canonicalize(&self.root).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| root.clone());
        let s = |b: &[u8]| String::from_utf8_lossy(b).replace(&real, "<ROOT>").replace(&root, "<ROOT>");
        (s(&o.stdout), s(&o.stderr), o.status.code().unwrap_or(-1))
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const HAND: &str = "# my attend settings\nengagement:\n  decay_per_minute: 0.1  # tuned\n\ncleanup:\n  interval: 900   # slower here\n";

#[test]
fn set_writes_one_line_of_attend_s_user_file_and_get_reads_it() {
    let fx = Fx::new();
    fx.write(&fx.user(), HAND);
    let (_, err, code) = fx.run(&["settings", "set", "attend.engagement.decay_per_minute", "0.05"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.user()), HAND.replace("0.1  # tuned", "0.05  # tuned"));
    assert_eq!(fx.run(&["settings", "get", "attend.engagement.decay_per_minute"]).0, "0.05\n");
    assert_eq!(fx.run(&["settings", "get", "attend.cleanup.interval"]).0, "900\n");
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
    let (_, err, code) = fx.run(&["settings", "set", "attend.cleanup.interval", "90", "--project", "."]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.project()), "# attend settings; `ways settings help attend` describes each key\ncleanup:\n  interval: 90\n");
    // Set in the user file now, the project still wins: exit 4 names it.
    let (_, err, code) = fx.run(&["settings", "set", "attend.cleanup.interval", "50"]);
    assert_eq!(code, 4, "{err}");
    assert!(err.contains("proj/.claude/attend.yaml sets attend.cleanup.interval"), "{err}");
}

#[test]
fn lint_names_attend_s_findings_and_fix_repairs_them() {
    let fx = Fx::new();
    fx.write(&fx.user(), "# mine\ngovernor:\n  base_cooldown: abc   # oops\n  rate_window: 60\n");
    fx.write(&fx.project(), "sensors:\n  -processes:\n  git:\n    interval: 30\n");
    let (out, _, code) = fx.run(&["settings", "lint"]);
    assert_eq!(code, 3);
    assert!(out.contains("<ROOT>/xdg/config/attend/config.yaml:3: [attend.governor] governor.base_cooldown: expected an integer"), "{out}");
    // The retired sensors section names each old key's sensor file (ADR-403 §7).
    assert!(out.contains("<ROOT>/proj/.claude/attend.yaml:2: sensors.-processes: retired by ADR-403; write `enabled: false` in .claude/attend/sensors/processes.yaml"), "{out}");
    assert!(out.contains("<ROOT>/proj/.claude/attend.yaml:4: sensors.git.interval: retired by ADR-403; write `interval: 30` in .claude/attend/sensors/git.yaml"), "{out}");
    let (_, err, code) = fx.run(&["settings", "fix", "attend.governor"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.read(&fx.user()), "# mine\ngovernor:\n  base_cooldown: 15   # oops\n  rate_window: 60\n", "the user file takes the canonical value");
    // The files the lines name clear them.
    fx.write(&fx.project(), "");
    fx.write(&fx.root.join("proj/.claude/attend/sensors/processes.yaml"), "enabled: false\n");
    fx.write(&fx.root.join("proj/.claude/attend/sensors/git.yaml"), "interval: 30\n");
    let (out, _, code) = fx.run(&["settings", "lint"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_copied_attend_file_reads_with_file() {
    let fx = Fx::new();
    let copy = fx.root.join("copy/attend.yaml");
    fx.write(&copy, "cleanup:\n  enabled: false\n");
    let (out, err, code) = fx.run(&["settings", "list", "attend.cleanup", "--file", copy.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("attend.cleanup.enabled=false\n"), "{out}");
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
fn help_describes_an_attend_key() {
    let fx = Fx::new();
    let (out, _, code) = fx.run(&["settings", "help", "attend.project_sensors"]);
    assert_eq!(code, 0);
    assert!(out.contains("file:    attend key project_sensors") && out.contains("user"), "{out}");
}

#[test]
fn the_chat_keys_are_listed_set_and_refused_like_any_attend_key() {
    let fx = Fx::new();
    // Read with their defaults before any file sets them.
    assert_eq!(fx.run(&["settings", "get", "attend.chat.tabs.jump"]).0, "auto\n");
    assert_eq!(fx.run(&["settings", "get", "attend.chat.tabs.menu_on_repeat"]).0, "true\n");
    assert_eq!(fx.run(&["settings", "get", "attend.chat.tabs.focus_key"]).0, "both\n");
    assert_eq!(fx.run(&["settings", "get", "attend.chat.mouse"]).0, "true\n");
    let (_, err, code) = fx.run(&["settings", "set", "attend.chat.tabs.jump", "ctrl"]);
    assert_eq!(code, 0, "{err}");
    assert!(fx.read(&fx.user()).contains("chat:\n  tabs:\n    jump: ctrl\n"), "{}", fx.read(&fx.user()));
    assert_eq!(fx.run(&["settings", "get", "attend.chat.tabs.jump"]).0, "ctrl\n");
    // A value outside the choice writes nothing.
    let before = fx.read(&fx.user());
    let (_, err, code) = fx.run(&["settings", "set", "attend.chat.tabs.jump", "super"]);
    assert_eq!(code, 3, "{err}");
    assert_eq!(fx.read(&fx.user()), before);
    // The chat's keys are the user's: a project file cannot set them.
    let (_, _, code) = fx.run(&["settings", "set", "attend.chat.mouse", "true", "--project", "."]);
    assert_ne!(code, 0, "a project cannot set a user key");
}

// The attend mod and keepwarm read `list mods.attend --json --effective
// --project <session dir>`. A session started in the home directory or a
// scratch one is in no project: the read keeps the user's value, exit 0.
#[test]
fn a_session_dir_in_no_project_reads_the_user_layer() {
    let fx = Fx::new();
    fx.write(&fx.root.join("xdg/config/agent-ways/config.yaml"), "mods:\n  attend:\n    listening: active\n");
    std::fs::create_dir_all(fx.root.join("scratch")).unwrap();
    for dir in ["home", "scratch"] {
        let at = fx.root.join(dir);
        let (out, err, code) = fx.run(&["settings", "list", "mods.attend", "--json", "--effective", "--project", at.to_str().unwrap()]);
        assert_eq!(code, 0, "{dir}: {err}");
        assert!(out.contains("\"mods.attend.listening\": {\n      \"value\": \"active\""), "{dir}: {out}");
    }
}

/// The user's sensor file for `name`.
fn sensor(fx: &Fx, name: &str) -> PathBuf {
    fx.root.join(format!("xdg/config/attend/sensors/{name}.yaml"))
}

// A sensor's field is a key over its files (ADR-403 §5): set writes the
// user's sensor file, creating it with only that field over the shipped
// one, and get and list name that file as the source.
#[test]
fn a_sensor_field_is_set_in_the_user_s_sensor_file_over_the_shipped_one() {
    let fx = Fx::new();
    let (out, _, code) = fx.run(&["settings", "get", "attend.sensors.git.interval", "--json"]);
    assert_eq!(code, 0);
    assert!(out.contains("\"value\": 30") && out.contains("\"layer\": \"shipped\""), "{out}");
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.git.interval", "90"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("user: <ROOT>/xdg/config/attend/sensors/git.yaml"), "{err}");
    assert!(fx.read(&sensor(&fx, "git")).ends_with("\ninterval: 90\n"), "{}", fx.read(&sensor(&fx, "git")));
    let (out, _, _) = fx.run(&["settings", "list", "--json", "attend.sensors.git"]);
    assert!(out.contains("\"<ROOT>/xdg/config/attend/sensors/git.yaml\": {\n      \"interval\": 90"), "the stored fragment has the file's shape:\n{out}");
    assert!(out.contains("\"file\": \"<ROOT>/xdg/config/attend/sensors/git.yaml\""), "{out}");
    // unset takes the field out, and the shipped value is in effect again.
    let (_, err, code) = fx.run(&["settings", "unset", "attend.sensors.git.interval"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(fx.run(&["settings", "get", "attend.sensors.git.interval"]).0, "30\n");
}

#[test]
fn a_value_that_makes_the_sensor_invalid_exits_3_and_says_why() {
    let fx = Fx::new();
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.git.min_interval", "500"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("would make the sensor git invalid: min_interval: min_interval 500 is longer than interval 30"), "{err}");
    assert!(!sensor(&fx, "git").exists(), "nothing written");
    // A field of a sensor no file defines defines no sensor.
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.weather.interval", "40"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("kind is required"), "{err}");
    assert!(!sensor(&fx, "weather").exists());
}

#[test]
fn project_writes_the_project_s_sensor_file() {
    let fx = Fx::new();
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.processes.enabled", "false", "--project", "."]);
    assert_eq!(code, 0, "{err}");
    let file = fx.root.join("proj/.claude/attend/sensors/processes.yaml");
    assert!(fx.read(&file).ends_with("\nenabled: false\n"), "{}", fx.read(&file));
    // Set in the user's file now, the project's still wins: exit 4 names it.
    let (_, err, code) = fx.run(&["settings", "set", "attend.sensors.processes.enabled", "true"]);
    assert_eq!(code, 4, "{err}");
    assert!(err.contains("proj/.claude/attend/sensors/processes.yaml sets attend.sensors.processes.enabled"), "{err}");
}

// A sensor's name is its file's name, so a name that is a path is refused
// before any file is opened.
#[test]
fn a_sensor_name_that_is_a_path_is_refused_by_set_and_writes_nothing() {
    let fx = Fx::new();
    let outside = fx.root.join("outside/compose.yaml");
    fx.write(&outside, "interval: 1\n");
    let stem = fx.root.join("outside/compose");
    for name in [stem.to_str().unwrap(), "../../../outside/compose"] {
        let (_, err, code) = fx.run(&["settings", "set", &format!("attend.sensors.{name}.interval"), "7"]);
        assert_eq!(code, 2, "{name}: {err}");
        assert!(err.contains("is not a sensor name"), "{name}: {err}");
    }
    assert_eq!(fx.read(&outside), "interval: 1\n", "the file outside is untouched");
    assert!(!fx.root.join("xdg/config/attend/sensors").exists(), "nothing written");
}

#[test]
fn a_sensor_name_that_is_a_path_is_refused_by_apply_and_writes_nothing() {
    let fx = Fx::new();
    let obj = fx.root.join("obj.yaml");
    fx.write(&obj, "sensors:\n  \"../../../outside/dotdot\":\n    enabled: false\n");
    let (out, err, code) = fx.run(&["settings", "apply", "--file", obj.to_str().unwrap()]);
    assert_ne!(code, 0, "{out}{err}");
    assert!(format!("{out}{err}").contains("is not a sensor name"), "{out}{err}");
    assert!(!fx.root.join("outside").exists() && !fx.root.join("xdg/outside").exists(), "nothing written outside the sensor directories");
    assert!(!fx.root.join("xdg/config/attend/sensors").exists(), "nothing written");
}
