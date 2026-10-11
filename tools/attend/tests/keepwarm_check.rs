//! `attend keepwarm check` through the CLI: ADR-182's judgment run once per
//! call, for a host that polls without `attend run` (ADR-406 items 10 and
//! 13). A stand-in `ways` on PATH serves the transcript reading the sensor
//! takes from `ways context --json`.

mod common;
use common::Fixture;
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
        std::fs::write(&ways, format!("#!/bin/sh\ncat '{}'\n", reading.display())).unwrap();
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
    assert!(w.status_line().contains("off"), "{}", w.status_line());
    w.reading(&[(30 * MIN, 200_000, 500), (MIN, 0, 200_500)]);
    assert_eq!(w.check(), "");
    let status = w.status_line();
    assert!(status.starts_with("keepwarm    on, 3h00m left") || status.starts_with("keepwarm    on, 2h59m left"), "{status}");
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
    assert!(!status.contains(" on,"), "a window that ended an hour ago is not armed: {status}");

    let w = Warm::new("kw-late-write-2");
    w.reading(&[(3 * 60 * MIN, 200_000, 500)]);
    assert_eq!(w.check(), "");
    w.reading(&[(3 * 60 * MIN, 200_000, 500), (20 * MIN, 0, 200_500)]);
    assert_eq!(w.check(), "");
    let status = w.status_line();
    assert!(status.starts_with("keepwarm    on, 2h40m left") || status.starts_with("keepwarm    on, 2h39m left"), "{status}");
}
