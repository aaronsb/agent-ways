//! `attend keepwarm check` through the CLI: ADR-182's judgment run once per
//! call, for a host that polls without `attend run` (ADR-406 items 10 and
//! 13). A stand-in `ways` on PATH serves the transcript reading the sensor
//! takes from `ways context --json`.

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

/// A fixture with the keepwarm sensor on, and a `ways` that prints
/// whatever [`Warm::reading`] last wrote.
struct Warm {
    f: Fixture,
    bin: PathBuf,
    reading: PathBuf,
}

impl Warm {
    fn new(tag: &str) -> Self {
        let f = Fixture::new(tag);
        std::fs::write(
            f.home.join("config").join("attend").join("config.yaml"),
            "sensors:\n  keepwarm:\n    enabled: true\ncleanup:\n  enabled: false\n",
        )
        .unwrap();
        let bin = f.home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let reading = f.home.join("reading.json");
        let ways = bin.join("ways");
        // `ways settings ...` answers with settings.json beside the reading,
        // when a test wrote one; anything else is `ways context --json`.
        let settings = f.home.join("settings.json");
        // The keepalive policy runs only where ways says enabled: true.
        std::fs::write(&settings, r#"{"view":"effective","keys":{"attend_mod.keepalive.enabled":{"value":true}}}"#).unwrap();
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

/// The sensor's own switch in attend's config turns the check off too.
#[test]
fn the_check_is_off_where_the_sensor_is_off() {
    let w = Warm::new("kw-config-off");
    std::fs::write(
        w.f.home.join("config").join("attend").join("config.yaml"),
        "sensors:\n  keepwarm:\n    enabled: false\n",
    )
    .unwrap();
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    w.check();
    let out = w.run(&["keepwarm", "check"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("off in attend's config"));
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

/// While an `attend run` holds the session, its sensor owns keepwarm: the
/// check prints nothing, so the session is not woken twice.
#[test]
fn the_check_stands_down_under_a_live_run() {
    let w = Warm::new("kw-live-run");
    w.reading(&[(51 * MIN, 200_000, 500)]);
    assert!(w.run(&["keepwarm", "on"]).status.success());
    w.check();
    let run = w.f.run();
    run.wait_for("the run's lock", std::time::Duration::from_secs(20), |_| {
        let out = w.run(&["keepwarm", "check"]);
        String::from_utf8_lossy(&out.stderr).contains("an attend run holds this session")
    });
    assert_eq!(w.check(), "");
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

/// With `attend_mod.keepwarm: off` the attend mod never runs `keepwarm
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
/// never the "on" and "off" of the mod's `attend_mod.keepwarm` setting, and
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
        r#"{"view":"effective","keys":{"attend_mod.keepwarm":{"value":"off","default":"on","layer":"project","file":"/p/.claude/ways.yaml"}}}"#,
    )
    .unwrap();
    let text = card(&w);
    let line = text.lines().find(|l| l.starts_with("mod setting")).unwrap_or_else(|| panic!("{text}"));
    assert_eq!(line, "mod setting attend_mod.keepwarm off (from /p/.claude/ways.yaml); the mod never runs the keepwarm check in this session");
}

/// `check --policy --json`: the keepalive policy's decision (ADR-414 item
/// 7), with the autocompact gate the mod read.
fn policy(w: &Warm, gate: &str) -> serde_json::Value {
    let out = w.run(&["keepwarm", "check", "--policy", "--json", "--autocompact", gate]);
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
        r#"{"view":"effective","keys":{"attend_mod.keepalive.enabled":{"value":true},"attend_mod.keepalive.max_keepalives":{"value":0},"attend_mod.keepalive.wrap":{"value":"file"},"attend_mod.keepalive.wrap_file":{"value":"{state}/wrap/{session}.md"}}}"#,
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
        r#"{"view":"effective","keys":{"attend_mod.keepalive.enabled":{"value":true},"attend_mod.keepalive.max_keepalives":{"value":0}}}"#,
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
    assert!(text.contains("keepalive   off: the attend_mod settings could not be read (armed windows only)"), "{text}");
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
        r#"{"view":"effective","keys":{"attend_mod.keepalive.enabled":{"value":true},"attend_mod.keepalive.max_keepalives":{"value":0}}}"#,
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
    std::fs::write(&user, "attend_mod:\n  keepwarm: on\n").unwrap();
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
    for f in ["gone.settings.json", "gone.lock", "live.settings.json", "live.lock", "live.ledger.json"] {
        std::fs::write(dir.join(f), "{}").unwrap();
    }
    // A lock another process holds is never pruned, ledger or not.
    let held = std::fs::OpenOptions::new().write(true).open(dir.join("gone.lock")).unwrap();
    held.lock().unwrap();
    policy(&w, "off");
    assert!(!dir.join("gone.settings.json").exists());
    assert!(dir.join("gone.lock").exists(), "a lock file is never pruned");
    assert!(dir.join("live.settings.json").exists() && dir.join("live.lock").exists());
    drop(held);
}
}
