// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend keepwarm check` through the CLI: ADR-182's judgment run once per
//! call, for the attend mod, which polls it. A
//! stand-in `ways` on PATH serves the transcript reading the check takes
//! from `ways context --json`, and the `mods.attend` settings.

mod common;
use common::Fixture;

/// A build without the keepwarm sensor cannot judge: `check` prints
/// nothing and exits 0, so a polling host reads "no wake due", and the
/// arming verbs refuse.
#[cfg(not(feature = "sensor-keepwarm"))]
#[test]
fn without_the_sensor_the_check_prints_nothing_and_exits_0() {
    let f = Fixture::new("kw-no-sensor");
    let out = f.attend(&["keepwarm", "check"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no keepwarm sensor"));
    assert_eq!(f.attend(&["keepwarm", "on"]).status.code(), Some(1));
}

/// The judgment itself, in a build with the sensor.
#[cfg(feature = "sensor-keepwarm")]
mod judged {
use super::*;
use std::ffi::OsString;
use std::path::PathBuf;


const MIN: u64 = 60;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A fixture and a `ways` that prints whatever [`Warm::reading`] last
/// wrote, and the `mods.attend` settings [`Warm::settings`] last wrote.
struct Warm {
    f: Fixture,
    bin: PathBuf,
    reading: PathBuf,
}

impl Warm {
    fn new(tag: &str) -> Self {
        let f = Fixture::new(tag);
        let bin = f.home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let reading = f.home.join("reading.json");
        let ways = bin.join("ways");
        // `ways settings ...` answers with settings.json beside the reading,
        // when a test wrote one; anything else is `ways context --json`.
        let settings = f.home.join("settings.json");
        // The idle path runs only where ways says keepalive: true.
        std::fs::write(&settings, r#"{"view":"effective","keys":{"mods.attend.continuity.keepalive":{"value":true}}}"#).unwrap();
        std::fs::write(
            &ways,
            format!(
                "#!/bin/sh\necho \"$1\" >> '{}'\nif [ \"$1\" = settings ]; then cat '{}'; else cat '{}'; fi\n",
                f.home.join("ways-calls.log").display(),
                settings.display(),
                reading.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ways, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Warm { f, bin, reading }
    }

    /// The effective `mods.attend` keys `ways settings list` answers with,
    /// as a JSON object of key to value.
    fn settings(&self, keys: &str) {
        std::fs::write(self.f.home.join("settings.json"), format!(r#"{{"view":"effective","keys":{keys}}}"#)).unwrap();
    }

    /// The transcript's usage tail: `(seconds ago, cache read, cache write)`.
    fn reading(&self, entries: &[(u64, u64, u64)]) {
        let t = now();
        let tail: Vec<String> = entries
            .iter()
            .map(|(ago, read, write)| {
                format!(
                    r#"{{"at_epoch":{},"model":"claude-fable-5-1","input":20,"cache_read":{read},"cache_creation":{write},"output":5}}"#,
                    t - ago
                )
            })
            .collect();
        let (_, read, write) = entries.last().unwrap();
        std::fs::write(
            &self.reading,
            format!(r#"{{"tokens_used":{},"model":"claude-fable-5-1","usage_tail":[{}]}}"#, read + write + 20, tail.join(",")),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut path = OsString::from(&self.bin);
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        self.f.attend_with(args, &[("PATH", &path)])
    }

    /// `attend keepwarm check`'s stdout; it always exits 0.
    fn check(&self) -> String {
        let out = self.run(&["keepwarm", "check"]);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn status_line(&self) -> String {
        let out = self.run(&["keepwarm", "status"]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).lines().find(|l| l.starts_with("keepwarm")).unwrap().to_string()
    }

    fn ledger(&self) -> PathBuf {
        self.f.cache().join("keepwarm").join(format!("{}.ledger.json", self.f.sid))
    }
}

/// Armed, a large context, 51 idle minutes: the first check baselines as
/// the sensor's first poll does, the second wakes, and the wake is not
/// repeated in the same idle stretch.
#[test]
fn an_armed_idle_session_is_woken_once() {
    let w = Warm::new("kw-wake");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    assert_eq!(w.check(), "", "the first check baselines");
    let line = w.check();
    assert!(line.starts_with("keepwarm: ") && line.ends_with("Reply with one word and no tools.\n"), "{line:?}");
    assert_eq!(line.lines().count(), 1);
    assert_eq!(w.check(), "", "one wake per idle stretch");
}

#[test]
fn an_unarmed_session_is_not_woken() {
    let w = Warm::new("kw-unarmed");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert_eq!(w.check(), "");
    assert_eq!(w.check(), "");
}

#[test]
fn a_small_context_is_not_worth_a_wake() {
    let w = Warm::new("kw-small");
    w.reading(&[(51 * MIN, 20_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    assert_eq!(w.check(), "");
    assert_eq!(w.check(), "");
}

#[test]
fn not_yet_fifty_idle_minutes_is_no_wake() {
    let w = Warm::new("kw-early");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    assert_eq!(w.check(), "");
    assert_eq!(w.check(), "");
}

/// A wake that no turn follows within ten minutes disarms, with the reason.
#[test]
fn a_wake_with_no_turn_disarms() {
    let w = Warm::new("kw-noturn");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    w.check();
    assert!(w.check().starts_with("keepwarm: "));
    // Eleven minutes pass with no assistant turn: move the recorded wake back.
    let ledger = std::fs::read_to_string(w.ledger()).unwrap();
    let mut doc: serde_json::Value = serde_json::from_str(&ledger).unwrap();
    doc["pinged_at"] = serde_json::json!(now() - 11 * MIN);
    std::fs::write(w.ledger(), doc.to_string()).unwrap();

    assert_eq!(w.check(), "");
    let status = w.status_line();
    assert!(status.contains("stopped, the wake produced no turn within ten minutes"), "{status}");
}

/// A paid cold write on a large context arms three hours on its own.
#[test]
fn a_paid_cold_write_arms_keepwarm() {
    let w = Warm::new("kw-coldwrite");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    assert_eq!(w.check(), "");
    assert!(w.status_line().contains("not armed"), "{}", w.status_line());
    w.reading(&[(30 * MIN, 200_000, 500), (MIN, 0, 200_500)]);
    assert_eq!(w.check(), "");
    let status = w.status_line();
    assert!(status.starts_with("keepwarm    armed, 3h00m left") || status.starts_with("keepwarm    armed, 2h59m left"), "{status}");
    // The sensor's own arm is marked, so the keepalive policy ignores it.
    let arm = w.f.cache().join("keepwarm").join(format!("{}.arm.json", w.f.sid));
    let arm: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(arm).unwrap()).unwrap();
    assert_eq!(arm["auto"], true, "{arm}");
}

/// `mods.attend.keepwarm: off`, the switch the attend mod reads, turns the
/// check off, the plain one and the policy's, where a wake is due. With
/// the key on, the same session wakes: the control.
#[test]
fn the_check_is_off_where_mods_attend_keepwarm_is_off() {
    let w = Warm::new("kw-config-off");
    w.settings(r#"{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.keepwarm":{"value":"off"}}"#);
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    for args in [&["keepwarm", "check"][..], &["keepwarm", "check", "--policy", "--json", "--autocompact", "off"][..]] {
        for _ in 0..2 {
            let out = w.run(args);
            assert_eq!(out.status.code(), Some(0), "{args:?}");
            let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            assert!(!String::from_utf8_lossy(&out.stdout).starts_with("keepwarm: "), "{args:?}: {said}");
            assert!(said.contains("mods.attend.keepwarm is off"), "{args:?}: {said}");
        }
    }
    let on = Warm::new("kw-config-on");
    on.settings(r#"{"mods.attend.keepwarm":{"value":"on"}}"#);
    on.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(on.run(&["keepwarm", "on"]).status.success());
    on.check();
    assert!(on.check().starts_with("keepwarm: "), "the control wakes");
}

/// A `keepwarm.yaml` that sets `enabled: false`, the opt-out from when a
/// shipped sensor file switched keepwarm on, still turns the check off for
/// this release, and the check names the setting to move to. One that
/// switches nothing off does not stop a due wake. `attend sensor list`
/// names the setting for either.
#[test]
fn a_legacy_keepwarm_yaml_opt_out_is_honoured_for_one_release() {
    let w = Warm::new("kw-legacy-off");
    w.f.sensor("keepwarm", "enabled: false\n");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    w.check();
    let out = w.run(&["keepwarm", "check"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("keepwarm.yaml no longer switches keepwarm: set `ways settings set mods.attend.keepwarm off`"), "{err}");
    assert!(err.contains("this release still honours their enabled: false as an opt-out"), "{err}");
    let list = w.run(&["sensor", "list"]);
    let listed = String::from_utf8_lossy(&list.stdout);
    assert!(listed.contains("keepwarm: ") && listed.contains("no longer switches keepwarm"), "{listed}");

    let on = Warm::new("kw-legacy-on");
    on.f.sensor("keepwarm", "enabled: true\n");
    on.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(on.run(&["keepwarm", "on"]).status.success());
    on.check();
    assert!(on.check().starts_with("keepwarm: "), "a file that opts out of nothing stops no wake");
    let listed = String::from_utf8_lossy(&on.run(&["sensor", "list"]).stdout).into_owned();
    assert!(listed.contains("no longer switches keepwarm") && !listed.contains("still honours"), "{listed}");
}

/// The project's `keepwarm.yaml` in the fixture project.
fn project_keepwarm(w: &Warm, text: &str) {
    let dir = std::path::Path::new(&w.f.origin).join(".claude").join("attend").join("sensors");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keepwarm.yaml"), text).unwrap();
}

/// The legacy files resolve `enabled` as the layers did: the project's
/// file over the user's, and a file that does not set it leaves it to the
/// one below. A user's tuning file and a project's `enabled: false` read
/// off; a user's `enabled: false` and a project's `enabled: true` read on.
/// The check's note names every file present.
#[test]
fn the_legacy_files_resolve_enabled_project_over_user() {
    let off = Warm::new("kw-legacy-proj-off");
    off.f.sensor("keepwarm", "interval: 90\n");
    project_keepwarm(&off, "enabled: false\n");
    off.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(off.run(&["keepwarm", "on"]).status.success());
    off.check();
    let out = off.run(&["keepwarm", "check"]);
    assert!(out.stdout.is_empty(), "the project's opt-out holds: {}", String::from_utf8_lossy(&out.stdout));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("config/attend/sensors/keepwarm.yaml and ") && err.contains("proj/.claude/attend/sensors/keepwarm.yaml no longer switch keepwarm"), "{err}");

    let on = Warm::new("kw-legacy-proj-on");
    on.f.sensor("keepwarm", "enabled: false\n");
    project_keepwarm(&on, "enabled: true\n");
    on.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(on.run(&["keepwarm", "on"]).status.success());
    on.check();
    assert!(on.check().starts_with("keepwarm: "), "the project's enabled: true wins over the user's opt-out");
}

/// `attend config lint` names a legacy `keepwarm.yaml` and the config key
/// to move to, as a warning that leaves the exit alone.
#[test]
fn attend_config_lint_names_the_legacy_keepwarm_file() {
    let w = Warm::new("kw-legacy-lint");
    w.f.sensor("keepwarm", "enabled: false\n");
    let out = w.run(&["config", "lint"]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("keepwarm.yaml: [attend.sensors.keepwarm] keepwarm: warning: ") && said.contains("`ways settings set mods.attend.keepwarm off`"), "{said}");
}

/// Outside a Claude Code session there is nothing to warm: nothing printed,
/// exit 0.
#[test]
fn the_check_without_a_session_prints_nothing() {
    let w = Warm::new("kw-nosession");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    let out = w.f.without_session(|| w.run(&["keepwarm", "check"]));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

/// A cold write paid while no check ran (the host was dormant) arms the
/// window from the time of the write, as the sensor's loop would have. One
/// whose window has already run out arms nothing.
#[test]
fn a_cold_write_scored_late_arms_from_its_own_time() {
    let w = Warm::new("kw-late-write");
    w.reading(&[(5 * 60 * MIN, 200_000, 500)]);
    assert_eq!(w.check(), "");
    w.reading(&[(5 * 60 * MIN, 200_000, 500), (4 * 60 * MIN, 0, 200_500)]);
    assert_eq!(w.check(), "");
    let status = w.status_line();
    assert!(status.contains("not armed"), "a window that ended an hour ago is not armed: {status}");

    let w = Warm::new("kw-late-write-2");
    w.reading(&[(3 * 60 * MIN, 200_000, 500)]);
    assert_eq!(w.check(), "");
    w.reading(&[(3 * 60 * MIN, 200_000, 500), (20 * MIN, 0, 200_500)]);
    assert_eq!(w.check(), "");
    let status = w.status_line();
    assert!(status.starts_with("keepwarm    armed, 2h40m left") || status.starts_with("keepwarm    armed, 2h39m left"), "{status}");
}

/// With `mods.attend.keepwarm: off` the attend mod never runs `keepwarm
/// check` (the mod's own test holds that), and only the check and `attend
/// run`'s sensor arm keepwarm (ADR-406 item 13). So the verbs the mod does
/// run leave a paid cold write unarmed; the check on the same readings, run
/// afterwards as the control, arms it.
#[test]
fn the_verbs_a_mod_with_keepwarm_off_runs_never_arm_after_a_cold_write() {
    let w = Warm::new("kw-mod-off");
    let mod_verbs: [&[&str]; 6] = [
        &["listen", "on", "--host", "mod"],
        &["message", "inbox", "--drain", "--json"],
        &["message", "unread", "--json"],
        &["peer", "list"],
        &["keepwarm", "status"],
        &["listen", "status", "--json"],
    ];
    w.reading(&[(30 * MIN, 200_000, 500)]);
    for v in mod_verbs {
        assert!(w.run(v).status.success(), "attend {v:?}");
    }
    w.reading(&[(30 * MIN, 200_000, 500), (MIN, 0, 200_500)]);
    for v in mod_verbs {
        assert!(w.run(v).status.success(), "attend {v:?}");
    }
    assert!(w.status_line().contains("not armed"), "nothing armed: {}", w.status_line());

    w.reading(&[(30 * MIN, 200_000, 500)]);
    w.check();
    w.reading(&[(30 * MIN, 200_000, 500), (MIN, 0, 200_500)]);
    w.check();
    assert!(w.status_line().contains("armed, "), "the check arms: {}", w.status_line());
}

/// The clean-slate test's finding 6: `status` says "armed" or "not armed",
/// never the "on" and "off" of the mod's `mods.attend.keepwarm` setting, and
/// in a mod host it shows that setting on a line of its own.
#[test]
fn status_says_armed_and_shows_the_mod_setting_in_a_mod_host() {
    let w = Warm::new("kw-wording");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    let card = |w: &Warm| {
        let out = w.run(&["keepwarm", "status"]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(w.status_line(), "keepwarm    not armed (attend keepwarm on arms it for 6h00m)");
    assert!(!card(&w).contains("mod setting"), "no mod host, no mod line");
    let off = w.run(&["keepwarm", "off"]);
    assert_eq!(String::from_utf8_lossy(&off.stdout), "keepwarm is not armed\n");
    assert!(w.run(&["keepwarm", "on"]).status.success());
    assert!(w.status_line().starts_with("keepwarm    armed, 6h00m left") || w.status_line().starts_with("keepwarm    armed, 5h59m left"), "{}", w.status_line());

    assert!(w.run(&["listen", "on", "--host", "mod"]).status.success());
    std::fs::write(
        w.f.home.join("settings.json"),
        r#"{"view":"effective","keys":{"mods.attend.keepwarm":{"value":"off","default":"on","layer":"project","file":"/p/.claude/ways.yaml"}}}"#,
    )
    .unwrap();
    let text = card(&w);
    let line = text.lines().find(|l| l.starts_with("mod setting")).unwrap_or_else(|| panic!("{text}"));
    assert_eq!(line, "mod setting mods.attend.keepwarm off (from /p/.claude/ways.yaml); the mod never runs the keepwarm check in this session");
}

/// `check --policy --json`: the keepalive policy's decision (ADR-414 item
/// 7), with the autocompact gate the mod read.
fn policy(w: &Warm, gate: &str) -> serde_json::Value {
    // The mod passes the skills the session lists; the shipped wrap is one.
    let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", gate, "--skills", "wrap,handoff,start"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

/// With autocompact off, an idle session of 200k tokens is due its
/// keepalive at 50 idle minutes with nothing armed, counted 1 of 8.
#[test]
fn the_policy_keeps_an_unarmed_session_warm_with_autocompact_off() {
    let w = Warm::new("ka-due");
    w.reading(&[(49 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!(d["action"], "none", "{d}");
    assert_eq!(d["policy"], true);
    let next = d["next_at"].as_u64().unwrap();
    assert!((now() + MIN - 2..=now() + MIN + 2).contains(&next), "due a minute from now: {d}");
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!(d["action"], "keepalive", "{d}");
    assert_eq!((d["count"].as_u64(), d["max"].as_u64()), (Some(1), Some(8)));
    let text = d["text"].as_str().unwrap();
    assert!(text.starts_with("keepwarm: 50m idle") && text.ends_with("Reply with one word and no tools."), "{text}");
    assert_eq!(policy(&w, "off")["action"], "none", "one keepalive per due point");
}

/// Under ADR-182's 50k floor the policy runs and nothing is due: the
/// decision says why in `idle`, which the mod's row shows.
#[test]
fn the_policy_check_says_why_it_is_idle() {
    let w = Warm::new("ka-idle");
    w.reading(&[(51 * MIN, 20_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["next_at"].as_u64(), d["idle"].as_str()), (Some("none"), None, Some("under 50k")), "{d}");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    assert!(policy(&w, "off")["idle"].is_null(), "a due point is not idle");
}

/// With autocompact on, or a gate the mod could not read, nothing is sent.
#[test]
fn the_policy_does_not_run_with_autocompact_on_or_unknown() {
    let w = Warm::new("ka-gate");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    for gate in ["on", "unknown"] {
        let d = policy(&w, gate);
        assert_eq!((d["action"].as_str(), d["policy"].as_bool()), (Some("none"), Some(false)), "{gate}: {d}");
    }
    let out = w.run(&["keepwarm", "status"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("keepalive   off: autocompact could not be read, so it counts as on (armed windows only)"), "{text}");
}

/// The settings bound the policy: `max_keepalives: 0` wraps at the first
/// due point, the wrap carrying where its reply goes.
#[test]
fn the_settings_bound_the_policy_and_route_the_wrap() {
    let w = Warm::new("ka-settings");
    std::fs::write(
        w.f.home.join("settings.json"),
        r#"{"view":"effective","keys":{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.max_keepalives":{"value":0},"mods.attend.continuity.wrap":{"value":"file"},"mods.attend.continuity.wrap_file":{"value":"{state}/wrap/{session}.md"}}}"#,
    )
    .unwrap();
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!(d["action"], "wrap", "{d}");
    assert_eq!(d["wrap"]["dest"], "file");
    let want = format!("{}/wrap/{}.md", w.f.home.join("state").join("agent-ways").display(), w.f.sid);
    assert_eq!(d["wrap"]["file"], want, "under the ways state root");
    assert!(d["text"].as_str().unwrap().starts_with("wrap: "), "{d}");
    assert_eq!(policy(&w, "off")["state"], "wrapped");
    // The mod records where the reply went; the card names it.
    assert!(w.run(&["keepwarm", "wrapped", "file", "--detail", "/tmp/x/s.md"]).status.success());
    let out = w.run(&["keepwarm", "status"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.lines().any(|l| l.starts_with("keepalive   wrapped at ") && l.ends_with("· the reply is saved to /tmp/x/s.md")), "{text}");
    // The operator's prompt starts it again.
    assert!(w.run(&["keepwarm", "reset"]).status.success());
    w.reading(&[(MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["state"].as_str(), d["count"].as_u64()), (Some("running"), Some(0)), "{d}");
}

/// `after_wrap: compact` with keepalives after the wrap: the mod reports the
/// wrap's focus, and the policy's next due point asks for the compaction
/// with it, here at once because no keepalive hours are set.
#[test]
fn the_compaction_after_the_wrap_carries_the_focus_the_mod_reported() {
    let w = Warm::new("ka-compact");
    std::fs::write(
        w.f.home.join("settings.json"),
        r#"{"view":"effective","keys":{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.windows":{"value":[]},"mods.attend.continuity.after_wrap":{"value":"compact"},"mods.attend.continuity.after_wrap_keepalives":{"value":2},"mods.attend.continuity.wrap_skill":{"value":"handoff"}}}"#,
    )
    .unwrap();
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!(d["action"], "wrap", "{d}");
    assert_eq!(d["wrap"]["after"], "compact");
    assert_eq!(d["wrap"]["after_keepalives"], 2);
    assert!(d["text"].as_str().unwrap().contains("Run the handoff skill: square the task list"), "{d}");
    assert!(w.run(&["keepwarm", "wrapped", "transcript", "--focus", "keep the handoff\nand the tasks"]).status.success());
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("compact"), Some("compacted")), "{d}");
    assert_eq!(d["text"], "keep the handoff and the tasks", "one line");
    assert!(w.run(&["keepwarm", "compacted"]).status.success());
    assert_eq!(policy(&w, "off")["action"], "none", "one compaction");
    let card = card_text(&w);
    assert!(card.lines().any(|l| l.starts_with("keepalive   wrapped at ") && l.contains(" · compacted at ")), "{card}");
    assert!(w.run(&["keepwarm", "compacted", "--failed", "a turn was running"]).status.success());
    assert!(card_text(&w).contains("failed: a turn was running"));
    // A report that lands after the operator's prompt reset the ledger
    // marks nothing, so the next stretch after a wrap still runs.
    assert!(w.run(&["keepwarm", "reset"]).status.success());
    assert!(w.run(&["keepwarm", "compacted", "--failed", "late"]).status.success());
    w.reading(&[(50 * MIN, 200_000, 500)]);
    assert_eq!(policy(&w, "off")["action"], "wrap");
    assert!(w.run(&["keepwarm", "wrapped", "transcript", "--focus", "keep it"]).status.success());
    let d = policy(&w, "off");
    assert_eq!(d["action"], "compact", "a stale mark would end the stretch after this wrap: {d}");
    // An unfinished wrap records no focus.
    assert!(w.run(&["keepwarm", "reset"]).status.success());
    assert_eq!(policy(&w, "off")["action"], "wrap");
    assert!(w.run(&["keepwarm", "wrapped", "unfinished", "--detail", "error", "--focus", "keep it"]).status.success());
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("none"), Some("wrapped")), "{d}");
    assert!(d["reason"].as_str().unwrap().contains("no reply to compact with"), "{d}");
}

/// The --skills handshake: a listing without wrap_skill, or one the host
/// could not read, stops at the bound; a host that passes no flag (a mod
/// from before it) keeps its wrap but gets no stretch after it.
#[test]
fn the_skills_flag_gates_the_wrap_and_its_absence_is_the_old_protocol() {
    let run = |tag: &str, flags: &[&str]| {
        let w = Warm::new(tag);
        std::fs::write(
            w.f.home.join("settings.json"),
            r#"{"view":"effective","keys":{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.max_keepalives":{"value":0},"mods.attend.continuity.windows":{"value":[]},"mods.attend.continuity.after_wrap":{"value":"compact"},"mods.attend.continuity.after_wrap_keepalives":{"value":2}}}"#,
        )
        .unwrap();
        w.reading(&[(50 * MIN, 200_000, 500)]);
        let mut args = vec!["keepwarm", "check", "--policy", "--json", "--autocompact", "off"];
        args.extend_from_slice(flags);
        let out = w.run(&args);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let d: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (w, d)
    };
    let (_, d) = run("sk-missing", &["--skills=start"]);
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("none"), Some("stopped")), "{d}");
    assert!(d["reason"].as_str().unwrap().contains("the wrap skill is not among the session's skills"), "{d}");
    let (_, d) = run("sk-unread", &["--skills-unread"]);
    assert!(d["reason"].as_str().unwrap().contains("could not be read"), "{d}");
    let (_, d) = run("sk-partial", &["--skills=start", "--skills-partial"]);
    assert_eq!(d["action"], "wrap", "a partial listing is not held against it: {d}");
    let (w, d) = run("sk-none", &[]);
    assert_eq!(d["action"], "wrap", "{d}");
    assert_eq!(d["wrap"]["after_keepalives"], 0, "the old protocol has no stretch after the wrap: {d}");
    assert!(w.run(&["keepwarm", "wrapped", "transcript", "--focus", "keep it"]).status.success());
    let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", "off"]);
    let d: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_ne!(d["action"], "compact", "no compact action for an old mod: {d}");
    // The card, which reads the settings alone, follows what the check saw.
    let card = card_text(&w);
    assert!(!card.contains("after it"), "{card}");
}

/// `attend keepwarm off` stops the policy until the operator's next prompt.
#[test]
fn keepwarm_off_stops_the_policy_until_the_next_prompt() {
    let w = Warm::new("ka-off");
    w.reading(&[(50 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "off"]).status.success());
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("none"), Some("stopped")), "{d}");
    assert!(w.run(&["keepwarm", "reset"]).status.success());
    assert_eq!(policy(&w, "off")["action"], "keepalive");
}

/// Two checks on one session id at once take turns under the session's
/// lock: the wrap is decided once.
#[test]
fn concurrent_checks_decide_one_wrap() {
    let w = Warm::new("ka-lock");
    std::fs::write(
        w.f.home.join("settings.json"),
        r#"{"view":"effective","keys":{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.max_keepalives":{"value":0}}}"#,
    )
    .unwrap();
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let actions: Vec<String> = std::thread::scope(|s| {
        let runs: Vec<_> = (0..6).map(|_| s.spawn(|| policy(&w, "off")["action"].as_str().unwrap_or_default().to_string())).collect();
        runs.into_iter().map(|r| r.join().unwrap()).collect()
    });
    assert_eq!(actions.iter().filter(|a| *a == "wrap").count(), 1, "{actions:?}");
}

/// Settings ways cannot answer run no policy, and the card says why.
#[test]
fn unreadable_settings_run_no_policy() {
    let w = Warm::new("ka-unreadable");
    std::fs::write(w.f.home.join("settings.json"), "not json").unwrap();
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["policy"].as_bool()), (Some("none"), Some(false)), "{d}");
    let out = w.run(&["keepwarm", "status"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("keepalive   off: the mods.attend settings could not be read (armed windows only)"), "{text}");
}

/// A transcript ways cannot read leaves the policy running: the host is
/// told to ask again.
#[test]
fn an_unreadable_transcript_keeps_the_policy_and_asks_again() {
    let w = Warm::new("ka-noreading");
    let d = policy(&w, "off");
    assert_eq!((d["policy"].as_bool(), d["retry"].as_bool(), d["state"].as_str()), (Some(true), Some(true), Some("running")), "{d}");
    assert!(d["next_at"].as_u64().unwrap() > now());
}

/// `reset --at` records the prompt the mod held while autocompact was on.
#[test]
fn reset_at_records_a_held_prompt() {
    let w = Warm::new("ka-reset-at");
    let at = now() - 30 * MIN;
    assert!(w.run(&["keepwarm", "reset", "--at", &at.to_string()]).status.success());
    let ledger: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(w.ledger()).unwrap()).unwrap();
    assert_eq!(ledger["policy"]["operator_at"].as_u64(), Some(at));
}

/// The policy check keeps its settings read a few minutes, so a session
/// that asks every minute does not run ways settings every minute.
#[test]
fn the_policy_check_keeps_its_settings_read() {
    let w = Warm::new("ka-settings-kept");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    for _ in 0..3 {
        policy(&w, "on");
    }
    let calls = std::fs::read_to_string(w.f.home.join("ways-calls.log")).unwrap_or_default();
    assert_eq!(calls.lines().filter(|l| *l == "settings").count(), 1, "{calls}");
}

/// `attend keepwarm wrapped --save` with `reply` on stdin.
fn save(w: &Warm, outcome: &str, path: &std::path::Path, detail: &str) -> std::process::Output {
    save_bytes(w, outcome, path, detail, b"Continuation prompt.")
}

fn save_bytes(w: &Warm, outcome: &str, path: &std::path::Path, detail: &str, reply: &[u8]) -> std::process::Output {
    use std::io::Write;
    let mut path_env = OsString::from(&w.bin);
    path_env.push(":");
    path_env.push(std::env::var_os("PATH").unwrap_or_default());
    let mut cmd = w.f.command();
    cmd.env("PATH", path_env).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    cmd.args(["keepwarm", "wrapped", outcome, "--detail", detail, "--path", &path.to_string_lossy(), "--save"]);
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let reply = reply.to_vec();
    let feeder = std::thread::spawn(move || {
        stdin.write_all(&reply).ok();
    });
    let out = child.wait_with_output().unwrap();
    feeder.join().unwrap();
    out
}

fn card_text(w: &Warm) -> String {
    String::from_utf8_lossy(&w.run(&["keepwarm", "status"]).stdout).into_owned()
}

/// attend writes the wrap's reply itself, checking the path again: under
/// the state root's wrap/, the session in its name, through no symlink.
#[test]
fn wrapped_save_writes_the_reply_under_the_state_root_and_refuses_a_link() {
    let w = Warm::new("ka-save");
    std::fs::write(
        w.f.home.join("settings.json"),
        r#"{"view":"effective","keys":{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.max_keepalives":{"value":0}}}"#,
    )
    .unwrap();
    w.reading(&[(50 * MIN, 200_000, 500)]);
    assert_eq!(policy(&w, "off")["action"], "wrap");
    let wrap = w.f.home.join("state").join("agent-ways").join("wrap");
    let file = wrap.join(format!("{}.md", w.f.sid));
    let out = save(&w, "file", &file, "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Continuation prompt.");
    assert!(card_text(&w).contains(&format!("the reply is saved to {}", file.display())));
    // A refused send falls back to the file; the card names both.
    std::fs::remove_file(&file).unwrap();
    let out = save(&w, "refused", &file, "Chaucer-beta: no live session");
    assert!(out.status.success());
    assert!(card_text(&w).contains(&format!("handoff refused: Chaucer-beta: no live session; saved to {}", file.display())));
    // A link planted at the path is refused, and nothing is written through it.
    std::fs::remove_file(&file).unwrap();
    let victim = w.f.home.join("victim");
    std::fs::write(&victim, "keep").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&victim, &file).unwrap();
        let out = save(&w, "file", &file, "");
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert!(card_text(&w).contains("the wrap did not finish (could not write"), "{}", card_text(&w));
    }
    // A reply over 1 MiB is refused and nothing is written.
    let big = vec![b'x'; 1024 * 1024 + 1];
    std::fs::remove_file(&file).ok();
    let out = save_bytes(&w, "file", &file, "", &big);
    assert_eq!(out.status.code(), Some(1));
    assert!(!file.exists() || std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
    assert!(card_text(&w).contains("over 1048576 bytes"), "{}", card_text(&w));
    // --save is for file and refused only.
    let out = save(&w, "sent", &file, "aaron");
    assert_eq!(out.status.code(), Some(2));
    // Outside the wrap directory: refused.
    let out = save(&w, "file", &w.f.home.join(format!("{}.md", w.f.sid)), "");
    assert_eq!(out.status.code(), Some(1));
}

/// The kept settings read is dropped when a settings file it reads from
/// is written, created or deleted.
#[test]
fn the_kept_settings_read_follows_its_files() {
    let w = Warm::new("ka-settings-files");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    let reads = |w: &Warm| std::fs::read_to_string(w.f.home.join("ways-calls.log")).unwrap_or_default().lines().filter(|l| *l == "settings").count();
    policy(&w, "off");
    policy(&w, "off");
    assert_eq!(reads(&w), 1);
    let user = w.f.home.join("config").join("agent-ways").join("config.yaml");
    std::fs::create_dir_all(user.parent().unwrap()).unwrap();
    std::fs::write(&user, "mods:\n  attend:\n    keepwarm: on\n").unwrap();
    policy(&w, "off");
    assert_eq!(reads(&w), 2, "a created user file is a change");
    policy(&w, "off");
    assert_eq!(reads(&w), 2);
    std::fs::remove_file(&user).unwrap();
    policy(&w, "off");
    assert_eq!(reads(&w), 3, "a deleted one is too");
}

/// A check removes other sessions' kept settings reads and lock files once
/// their ledger is gone or stale, and keeps a live session's.
#[test]
fn a_check_prunes_other_sessions_stale_files() {
    let w = Warm::new("ka-prune");
    w.reading(&[(30 * MIN, 200_000, 500)]);
    let dir = w.f.cache().join("keepwarm");
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["gone.settings.json", "gone.background.json", "gone.lock", "live.settings.json", "live.background.json", "live.lock", "live.ledger.json"] {
        std::fs::write(dir.join(f), "{}").unwrap();
    }
    // A lock another process holds is never pruned, ledger or not.
    let held = std::fs::OpenOptions::new().write(true).open(dir.join("gone.lock")).unwrap();
    held.lock().unwrap();
    policy(&w, "off");
    assert!(!dir.join("gone.settings.json").exists());
    assert!(!dir.join("gone.background.json").exists(), "a stale Stop record goes with the settings read");
    assert!(dir.join("gone.lock").exists(), "a lock file is never pruned");
    assert!(dir.join("live.settings.json").exists() && dir.join("live.background.json").exists() && dir.join("live.lock").exists());
    drop(held);
}

/// Relay mode through the CLI: a fill past the hard line sends a wrap
/// naming its id; the mod reports the wrap's marker; with the handoff the
/// skill wrote, the check asks for a compaction with the handoff and a
/// resume naming the file; `resumed` counts the relay; `wrap-id` gives the
/// id to the PreCompact hook.
///
/// References:
/// - ADR-417 items 2 to 5: the relay's fill trigger, compaction, resume and
///   count.
#[test]
fn a_relay_wraps_on_fill_compacts_with_the_handoff_and_resumes() {
    let w = Warm::new("ka-relay");
    w.settings(r#"{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.relay":{"value":"on"}}"#);
    let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(&w.f.origin).status().unwrap();
    assert!(init.success());
    w.reading(&[(MIN, 700_000, 500)]);
    assert_eq!(w.run(&["keepwarm", "wrap-id"]).status.code(), Some(1), "no wrap recorded yet");
    let fill = |tokens: &str| {
        let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", "on", "--skills", "wrap", "--fill", tokens, "--window", "1000000", "--native-at", "900000"]);
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let d = fill("700000");
    assert_eq!((d["action"].as_str(), d["relay"].as_bool(), d["relay_at_tokens"].as_u64()), (Some("none"), Some(true), Some(750_000)), "{d}");
    // The drain is offered until the mod reports its turn went in.
    for tokens in ["760000", "770000"] {
        let d = fill(tokens);
        assert_eq!((d["action"].as_str(), d["draining"].as_bool()), (Some("drain"), Some(true)), "{d}");
    }
    assert!(w.run(&["keepwarm", "drained"]).status.success());
    let d = fill("780000");
    assert_eq!((d["action"].as_str(), d["draining"].as_bool()), (Some("none"), Some(true)), "{d}");
    let d = fill("860000");
    assert_eq!(d["action"], "wrap", "{d}");
    let id = d["wrap_id"].as_str().unwrap().to_string();
    assert!(d["text"].as_str().unwrap().starts_with("wrap: context at 86% (860k of 1M), past the hard line of 85%.\n"), "{d}");
    assert_eq!(w.run(&["keepwarm", "wrap-id"]).stdout, format!("{id}\n").into_bytes());
    // The skill wrote the handoff with the id; the mod reports the marker.
    let handoff = std::path::Path::new(&w.f.origin).join(".claude").join("handoff.wrap.md");
    std::fs::create_dir_all(handoff.parent().unwrap()).unwrap();
    std::fs::write(&handoff, format!("2026-11-26 e6bbfbdd wrap={id}\nContinue work.\n\nTASKS — one.\n\nDO NEXT — start.\n")).unwrap();
    let marker = format!("relay: continue tasks=1 goal=none wrap={id} head=e6bbfbdd");
    assert!(w.run(&["keepwarm", "wrapped", "transcript", "--marker", &marker]).status.success());
    let d = policy(&w, "on");
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("compact"), Some("compacted")), "{d}");
    assert!(d["text"].as_str().unwrap().contains(&format!("wrap={id}\nContinue work.")), "{d}");
    let resume = d["resume"].as_str().unwrap();
    assert!(resume.contains("this is relay 1 of 5") && resume.contains(".claude/handoff.wrap.md. The task list"), "{resume}");
    assert!(w.run(&["keepwarm", "compacted"]).status.success());
    assert!(w.run(&["keepwarm", "resumed"]).status.success());
    let d = policy(&w, "on");
    assert_eq!((d["state"].as_str(), d["relay_count"].as_u64()), (Some("resumed"), Some(1)), "{d}");
    let card = card_text(&w);
    assert!(card.contains("relay       on · 1 of 5"), "{card}");
    // A resumed turn's stop line stops the relay.
    assert!(w.run(&["keepwarm", "stopped", "decision", "--detail", "which database"]).status.success());
    let d = policy(&w, "on");
    assert_eq!((d["state"].as_str(), d["relay_stop"].as_str()), (Some("stopped"), Some("decision")), "{d}");
    assert_eq!(w.run(&["keepwarm", "wrap-id"]).stdout, format!("{id}\n").into_bytes(), "the id outlives the stop");
}

/// A relay wrap through the CLI up to the point its marker is judged.
/// Returns the wrap's id and the handoff file.
fn relay_wrap_judged(w: &Warm, marker_head: &str) -> (String, PathBuf) {
    w.settings(r#"{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.relay":{"value":"on"}}"#);
    let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(&w.f.origin).status().unwrap();
    assert!(init.success());
    w.reading(&[(MIN, 860_000, 500)]);
    let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", "off", "--skills", "wrap", "--fill", "860000", "--window", "1000000"]);
    let d: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["action"], "wrap", "{d}");
    let id = d["wrap_id"].as_str().unwrap().to_string();
    let handoff = PathBuf::from(&w.f.origin).join(".claude").join("handoff.wrap.md");
    std::fs::create_dir_all(handoff.parent().unwrap()).unwrap();
    std::fs::write(&handoff, format!("2026-11-26 e6bbfbdd wrap={id}\nContinue work.\n\nTASKS — one.\n\nDO NEXT — start.\n")).unwrap();
    let marker = format!("relay: {marker_head} tasks=1 goal=none wrap={id} head=e6bbfbdd");
    assert!(w.run(&["keepwarm", "wrapped", "transcript", "--marker", &marker]).status.success());
    (id, handoff)
}

/// A relay wrap's compaction, then the operator's `/wrap handoff`, whose
/// skill records its own id: `wrap-id` gives the PreCompact hook the
/// handoff wrap's id, not the relay wrap's.
///
/// References:
/// - ADR-417 item 3: the PreCompact hook asks attend for the last wrap's id.
#[test]
fn a_handoff_mode_wrap_after_a_relay_wrap_is_the_last_wrap() {
    let w = Warm::new("ka-relay-then-handoff");
    let (a, _) = relay_wrap_judged(&w, "continue");
    assert_eq!(policy(&w, "off")["action"], "compact");
    assert!(w.run(&["keepwarm", "compacted"]).status.success());
    assert!(w.run(&["keepwarm", "resumed"]).status.success());
    assert!(w.run(&["keepwarm", "reset"]).status.success());
    assert_eq!(w.run(&["keepwarm", "wrap-id"]).stdout, format!("{a}\n").into_bytes());
    // `/wrap handoff`: the skill records its id.
    let b = "20261126T2210Z-s1abcdef";
    assert!(w.run(&["keepwarm", "wrap-id", "--record", b]).status.success());
    assert_eq!(w.run(&["keepwarm", "wrap-id"]).stdout, format!("{b}\n").into_bytes());
    assert_eq!(w.run(&["keepwarm", "wrap-id", "--record", "no good"]).status.code(), Some(2));
    assert_eq!(w.run(&["keepwarm", "wrap-id"]).stdout, format!("{b}\n").into_bytes(), "a bad id records nothing");
}

/// The compaction attend asked for is not recorded until the mod reports
/// it ran: a check after a deferral asks for it again. Once it ran, a check
/// that hears no resume stops the relay and hands over the handoff.
///
/// References:
/// - ADR-417 item 4: a turn before the compaction defers it; a resume that
///   does not go in stops the relay.
#[test]
fn a_deferred_relay_compaction_is_asked_for_again_and_a_lost_resume_stops() {
    let w = Warm::new("ka-relay-defer");
    let (id, _) = relay_wrap_judged(&w, "continue");
    let first = policy(&w, "off");
    assert_eq!((first["action"].as_str(), first["state"].as_str()), (Some("compact"), Some("compacted")), "{first}");
    let again = policy(&w, "off");
    assert_eq!((again["action"].as_str(), again["text"].as_str(), again["resume"].as_str()), (Some("compact"), first["text"].as_str(), first["resume"].as_str()), "{again}");
    assert!(w.run(&["keepwarm", "compacted"]).status.success());
    // The mod that compacted reloaded before its resume.
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str(), d["relay_stop"].as_str(), d["send_handoff"].as_bool()), (Some("none"), Some("stopped"), Some("resume lost"), Some(true)), "{d}");
    assert_eq!(d["text"].as_str(), Some(format!("2026-11-26 e6bbfbdd wrap={id}\nContinue work.\n\nTASKS — one.\n\nDO NEXT — start.").as_str()), "{d}");
    assert!(w.run(&["keepwarm", "resumed"]).status.success());
    let d = policy(&w, "off");
    assert_eq!((d["state"].as_str(), d["relay_count"].as_u64(), d["send_handoff"].as_bool()), (Some("stopped"), Some(0), Some(false)), "a late resume report counts nothing: {d}");
}

/// A repository at the fixture's project with the handoff the wrap `id`
/// wrote, or none.
fn handoff_for(w: &Warm, id: Option<&str>) -> PathBuf {
    let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(&w.f.origin).status().unwrap();
    assert!(init.success());
    let handoff = PathBuf::from(&w.f.origin).join(".claude").join("handoff.wrap.md");
    if let Some(id) = id {
        std::fs::create_dir_all(handoff.parent().unwrap()).unwrap();
        std::fs::write(&handoff, format!("2026-11-27 e6bbfbdd wrap={id}\nContinue work.\n\nDO NEXT — start.\n")).unwrap();
    }
    handoff
}

fn judged(w: &Warm, id: &str, verdict: &str) -> std::process::Output {
    w.run(&["keepwarm", "judged", "--wrap", id, "--verdict", verdict, "--handoff", "sufficient", "--own", "stop", "--reason", "the release landed"])
}

/// `judged --verdict stop` with no handoff, or one whose first line
/// carries another id, is refused with exit 1 and records nothing.
///
/// References:
/// - ADR-418 item 13: the mod refuses to arm the exit until the handoff
///   exists.
#[test]
fn judged_stop_without_the_wrap_s_handoff_is_refused() {
    let w = Warm::new("kw-judged-refused");
    let id = "20261127T0900Z-3f9a1c2e";
    handoff_for(&w, None);
    let out = judged(&w, id, "stop");
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("keepwarm judged refused: no handoff"), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!w.ledger().exists(), "nothing recorded");
    handoff_for(&w, Some("20261127T1000Z-0badc0de"));
    let out = judged(&w, id, "stop");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("does not carry wrap={id}")), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!w.ledger().exists(), "nothing recorded");
}

/// The armed exit end to end: `judged --verdict stop` arms it, the first
/// due point with nothing pending returns `finish` and attend marks the
/// session finished in the same process, and `attend keepwarm reset`, the
/// operator's prompt, clears both.
///
/// References:
/// - ADR-418 items 12 and 13: the finished mark, and an operator prompt
///   that disarms.
#[test]
fn an_armed_exit_finishes_the_session_and_reset_clears_it() {
    let w = Warm::new("kw-armed-exit");
    let id = "20261127T0900Z-3f9a1c2e";
    handoff_for(&w, Some(id));
    assert!(w.run(&["listen", "on"]).status.success());
    let out = judged(&w, id, "stop");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with(&format!("exit armed for wrap {id}: ")), "{}", String::from_utf8_lossy(&out.stdout));
    let finished = || {
        let out = w.run(&["listen", "status", "--json"]);
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        v["finished"].is_u64()
    };
    w.reading(&[(30 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str(), d["exit"]["state"].as_str()), (Some("none"), Some("armed"), Some("armed")), "{d}");
    assert!(!finished());
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str(), d["text"].as_str()), (Some("finish"), Some("finished"), None), "{d}");
    assert!(finished(), "the check that returns finish sets the mark");
    let status = w.run(&["keepwarm", "status"]);
    let card = String::from_utf8_lossy(&status.stdout);
    assert!(card.lines().any(|l| l.starts_with("exit        finished ")), "{card}");
    // The mod's read of the exit, for a process that resumes the session.
    let exit = || -> serde_json::Value { serde_json::from_slice::<serde_json::Value>(&w.run(&["keepwarm", "status", "--json"]).stdout).unwrap()["exit"].clone() };
    assert_eq!((exit()["state"].as_str(), exit()["wrap_id"].as_str()), (Some("finished"), Some(id)), "{}", exit());
    let out = w.run(&["keepwarm", "reset"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "keepalive count reset\nthe session is no longer finished\n");
    assert!(!finished(), "reset clears the mark");
    assert!(exit().is_null(), "reset clears the exit: {}", exit());
    let d = policy(&w, "off");
    assert!(d["exit"].is_null(), "reset clears the exit: {d}");
    assert_eq!(d["action"], "keepalive", "the next due point pings as today: {d}");
}

/// A session that is not listening finishes with no finished mark, and the
/// operator's prompt leaves it not listening: only `attend listen on`
/// starts listening.
#[test]
fn a_session_not_listening_finishes_unmarked_and_reset_does_not_listen() {
    let w = Warm::new("kw-exit-unlistened");
    let id = "20261127T0900Z-3f9a1c2e";
    handoff_for(&w, Some(id));
    let listen = || -> serde_json::Value { serde_json::from_slice(&w.run(&["listen", "status", "--json"]).stdout).unwrap() };
    assert_eq!(listen()["listening"], false);
    assert!(judged(&w, id, "stop").status.success());
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("finish"), Some("finished")), "{d}");
    assert!(listen()["finished"].is_null(), "no mark on a session that is not listening: {}", listen());
    let out = w.run(&["keepwarm", "reset"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "keepalive count reset\n");
    assert_eq!((listen()["listening"].as_bool(), listen()["finished"].is_null()), (Some(false), true), "{}", listen());
}

/// With relay off and autocompact on, the default, the exit still finishes
/// at its due point; with keepwarm off it does nothing, and `judged` says so.
///
/// References:
/// - ADR-418 item 14, item 2(c): after a `stop` the idle path runs the
///   armed exit.
#[test]
fn a_stop_exit_runs_with_autocompact_on_and_says_when_keepwarm_is_off() {
    let w = Warm::new("kw-exit-autocompact");
    let id = "20261127T0900Z-3f9a1c2e";
    handoff_for(&w, Some(id));
    assert!(judged(&w, id, "stop").status.success());
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "on");
    assert_eq!((d["action"].as_str(), d["state"].as_str(), d["policy"].as_bool()), (Some("finish"), Some("finished"), Some(false)), "{d}");
    let w = Warm::new("kw-exit-keepwarm-off");
    w.settings(r#"{"mods.attend.keepwarm":{"value":"off"}}"#);
    handoff_for(&w, Some(id));
    let out = judged(&w, id, "stop");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("exit armed for wrap {id}, but mods.attend.keepwarm is off: no tick runs, so the exit does nothing while it is off\n")
    );
}

/// The Stop hook's background work, through the drain: a running shell and
/// a monitor keep an armed exit pinging with no `--pending` from the host,
/// a `--pending` that names them wins, and a Stop that lists nothing lets
/// the exit finish.
///
/// References:
/// - ADR-418 item 5: shells and watchers in flight are pending.
#[test]
fn the_stop_hook_s_background_work_keeps_an_armed_exit_pinging() {
    const RUNNING: &str = r#"{"hook_event_name":"Stop","stop_hook_active":false,"background_tasks":[{"id":"b1","type":"shell","status":"running","description":"suite","command":"cargo test"},{"id":"m1","type":"monitor","status":"running","description":"watch CI"}]}"#;
    let id = "20261127T0900Z-3f9a1c2e";
    let armed = |tag: &str, payload: &str| {
        let w = Warm::new(tag);
        handoff_for(&w, Some(id));
        assert!(judged(&w, id, "stop").status.success());
        w.f.drain_hook_with(payload);
        w.reading(&[(50 * MIN, 200_000, 500)]);
        w
    };
    let w = armed("kw-exit-bg-running", RUNNING);
    let d = policy(&w, "off");
    assert_eq!(d["action"], "keepalive", "{d}");
    assert!(d["reason"].as_str().unwrap().contains("1 shell, 1 watcher"), "{d}");
    let w = armed("kw-exit-bg-named", RUNNING);
    let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", "off", "--skills", "wrap", "--pending", "agents=0,shells=0,watchers=0"]);
    let d: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["action"], "finish", "the host's counts win: {d}");
    let w = armed("kw-exit-bg-empty", r#"{"hook_event_name":"Stop","stop_hook_active":false,"background_tasks":[]}"#);
    let d = policy(&w, "off");
    assert_eq!(d["action"], "finish", "{d}");
}

/// `compacted --trigger boundary` while a relay wrap waits is refused with
/// exit 1, writes nothing, and says why.
#[test]
fn a_refused_boundary_compaction_exits_1_and_writes_nothing() {
    let w = Warm::new("kw-boundary-refused");
    relay_wrap_judged(&w, "continue");
    let before = std::fs::read(w.ledger()).unwrap();
    let out = w.run(&["keepwarm", "compacted", "--trigger", "boundary"]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "keepwarm: a relay wrap is pending, so its compaction is the relay's; the boundary compaction is not recorded\n"
    );
    assert_eq!(std::fs::read(w.ledger()).unwrap(), before, "the ledger is not written");
    let w = Warm::new("kw-boundary-taken");
    assert!(w.run(&["keepwarm", "compacted", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "resumed", "--trigger", "boundary"]).status.success());
}

/// A verdict other than stop is recorded and arms nothing; a malformed
/// `--pending` is refused by the parser.
#[test]
fn judged_continue_arms_nothing_and_pending_is_checked() {
    let w = Warm::new("kw-judged-continue");
    let id = "20261127T0900Z-3f9a1c2e";
    handoff_for(&w, Some(id));
    let out = judged(&w, id, "continue");
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("judged wrap {id}: continue, the handoff sufficient; no exit is armed\n"));
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["exit"]["state"].as_str()), (Some("keepalive"), Some("continue")), "{d}");
    let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", "off", "--pending", "pets=1"]);
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
}

const WRAP: &str = "20261127T0900Z-3f9a1c2e";

/// `attend keepwarm judged` for [`WRAP`] with the judge's `verdict` and the
/// mod's `move`, where one is given, and any `extra` flags.
fn judged_move(w: &Warm, verdict: &str, moved: Option<&str>, extra: &[&str]) -> std::process::Output {
    let mut args = vec!["keepwarm", "judged", "--wrap", WRAP, "--verdict", verdict, "--handoff", "sufficient", "--own", "continue", "--reason", "the release landed"];
    if let Some(m) = moved {
        args.extend(["--move", m]);
    }
    args.extend(extra);
    let out = w.run(&args);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    out
}

/// `attend keepwarm status --json`, and any `extra` flags.
fn status_json(w: &Warm, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["keepwarm", "status", "--json"];
    args.extend(extra);
    let out = w.run(&args);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The last boundary as `status --json` shows it, for a judge's `verdict`
/// carried out as `moved`.
fn boundary(verdict: &str, moved: &str) -> serde_json::Value {
    serde_json::json!({ "wrap": WRAP, "verdict": verdict, "handoff": "sufficient", "own": "continue", "move": moved, "reason": "the release landed" })
}

/// With no `--move`, the move is the verdict, as hand-run calls from
/// before the flag expect: a stop arms, a continue arms nothing.
#[test]
fn the_move_defaults_to_the_verdict() {
    let w = Warm::new("kw-move-default");
    handoff_for(&w, Some(WRAP));
    judged_move(&w, "stop", None, &[]);
    let s = status_json(&w, &[]);
    assert_eq!(s["boundary"], boundary("stop", "stop"), "{s}");
    assert_eq!((s["exit"]["state"].as_str(), s["exit"]["move"].as_str()), (Some("armed"), Some("stop")), "{s}");
    judged_move(&w, "continue", None, &[]);
    let s = status_json(&w, &[]);
    assert_eq!(s["boundary"], boundary("continue", "continue"), "{s}");
    assert_eq!(s["exit"]["state"], "continue", "{s}");
}

/// A judge's `compact` carried out as `stop`, as with relay off, arms the
/// exit, and the record keeps the verdict beside the move. The first due
/// point with nothing pending finishes.
///
/// References:
/// - ADR-418 item 12: the stop row arms the exit.
#[test]
fn a_compact_verdict_carried_out_as_stop_arms_and_keeps_both() {
    let w = Warm::new("kw-move-compact-stop");
    handoff_for(&w, Some(WRAP));
    let out = judged_move(&w, "compact", Some("stop"), &[]);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with(&format!("exit armed for wrap {WRAP}: ")), "{}", String::from_utf8_lossy(&out.stdout));
    let s = status_json(&w, &[]);
    assert_eq!(s["boundary"], boundary("compact", "stop"), "{s}");
    let e = &s["exit"];
    assert_eq!(
        (e["state"].as_str(), e["wrap_id"].as_str(), e["verdict"].as_str(), e["own"].as_str(), e["move"].as_str()),
        (Some("armed"), Some(WRAP), Some("compact"), Some("continue"), Some("stop")),
        "{s}"
    );
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["exit"]["move"].as_str()), (Some("finish"), Some("stop")), "{d}");
}

/// Your own `continue` against the judge's `stop` continues: the move arms
/// nothing, and the due point pings as today.
#[test]
fn a_continue_move_arms_nothing_against_a_stop_verdict() {
    let w = Warm::new("kw-move-continue");
    handoff_for(&w, Some(WRAP));
    let out = judged_move(&w, "stop", Some("continue"), &[]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("judged wrap {WRAP}: stop, carried out continue, the handoff sufficient; no exit is armed\n")
    );
    let s = status_json(&w, &[]);
    assert_eq!(s["boundary"], boundary("stop", "continue"), "{s}");
    assert_eq!(s["exit"]["state"], "continue", "{s}");
    w.reading(&[(50 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["state"].as_str()), (Some("keepalive"), Some("running")), "{d}");
}

/// Recording the same wrap again replaces its record: the mod records
/// `--move compact`, and once no tasks remain after the compaction,
/// `--move stop`, which then arms. The ledger holds one record.
#[test]
fn recording_the_wrap_again_replaces_its_record() {
    let w = Warm::new("kw-move-again");
    w.settings(RELAY_ON);
    handoff_for(&w, Some(WRAP));
    judged_move(&w, "compact", Some("compact"), &[]);
    assert_eq!(status_json(&w, &[])["exit"]["state"], "compact");
    assert!(w.run(&["keepwarm", "compacted", "--trigger", "boundary"]).status.success());
    judged_move(&w, "compact", Some("stop"), &[]);
    let s = status_json(&w, &[]);
    assert_eq!(s["boundary"], boundary("compact", "stop"), "{s}");
    assert_eq!(s["exit"]["state"], "armed", "{s}");
    let ledger: serde_json::Value = serde_json::from_slice(&std::fs::read(w.ledger()).unwrap()).unwrap();
    let exit = &ledger["policy"]["exit"];
    assert!(exit.is_object(), "one record: {ledger}");
    assert_eq!((exit["verdict"].as_str(), exit["move"].as_str(), exit["pings"].as_u64()), (Some("compact"), Some("stop"), Some(0)), "{exit}");
}

/// `judged --json` prints one object with every field present: whether it
/// recorded, armed and will be acted on, the move recorded, why a compact
/// was refused, and for `doubt` the route its handoff takes, with a
/// `handoff:` line naming the boundary judge and its reason. With relay
/// off the line does not say the relay stopped, and a doubt does not
/// compact, so it carries no instructions. A stop has no route. A refusal
/// prints no JSON.
///
/// References:
/// - ADR-418 item 11: in relay mode, `doubt` becomes `relay: stop doubt`,
///   and the handoff goes to the operator over attend.
#[test]
fn judged_json_gives_the_doubt_handoff_s_route() {
    let w = Warm::new("kw-judged-json");
    handoff_for(&w, Some(WRAP));
    let v = judged_json(&w, "doubt", None, &[]);
    let r = v["wrap"].clone();
    assert_eq!(r["to"], "fixture-human", "an empty wrap_to falls back to the operator: {v}");
    let line = r["handoff"].as_str().unwrap();
    assert!(line.starts_with("handoff: continuation prompt from "), "{line}");
    assert!(line.ends_with(" while its operator was away. The boundary judge said doubt, the release landed."), "{line}");
    for key in ["dest", "file", "after", "after_keepalives", "notes"] {
        assert!(r.get(key).is_some(), "the route carries {key}: {v}");
    }
    assert_eq!(
        v,
        serde_json::json!({ "recorded": true, "armed": true, "acted_on": true, "move": "doubt", "compact_refused": null, "wrap": r, "instructions": null, "resume": null })
    );
    let v = judged_json(&w, "compact", Some("stop"), &[]);
    assert_eq!(
        v,
        serde_json::json!({ "recorded": true, "armed": true, "acted_on": true, "move": "stop", "compact_refused": null, "wrap": null, "instructions": null, "resume": null })
    );
    let v = judged_json(&w, "stop", Some("continue"), &[]);
    assert_eq!(
        v,
        serde_json::json!({ "recorded": true, "armed": false, "acted_on": false, "move": "continue", "compact_refused": null, "wrap": null, "instructions": null, "resume": null })
    );
    let v = judged_json(&w, "compact", Some("compact"), &[]);
    assert_eq!((v["move"].as_str(), v["compact_refused"].as_str(), v["armed"].as_bool()), (Some("stop"), Some("relay is off"), Some(true)), "{v}");
    let w = Warm::new("kw-judged-json-refused");
    handoff_for(&w, None);
    let out = w.run(&["keepwarm", "judged", "--wrap", WRAP, "--verdict", "doubt", "--handoff", "sufficient", "--own", "stop", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(String::from_utf8_lossy(&out.stderr).contains("keepwarm judged refused: no handoff"), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The settings with keepalive and relay on.
const RELAY_ON: &str = r#"{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.relay":{"value":"on"}}"#;

/// `judged --json` for [`WRAP`], parsed.
fn judged_json(w: &Warm, verdict: &str, moved: Option<&str>, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--json"];
    args.extend(extra);
    let out = judged_move(w, verdict, moved, &args);
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

/// A handoff for [`WRAP`] over the compaction's cap, with its DO NEXT
/// last, after a long STATE section.
fn long_handoff(w: &Warm) -> PathBuf {
    let handoff = handoff_for(w, None);
    std::fs::create_dir_all(handoff.parent().unwrap()).unwrap();
    let state = "- the parser keeps its old grammar for one release, and the tests pin it.\n".repeat(200);
    let text = format!("2026-11-27 e6bbfbdd wrap={WRAP}\nContinue work on the parser.\n\nTASKS — recreate these.\n#1 [OPEN] port the grammar\n\nSTATE — where it stands.\n{state}\nDO NEXT — start #1 from the grammar file.\n");
    std::fs::write(&handoff, text).unwrap();
    handoff
}

/// With relay on and room, a compact is recorded as compact, arms
/// nothing, and gives the compaction's instructions, built as the relay
/// builds them, and the boundary resume. A long handoff keeps its DO NEXT,
/// and the resume keeps the relay's escape.
///
/// References:
/// - ADR-417 item 4: a relay compaction takes its instructions from the
///   handoff.
/// - ADR-418 item 12: the compact row.
#[test]
fn a_compact_with_room_gives_the_instructions_and_the_resume() {
    let w = Warm::new("kw-judged-compact");
    w.settings(RELAY_ON);
    let path = long_handoff(&w);
    let v = judged_json(&w, "compact", Some("compact"), &[]);
    let (instructions, resume) = (v["instructions"].as_str().unwrap().to_string(), v["resume"].as_str().unwrap().to_string());
    assert_eq!(
        v,
        serde_json::json!({ "recorded": true, "armed": false, "acted_on": false, "move": "compact", "compact_refused": null, "wrap": null, "instructions": instructions, "resume": resume })
    );
    assert!(instructions.starts_with("Summarize this session as the handoff below; keep its TASKS and DO NEXT whole.\n\n2026-11-27 e6bbfbdd wrap="), "{instructions}");
    assert!(instructions.chars().count() <= 10_000, "within the cap: {}", instructions.chars().count());
    assert!(instructions.contains("#1 [OPEN] port the grammar"), "TASKS kept");
    assert!(instructions.contains("DO NEXT — start #1 from the grammar file."), "DO NEXT kept: {instructions}");
    // attend names the handoff under the repository root git resolves, which
    // on macOS is the temp dir's real path (/private/var/... for /var/...).
    let path = std::fs::canonicalize(&path).unwrap();
    assert!(instructions.ends_with(&format!("(The handoff continues in {}; read it there.)", path.display())), "{instructions}");
    assert!(resume.starts_with("relay: the conversation was compacted at a work boundary; this is relay 1 of 5 since the operator's last prompt."), "{resume}");
    assert!(resume.contains("relay: stop decision or relay: stop doubt"), "{resume}");
    assert!(resume.ends_with("run /wrap boundary."), "{resume}");
}

/// With no room a compact is recorded as stop, which arms, and the JSON
/// says why: the relay stopped on `decision`, a relay wrap pending,
/// `max_relays` reached, the operator in the turn. No instructions or
/// resume go with it.
///
/// References:
/// - ADR-418 item 10: the `max_relays` cap ends the chain.
/// - ADR-418 item 13: no self-compaction while the operator is talking.
#[test]
fn a_compact_without_room_is_recorded_as_stop_with_the_reason() {
    let refused = |w: &Warm, extra: &[&str]| {
        let v = judged_json(w, "compact", Some("compact"), extra);
        assert_eq!((v["move"].as_str(), v["armed"].as_bool(), v["acted_on"].as_bool()), (Some("stop"), Some(true), Some(true)), "{v}");
        assert_eq!((v["instructions"].clone(), v["resume"].clone(), v["wrap"].clone()), (serde_json::Value::Null, serde_json::Value::Null, serde_json::Value::Null), "{v}");
        assert_eq!(status_json(w, &[])["boundary"]["move"], "stop");
        v["compact_refused"].as_str().unwrap().to_string()
    };
    // One boundary relay, then its resumed turn's `relay: stop decision`.
    let w = Warm::new("kw-judged-stopped");
    w.settings(RELAY_ON);
    handoff_for(&w, Some(WRAP));
    let v = judged_json(&w, "compact", Some("compact"), &[]);
    assert_eq!(v["compact_refused"], serde_json::Value::Null, "{v}");
    assert!(w.run(&["keepwarm", "compacted", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "resumed", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "stopped", "decision", "--detail", "which database"]).status.success());
    assert_eq!(refused(&w, &[]), "the relay stopped: decision");
    // A relay wrap waits for its answer.
    let w = Warm::new("kw-judged-pending");
    let (id, _) = relay_wrap_judged(&w, "continue");
    let out = w.run(&["keepwarm", "judged", "--json", "--wrap", &id, "--verdict", "compact", "--handoff", "sufficient", "--own", "compact", "--move", "compact"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((v["move"].as_str(), v["compact_refused"].as_str()), (Some("stop"), Some("a relay wrap is pending")), "{v}");
    // max_relays of 1, after one boundary relay.
    let w = Warm::new("kw-judged-max");
    w.settings(r#"{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.relay":{"value":"on"},"mods.attend.continuity.max_relays":{"value":1}}"#);
    handoff_for(&w, Some(WRAP));
    assert!(w.run(&["keepwarm", "compacted", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "resumed", "--trigger", "boundary"]).status.success());
    assert_eq!(refused(&w, &[]), "max_relays reached");
    // The operator's prompt started this turn.
    let w = Warm::new("kw-judged-operator");
    w.settings(RELAY_ON);
    handoff_for(&w, Some(WRAP));
    assert_eq!(refused(&w, &["--operator-turn"]), "the operator is in this turn");
}

/// A doubt with relay on stops the relay, so a later compact in the same
/// absence is refused. Its JSON carries the instructions the mod compacts
/// with, no resume, and a `handoff:` line that says the relay stopped. In
/// a turn the operator's prompt started, the doubt does not compact, and
/// the line does not say the operator was away.
///
/// References:
/// - ADR-418 item 11: in relay mode, `doubt` becomes `relay: stop doubt`.
#[test]
fn a_doubt_with_relay_on_stops_the_relay() {
    let w = Warm::new("kw-judged-doubt-relay");
    w.settings(RELAY_ON);
    handoff_for(&w, Some(WRAP));
    let v = judged_json(&w, "doubt", Some("doubt"), &[]);
    let (r, instructions) = (v["wrap"].clone(), v["instructions"].as_str().unwrap().to_string());
    assert_eq!(
        v,
        serde_json::json!({ "recorded": true, "armed": true, "acted_on": true, "move": "doubt", "compact_refused": null, "wrap": r, "instructions": instructions, "resume": null })
    );
    assert!(instructions.ends_with("DO NEXT — start."), "{instructions}");
    assert!(r["handoff"].as_str().unwrap().ends_with(" while its operator was away. The relay stopped: the boundary judge said doubt, the release landed."), "{r}");
    w.reading(&[(5 * MIN, 200_000, 500)]);
    let d = policy(&w, "off");
    assert_eq!((d["relay_stop"].as_str(), d["send_handoff"].as_bool()), (Some("doubt"), Some(false)), "the mod sent the doubt's handoff already: {d}");
    let v = judged_json(&w, "compact", Some("compact"), &[]);
    assert_eq!((v["move"].as_str(), v["compact_refused"].as_str()), (Some("stop"), Some("the relay stopped: doubt")), "{v}");
    let w = Warm::new("kw-judged-doubt-operator");
    w.settings(RELAY_ON);
    handoff_for(&w, Some(WRAP));
    let v = judged_json(&w, "doubt", Some("doubt"), &["--operator-turn"]);
    assert_eq!((v["armed"].as_bool(), v["instructions"].clone(), v["resume"].clone()), (Some(true), serde_json::Value::Null, serde_json::Value::Null), "{v}");
    let line = v["wrap"]["handoff"].as_str().unwrap();
    assert!(line.ends_with(" The relay stopped: the boundary judge said doubt, the release landed."), "{line}");
    assert!(!line.contains("away"), "{line}");
}

/// With keepwarm off no tick runs, so an armed exit is not acted on:
/// `acted_on` is false while `armed` is true.
#[test]
fn judged_json_says_an_armed_exit_is_not_acted_on_with_keepwarm_off() {
    let w = Warm::new("kw-judged-acted-on");
    w.settings(r#"{"mods.attend.keepwarm":{"value":"off"}}"#);
    handoff_for(&w, Some(WRAP));
    let v = judged_json(&w, "stop", Some("stop"), &[]);
    assert_eq!((v["armed"].as_bool(), v["acted_on"].as_bool()), (Some(true), Some(false)), "{v}");
}

/// `status --session <id>` reads the named session's ledger from a shell
/// no Claude Code process owns, as a tmux pane or the operator's terminal
/// is; without it, that shell has no session. An id with no session
/// record is refused with exit 3, and one no record could name with
/// exit 2.
#[test]
fn status_session_reads_another_session_from_any_shell() {
    let w = Warm::new("kw-status-session");
    handoff_for(&w, Some(WRAP));
    judged_move(&w, "compact", Some("stop"), &[]);
    w.reading(&[(5 * MIN, 200_000, 500)]);
    let sid = w.f.sid.clone();
    let record = w.f.home.join(".claude").join("sessions").join("3999999.json");
    w.f.without_session(|| {
        assert_eq!(w.run(&["keepwarm", "status", "--json"]).status.code(), Some(1), "no session owns this shell");
        std::fs::write(&record, w.f.record("3999999", &sid, None)).unwrap();
        let s = status_json(&w, &["--session", &sid]);
        assert_eq!(s["boundary"], boundary("compact", "stop"), "{s}");
        assert_eq!(s["exit"]["state"], "armed", "{s}");
        let out = w.run(&["keepwarm", "status", "--session", &sid]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let card = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = card.lines().filter(|l| l.contains(WRAP)).collect();
        assert_eq!(lines.len(), 1, "one line for the last boundary: {card}");
        assert!(
            lines[0].starts_with(&format!("exit        armed · wrap {WRAP}, the judge said compact, the handoff is sufficient, own call continue, carried out stop · "))
                && lines[0].ends_with(" · the judge's reason: the release landed"),
            "{card}"
        );
        assert_eq!(w.run(&["keepwarm", "status", "--json", "--session", "sess-nobody"]).status.code(), Some(3));
        assert_eq!(w.run(&["keepwarm", "status", "--json", "--session", "no good"]).status.code(), Some(2));
        std::fs::remove_file(&record).unwrap();
    });
}

/// The three keys a supervisor reads from `status --json`.
fn supervised(s: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "cache": s["cache"], "relays": s["relays"], "operator_prompt_at": s["operator_prompt_at"] })
}

/// The last request's time as `status --json` reports it, checked against
/// the reading written `ago` seconds before.
fn last_request(s: &serde_json::Value, ago: u64) -> u64 {
    let last = s["cache"]["last_request_at"].as_u64().unwrap_or_else(|| panic!("no last request: {s}"));
    assert!(last.abs_diff(now() - ago) <= 5, "{s}");
    last
}

/// The settings checks keep for a few minutes; a test that changes the
/// settings mid-run drops the kept read.
fn forget_settings(w: &Warm) {
    std::fs::remove_file(w.f.cache().join("keepwarm").join(format!("{}.settings.json", w.f.sid))).ok();
}

/// A warm cache with an armed exit: the card's verdict, the last request,
/// the lapse an hour after it, and the exit's tick fifty idle minutes after
/// it, the time the policy check itself waits for. The operator's prompt
/// is the one `keepwarm reset` recorded.
///
/// References:
/// - ADR-419 item 2: the cache with its next tick, the relay count, and
///   the operator's last prompt.
#[test]
fn status_json_gives_a_warm_cache_and_the_armed_exit_s_tick() {
    let w = Warm::new("kw-status-warm");
    let prompt = now() - 20 * MIN;
    assert!(w.run(&["keepwarm", "reset", "--at", &prompt.to_string()]).status.success());
    handoff_for(&w, Some(WRAP));
    judged_move(&w, "stop", None, &[]);
    w.reading(&[(5 * MIN, 200_000, 500)]);
    let s = status_json(&w, &[]);
    let last = last_request(&s, 5 * MIN);
    assert_eq!(s["exit"]["state"], "armed", "{s}");
    assert_eq!(
        supervised(&s),
        serde_json::json!({
            "cache": { "state": "warm", "last_request_at": last, "lapses_at": last + 60 * MIN, "next_tick_at": last + 50 * MIN },
            "relays": { "count": 0, "max": 5, "relay": false, "stopped": null },
            "operator_prompt_at": prompt,
        }),
        "{s}"
    );
    let d = policy(&w, "off");
    assert_eq!((d["state"].as_str(), d["next_at"].as_u64()), (Some("armed"), Some(last + 50 * MIN)), "{d}");
    assert_eq!(status_json(&w, &[])["cache"], s["cache"], "the check's own tick");
}

/// Two idle hours: the card says cold, the cache lapsed an hour after the
/// last request, and no check is armed to wake the session.
#[test]
fn status_json_gives_a_cold_cache_and_no_tick() {
    let w = Warm::new("kw-status-cold");
    w.reading(&[(120 * MIN, 200_000, 500)]);
    policy(&w, "off");
    let s = status_json(&w, &[]);
    let last = last_request(&s, 120 * MIN);
    assert_eq!(s["cache"], serde_json::json!({ "state": "cold", "last_request_at": last, "lapses_at": last + 60 * MIN, "next_tick_at": null }), "{s}");
}

/// A resumed generation's `relay: stop decision`: the relay is on, one
/// relay ran of the default five, and it stopped on `decision`. The stop
/// waits on the operator, so the hold's first tick falls 50 minutes after
/// the last request and decides hold or lapse.
///
/// References:
/// - ADR-418 item 6: waiting on the operator is pending.
#[test]
fn status_json_gives_a_relay_stopped_on_decision() {
    let w = Warm::new("kw-status-decision");
    w.settings(RELAY_ON);
    handoff_for(&w, Some(WRAP));
    judged_json(&w, "compact", Some("compact"), &[]);
    assert!(w.run(&["keepwarm", "compacted", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "resumed", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "stopped", "decision", "--detail", "which database"]).status.success());
    w.reading(&[(5 * MIN, 200_000, 500)]);
    policy(&w, "off");
    let s = status_json(&w, &[]);
    assert_eq!(s["relays"], serde_json::json!({ "count": 1, "max": 5, "relay": true, "stopped": "decision" }), "{s}");
    let last = last_request(&s, 5 * MIN);
    assert_eq!((s["cache"]["state"].as_str(), s["cache"]["next_tick_at"].as_u64()), (Some("warm"), Some(last + 50 * MIN)), "{s}");
    assert_eq!((s["hold"]["decision"].clone(), s["hold"]["until"].clone()), (serde_json::Value::Null, serde_json::Value::Null), "{s}");
    assert!(s["hold"]["reason"].as_str().is_some_and(|r| r.starts_with("the relay stopped on a decision: the operator is likely back about ")), "{s}");
}

/// With `max_relays` 1, one boundary relay leaves the count at 1 of 1 and
/// the relay not yet stopped. The next relay wrap is judged at the cap and
/// stops the relay with `1 of 1`, and nothing ticks after it.
#[test]
fn status_json_gives_the_relays_at_n_of_max_and_the_stop_at_the_cap() {
    let w = Warm::new("kw-status-cap");
    handoff_for(&w, Some(WRAP));
    assert!(w.run(&["keepwarm", "compacted", "--trigger", "boundary"]).status.success());
    assert!(w.run(&["keepwarm", "resumed", "--trigger", "boundary"]).status.success());
    let _ = relay_wrap_judged(&w, "continue");
    w.settings(r#"{"mods.attend.continuity.keepalive":{"value":true},"mods.attend.continuity.relay":{"value":"on"},"mods.attend.continuity.max_relays":{"value":1}}"#);
    forget_settings(&w);
    let s = status_json(&w, &[]);
    assert_eq!(s["relays"], serde_json::json!({ "count": 1, "max": 1, "relay": true, "stopped": null }), "{s}");
    let d = policy(&w, "off");
    assert_eq!((d["action"].as_str(), d["relay_stop"].as_str()), (Some("compact"), Some("1 of 1")), "{d}");
    let s = status_json(&w, &[]);
    assert_eq!(s["relays"], serde_json::json!({ "count": 1, "max": 1, "relay": true, "stopped": "1 of 1" }), "{s}");
    assert_eq!(s["cache"]["next_tick_at"], serde_json::Value::Null, "{s}");
}

/// No ledger, no transcript ways can read, and settings ways cannot give:
/// every key is present and every value is null, the cache `unknown`.
#[test]
fn status_json_with_nothing_on_record_is_all_null() {
    let w = Warm::new("kw-status-none");
    std::fs::write(w.f.home.join("settings.json"), "not json").unwrap();
    let s = status_json(&w, &[]);
    assert_eq!(
        s,
        serde_json::json!({
            "exit": null,
            "boundary": null,
            "cache": { "state": "unknown", "last_request_at": null, "lapses_at": null, "next_tick_at": null },
            "relays": { "count": null, "max": null, "relay": null, "stopped": null },
            "operator_prompt_at": null,
            "hold": { "decision": null, "until": null, "pings": null, "reason": null },
            "timezone": { "zone": null, "source": null },
        })
    );
    assert!(!w.ledger().exists(), "status writes no ledger");
}

/// `status --json --session <id>` from a shell no Claude Code process owns
/// gives the same cache, relays and operator's prompt as the session's own
/// process: each comes from a file or a `ways` read any shell can make.
#[test]
fn status_json_session_gives_the_same_fields_from_any_shell() {
    let w = Warm::new("kw-status-fields-session");
    w.settings(RELAY_ON);
    let prompt = now() - 20 * MIN;
    assert!(w.run(&["keepwarm", "reset", "--at", &prompt.to_string()]).status.success());
    handoff_for(&w, Some(WRAP));
    judged_move(&w, "stop", None, &[]);
    w.reading(&[(5 * MIN, 200_000, 500)]);
    policy(&w, "off");
    let own = supervised(&status_json(&w, &[]));
    let last = own["cache"]["last_request_at"].as_u64().unwrap();
    assert_eq!(own["cache"]["next_tick_at"].as_u64(), Some(last + 50 * MIN), "{own}");
    assert_eq!((own["relays"]["relay"].as_bool(), own["operator_prompt_at"].as_u64()), (Some(true), Some(prompt)), "{own}");
    let sid = w.f.sid.clone();
    let record = w.f.home.join(".claude").join("sessions").join("3999998.json");
    w.f.without_session(|| {
        std::fs::write(&record, w.f.record("3999998", &sid, None)).unwrap();
        assert_eq!(supervised(&status_json(&w, &["--session", &sid])), own);
        std::fs::remove_file(&record).unwrap();
    });
}
}
