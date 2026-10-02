//! attend's settings through the schema (ADR-503 §13, #698), through the
//! real binary with HOME and every XDG directory in a fixture of its own.
//!
//! `ATTEND_BIN` runs the suite against another build, such as the parent
//! commit's, to see which tests fail before the change.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    home: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!("attend-config-settings-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        for d in ["config", "cache", "data", "proj/.claude", ".claude/projects"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        Fx { home }
    }

    fn user(&self) -> PathBuf {
        self.home.join("config/attend/config.yaml")
    }

    fn write(&self, p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let bin = std::env::var_os("ATTEND_BIN").map(PathBuf::from).unwrap_or_else(|| env!("CARGO_BIN_EXE_attend").into());
        let mut c = Command::new(bin);
        c.args(args)
            .current_dir(self.home.join("proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("XDG_CACHE_HOME", self.home.join("cache"))
            .env("XDG_DATA_HOME", self.home.join("data"))
            .env("CLAUDE_PROJECT_DIR", self.home.join("proj"))
            .env("NO_COLOR", "1");
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    /// Two transcripts with a regular turn cycle, so `tune` has samples.
    fn sessions(&self) {
        let mut lines = String::new();
        for i in 0..40u32 {
            let t = i * 120;
            let ts = |s: u32| format!("2026-09-01T{:02}:{:02}:{:02}.000Z", s / 3600, (s / 60) % 60, s % 60);
            lines.push_str(&format!("{{\"type\":\"user\",\"timestamp\":\"{}\"}}\n", ts(t)));
            lines.push_str(&format!("{{\"type\":\"assistant\",\"timestamp\":\"{}\"}}\n", ts(t + 50)));
        }
        self.write(&self.home.join(".claude/projects/-proj/s1.jsonl"), &lines);
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// A hand-edited user file: comments, an order, and a burst_threshold of the
/// operator's own that tune must not overwrite.
const HAND: &str = "# my attend settings\ngovernor:\n  base_cooldown: 20   # slower\n\nengagement:\n  # tuned by hand in March\n  burst_threshold: 5\n  step_multiplier: 1.5\n  absolute_refractory: 60   # think time\n  decay_per_minute: 0.1\n  peer_activity_window: 900\n# trailing note\n";

// ── tune --apply ───────────────────────────────────────────────

#[test]
fn tune_apply_changes_only_the_derived_keys_and_keeps_comments() {
    let fx = Fx::new();
    fx.sessions();
    fx.write(&fx.user(), HAND);
    let out = fx.run(&["tune", "--apply"]);
    assert!(out.status.success(), "{}{}", text(&out.stdout), text(&out.stderr));
    let after = std::fs::read_to_string(fx.user()).unwrap();
    // Only the three derived values may differ, each on its own line.
    let (old, new): (Vec<&str>, Vec<&str>) = (HAND.lines().collect(), after.lines().collect());
    assert_eq!(old.len(), new.len(), "{after}");
    for (a, b) in old.iter().zip(&new) {
        let derived = ["absolute_refractory:", "decay_per_minute:", "peer_activity_window:"].iter().any(|k| a.trim_start().starts_with(k));
        if !derived {
            assert_eq!(a, b, "a line tune does not own changed:\n{after}");
        }
    }
    assert!(after.contains("  absolute_refractory: 70   # think time\n"), "the inline comment stays:\n{after}");
    assert!(after.contains("burst_threshold: 5") && after.contains("step_multiplier: 1.5"), "{after}");
    // The think time is 70 s; the window is the turn cycle's p90, 120 s,
    // times the operator's burst_threshold, 5.
    assert!(after.contains("peer_activity_window: 600"), "{after}");
}

#[cfg(unix)]
#[test]
fn tune_apply_replaces_the_file_atomically_and_leaves_nothing_beside_it() {
    use std::os::unix::fs::MetadataExt;
    let fx = Fx::new();
    fx.sessions();
    fx.write(&fx.user(), HAND);
    let before = std::fs::metadata(fx.user()).unwrap().ino();
    assert!(fx.run(&["tune", "--apply"]).status.success());
    assert_ne!(std::fs::metadata(fx.user()).unwrap().ino(), before, "written in place, not renamed into place");
    let names: Vec<String> = std::fs::read_dir(fx.user().parent().unwrap()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["config.yaml".to_string()], "a lock or temporary file was left");
}

#[test]
fn tune_apply_waits_for_the_settings_lock() {
    let fx = Fx::new();
    fx.sessions();
    fx.write(&fx.user(), HAND);
    let held = agent_settings::writer::Lock::acquire(&fx.user()).unwrap();
    let mut child = fx.cmd(&["tune", "--apply"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(800) {
        assert!(child.try_wait().unwrap().is_none(), "tune wrote while another writer held the lock");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(fx.user()).unwrap(), HAND, "nothing written under a held lock");
    drop(held);
    assert!(child.wait().unwrap().success());
    assert_ne!(std::fs::read_to_string(fx.user()).unwrap(), HAND, "written once the lock was free");
}

#[test]
fn tune_apply_derives_from_the_file_as_it_is_under_the_lock() {
    let fx = Fx::new();
    fx.sessions();
    fx.write(&fx.user(), "engagement:\n  burst_threshold: 3\n");
    let held = agent_settings::writer::Lock::acquire(&fx.user()).unwrap();
    let mut child = fx.cmd(&["tune", "--apply"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(400));
    // An edit lands while tune waits for the lock; tune must use it.
    fx.write(&fx.user(), "engagement:\n  burst_threshold: 5\n");
    drop(held);
    assert!(child.wait().unwrap().success());
    let after = std::fs::read_to_string(fx.user()).unwrap();
    assert!(after.contains("peer_activity_window: 600"), "derived from burst_threshold 5, not the 3 read before the lock:\n{after}");
}

#[test]
fn tune_apply_clamps_a_derived_decay_to_its_range() {
    let fx = Fx::new();
    fx.sessions();
    fx.write(&fx.user(), "engagement:\n  burst_threshold: 1\n  step_multiplier: 100\n");
    let out = fx.run(&["tune", "--apply"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(std::fs::read_to_string(fx.user()).unwrap().contains("decay_per_minute: 1.0"));
}

#[test]
fn an_old_minus_entry_switches_the_project_s_sensors_off() {
    let fx = Fx::new();
    fx.write(&fx.home.join("proj/.claude/attend.yaml"), "sensors:\n  -processes:\n");
    let o = text(&fx.run(&["config", "show"]).stdout);
    assert!(o.contains("attend.sensors.processes.enabled=false\n") && o.contains("attend.sensors.git.enabled=false\n"), "{o}");
}

#[test]
fn a_project_file_that_does_not_parse_switches_its_sensors_off() {
    let fx = Fx::new();
    fx.write(&fx.home.join("proj/.claude/attend.yaml"), "sensors:\n  processes:\n    enabled: false\ngovernor: [\n");
    let o = text(&fx.run(&["config", "show"]).stdout);
    assert!(o.contains("attend.sensors.processes.enabled=false\n") && o.contains("attend.sensors.peers.enabled=false\n"), "{o}");
}

#[test]
fn max_per_window_zero_mutes() {
    let fx = Fx::new();
    fx.write(&fx.user(), "governor:\n  max_per_window: 0\n");
    let out = fx.run(&["config", "show"]);
    assert!(text(&out.stdout).contains("attend.governor.max_per_window=0\n"), "{}", text(&out.stderr));
}

#[test]
fn tune_apply_on_no_file_writes_only_the_derived_values() {
    let fx = Fx::new();
    fx.sessions();
    assert!(fx.run(&["tune", "--apply"]).status.success());
    let body = std::fs::read_to_string(fx.user()).unwrap();
    assert_eq!(
        body,
        "# attend settings; `ways settings help attend` describes each key\nengagement:\n  absolute_refractory: 70\n  decay_per_minute: 0.1042\n  peer_activity_window: 360\n"
    );
    let lint = fx.run(&["config", "lint"]);
    assert!(lint.status.success(), "{}", text(&lint.stdout));
}

// ── lint and show ──────────────────────────────────────────────

#[test]
fn lint_reports_a_bad_value_with_its_line_and_exits_3() {
    let fx = Fx::new();
    fx.write(&fx.user(), "governor:\n  base_cooldown: abc\n");
    fx.write(&fx.home.join("proj/.claude/attend.yaml"), "sensors:\n  -processes:\n");
    let out = fx.run(&["config", "lint"]);
    assert_eq!(out.status.code(), Some(3), "{}{}", text(&out.stdout), text(&out.stderr));
    let o = text(&out.stdout);
    assert!(o.contains("config/attend/config.yaml:2: [attend.governor] governor.base_cooldown: expected an integer"), "{o}");
    assert!(o.contains("proj/.claude/attend.yaml:2: [attend.sensors] sensors.-processes: '-processes' is not a sensor name"), "{o}");
    let clean = Fx::new();
    assert_eq!(clean.run(&["config", "lint"]).status.code(), Some(0));
}

#[test]
fn show_prints_the_values_in_effect_as_key_value_lines() {
    let fx = Fx::new();
    fx.write(&fx.user(), "engagement:\n  decay_per_minute: 0.05\n");
    fx.write(&fx.home.join("proj/.claude/attend.yaml"), "sensors:\n  processes:\n    enabled: false\n");
    let out = fx.run(&["config", "show"]);
    assert!(out.status.success());
    let o = text(&out.stdout);
    for want in ["attend.engagement.decay_per_minute=0.05\n", "attend.sensors.processes.enabled=false\n", "attend.governor.base_cooldown=15\n", "attend.sensors.git.requires=[Bash(git:*)]\n"] {
        assert!(o.contains(want), "{want} missing:\n{o}");
    }
}

#[test]
fn a_bad_section_names_itself_on_stderr_and_the_rest_load() {
    let fx = Fx::new();
    fx.write(&fx.user(), "engagement:\n  burst_window: 900\ngovernor:\n  base_cooldown: 40\n");
    let out = fx.run(&["config", "show"]);
    assert!(out.status.success(), "a retired key no longer stops attend: {}", text(&out.stderr));
    let (o, e) = (text(&out.stdout), text(&out.stderr));
    assert!(o.contains("attend.governor.base_cooldown=40\n") && o.contains("attend.engagement.burst_threshold=3\n"), "{o}");
    assert!(e.contains("[attend] settings:") && e.contains("[attend.engagement] engagement.burst_window: unknown key") && e.contains("`ways settings fix attend.engagement`"), "{e}");
}

#[test]
fn init_never_overwrites_a_file() {
    let fx = Fx::new();
    fx.write(&fx.user(), "# mine\n");
    let out = fx.run(&["config", "init"]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(fx.user()).unwrap(), "# mine\n");
}
