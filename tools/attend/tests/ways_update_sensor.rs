// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The shipped `ways-update` sensor, run by `attend sensor host` against a
//! stub `ways` that prints a fixed `ways manage update --what-if --json`
//! answer: one notice when behind, once per host, and none when current,
//! blocked or switched off in the user layer.
//!
//! References:
//! - ADR-416 item 6: the update check and its sensor file.
//! - ADR-403: the command sensor kind the host runs.

#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::Fixture;

struct Host(Child);

impl Host {
    fn start(f: &Fixture) -> Host {
        let child = f
            .command()
            .args(["sensor", "host", "--session", &f.sid, "--cwd", &f.origin])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Host(child)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(None)) {
            return;
        }
        // SAFETY: kill(2) on the process group the child leads; it is not
        // reaped yet, so its pid still names that group.
        unsafe { libc::kill(-(self.0.id() as libc::pid_t), libc::SIGTERM) };
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if !matches!(self.0.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// A fixture whose `ways` prints `answer` for `manage update --what-if
/// --json` and logs each call, and whose `ways-update` sensor is the
/// shipped one polled every second. The stub calls no attend verb.
fn world(tag: &str, answer: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.sensor("peers", "enabled: false\n");
    // Drop the fixture's switch-off so the shipped file is what runs, and
    // poll it fast so a second poll can be seen.
    std::fs::remove_file(f.home.join("config/attend/sensors/ways-update.yaml")).unwrap();
    f.sensor("ways-update", "interval: 1\nmin_interval: 1\n");
    let ways = f.home.join("bin").join("ways");
    let script = format!(
        "#!/bin/sh\n[ \"$*\" = 'manage update --what-if --json' ] || exit 1\necho x >> '{}'\nprintf '%s' '{}'\n",
        f.home.join("ways.calls").display(),
        answer
    );
    std::fs::write(&ways, script).unwrap();
    std::fs::set_permissions(&ways, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A control sensor that proves the host is polling.
    let ctl = f.home.join("ctl.polls");
    f.sensor(
        "ctl",
        &format!("kind: command\ninterval: 1\nmin_interval: 1\nthreshold: 1\nargv: [sh, -c, \"echo x >> '{}'; echo '{{}}'\"]\noutput: json\n", ctl.display()),
    );
    f
}

fn count(f: &Fixture, file: &str) -> usize {
    std::fs::read_to_string(f.home.join(file)).map(|t| t.lines().count()).unwrap_or(0)
}

fn notices(f: &Fixture) -> Vec<String> {
    let tray: PathBuf = f.signals().join(f.project_tray());
    std::fs::read_dir(tray)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "signal"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .filter(|t| t.starts_with("system:ways-update|"))
        .collect()
}

/// Run a host until the control sensor has polled three times after the
/// ways-update sensor's first poll, then give the notices.
fn run(f: &Fixture) -> Vec<String> {
    let _host = Host::start(f);
    wait_for("a ways-update poll", || count(f, "ways.calls") >= 1);
    let after = count(f, "ctl.polls");
    wait_for("later polls", || count(f, "ctl.polls") >= after + 3 && count(f, "ways.calls") >= 3);
    notices(f)
}

#[test]
fn behind_is_one_event_lane_notice_with_no_expiry_and_a_later_poll_adds_none() {
    let f = world("wu-behind", r#"{"behind": true, "summary": "ways 1.54.2 -> 1.55.0"}"#);
    let got = run(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    let line = agent_identity::signal::wire_line(&got[0]);
    let s = agent_identity::parse_signal(line).unwrap();
    assert_eq!(s.from.as_ref(), "system:ways-update", "{line}");
    assert_eq!(s.to, Some(f.sid.as_str()), "{line}");
    assert_eq!(s.lane, Some(agent_identity::signal::Lane::Event), "{line}");
    assert_eq!(s.message, "agent-ways has a newer release: ways 1.54.2 -> 1.55.0. `ways manage update --what-if` shows it.", "{line}");
    assert!(s.expires.is_none() && !got[0].contains("expires:"), "no expiry tag: {line}");
}

#[test]
fn current_says_nothing() {
    let f = world("wu-current", r#"{"behind": false, "summary": ""}"#);
    assert!(run(&f).is_empty());
}

#[test]
fn a_blocked_checkout_says_nothing() {
    let f = world("wu-blocked", r#"{"behind": null, "blocked": "diverged", "summary": ""}"#);
    assert!(run(&f).is_empty());
}

#[test]
fn a_user_layer_switch_stops_it() {
    let f = world("wu-off", r#"{"behind": true, "summary": "ways 1.54.2 -> 1.55.0"}"#);
    f.sensor("ways-update", "enabled: false\n");
    let _host = Host::start(&f);
    wait_for("control polls", || count(&f, "ctl.polls") >= 4);
    assert_eq!((count(&f, "ways.calls"), notices(&f).len()), (0, 0));
}
