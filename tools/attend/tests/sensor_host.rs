// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor host`: the session's sensors run, and what they report
//! lands in the session's project tray as system signals, each addressed to
//! the session with its lane and expiry, and each cued by one stdout line.
//! A trigger kept for the session fires once across a restart, a kill
//! right after the fire included, and across `--moved-from`. A command the
//! host runs does not act as the session.

#![cfg(unix)]

mod common;

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Fixture, Host};
use serde_json::Value;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// The signal files in `tray`, by name, each with its text.
fn signals(tray: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(tray)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "signal"))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

/// The signals sensor `sensor` wrote to the fixture's project tray.
fn from(f: &Fixture, sensor: &str) -> Vec<(String, String)> {
    signals(&f.signals().join(f.project_tray())).into_iter().filter(|(_, t)| t.starts_with(&format!("system:{sensor}|"))).collect()
}

/// A command sensor polled every second whose output says `{"up": true}`,
/// appending a line to `<home>/<name>.polls` on each poll, with one
/// trigger that fires at magnitude 3, the event lane's cut, when `up`
/// enters true.
fn beacon(f: &Fixture, name: &str, trigger: &str) {
    beacon_at(f, name, 3, trigger);
}

/// [`beacon`] whose trigger fires at `magnitude`.
fn beacon_at(f: &Fixture, name: &str, magnitude: u32, trigger: &str) {
    let polls = f.home.join(format!("{name}.polls"));
    let script = format!("echo x >> '{}'; printf '{{\"up\": true}}'", polls.display());
    f.sensor(
        name,
        &format!(
            "kind: command\ninterval: 1\nmin_interval: 1\nthreshold: 1\nargv: [sh, -c, {}]\noutput: json\ntriggers:\n  - {{name: up, path: \"$\", on: enters, where: {{up: true}}, magnitude: {magnitude}, description: \"{name} is up\", {trigger}}}\n",
            serde_json::to_string(&script).unwrap()
        ),
    );
}

fn polls(f: &Fixture, name: &str) -> usize {
    std::fs::read_to_string(f.home.join(format!("{name}.polls"))).map(|t| t.lines().count()).unwrap_or(0)
}

/// The fixture with peers off too, so only the test's own sensors run.
fn quiet(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.sensor("peers", "enabled: false\n");
    f
}

/// A command sensor's trigger becomes a system signal to the session: from
/// `system:<sensor>`, addressed to it, on the event lane, lapsing when the
/// trigger says, or never for `expires: session`. Each write prints one
/// `signal <id>` line and stdout carries nothing else. `attend message
/// inbox --json` shows the sender's kind, the addressee, the lane and the
/// expiry.
#[test]
fn the_host_writes_a_trigger_as_a_system_signal_and_cues_each_write() {
    let f = quiet("host-write");
    beacon(&f, "beacon", "memory: session, expires: 120");
    beacon(&f, "notice", "memory: session, expires: session");
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("both signals", Duration::from_secs(20), || from(&f, "beacon").len() + from(&f, "notice").len() >= 2);
    let started = now();
    host.wait_for("a few more polls", Duration::from_secs(20), || polls(&f, "beacon") >= 3);

    let beacon = from(&f, "beacon");
    assert_eq!(beacon.len(), 1, "a session trigger fires once: {beacon:?}");
    let line = agent_identity::signal::wire_line(&beacon[0].1);
    let s = agent_identity::parse_signal(line).unwrap();
    assert_eq!((s.from.as_ref(), s.from_kind, s.from_id, s.to), ("system:beacon", Some("system"), Some("beacon"), Some(f.sid.as_str())), "{line}");
    assert_eq!((s.lane, s.message), (Some(agent_identity::signal::Lane::Event), "beacon is up"), "{line}");
    let expires = s.expires.unwrap_or_else(|| panic!("no expiry: {line}"));
    assert!(expires.abs_diff(started + 120) <= 10, "expires {expires}, written about {started}: {line}");
    let notice = from(&f, "notice");
    assert_eq!(notice.len(), 1, "{notice:?}");
    assert!(!notice[0].1.contains("expires:"), "expires: session writes no tag: {}", notice[0].1);

    let ids: Vec<String> = signals(&f.signals().join(f.project_tray())).iter().map(|(n, _)| n.trim_end_matches(".signal").to_string()).collect();
    let cues: Vec<String> = host.stdout().lines().map(str::to_string).collect();
    assert_eq!(cues.len(), ids.len(), "one cue per write: {cues:?} for {ids:?}");
    for cue in &cues {
        let id = cue.strip_prefix("signal ").unwrap_or_else(|| panic!("stdout carries only cues: {cue:?}"));
        assert!(ids.iter().any(|i| i == id), "{cue} names no signal written: {ids:?}");
    }

    let inbox: Value = serde_json::from_str(&f.ok(&["message", "inbox", "--json"])).unwrap();
    let row = inbox["messages"].as_array().unwrap().iter().find(|m| m["from"] == "system:beacon").unwrap_or_else(|| panic!("{inbox}"));
    assert_eq!((row["from_kind"].as_str(), row["to"].as_str(), row["lane"].as_str()), (Some("system"), Some(f.sid.as_str()), Some("event")), "{row}");
    assert_eq!(row["expires"].as_u64(), Some(expires), "{row}");
    let row = inbox["messages"].as_array().unwrap().iter().find(|m| m["from"] == "system:notice").unwrap();
    assert!(row["expires"].is_null(), "{row}");
}

/// What the host logs once a delivered session trigger reached the
/// checkpoint.
const CHECKPOINTED: &str = "a session trigger is spent; checkpointed";

/// An event-lane batch under the cut of 3.0 is logged and not written, one
/// at the cut is written, and a message-lane line, which has no cut, is written whatever its
/// magnitude. A test fire under the cut writes nothing either.
#[test]
fn an_event_batch_under_the_cut_writes_no_signal() {
    let f = quiet("host-cut");
    beacon_at(&f, "low", 1, "memory: session");
    beacon(&f, "high", "memory: session");
    f.sensor(
        "chat",
        "kind: command\ninterval: 1\nmin_interval: 1\nthreshold: 1\nlane: message\nargv: [sh, -c, \"printf '{\\\"up\\\": true, \\\"who\\\": \\\"ann\\\"}'\"]\noutput: json\n\
         triggers:\n  - {name: said, path: \"$\", on: enters, where: {up: true}, memory: session, author: who, magnitude: 1, description: \"ann said hi\"}\n",
    );
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("the high and message-lane signals", Duration::from_secs(20), || !from(&f, "high").is_empty() && !from(&f, "chat").is_empty());
    let after = polls(&f, "low");
    host.wait_for("three more polls of low", Duration::from_secs(20), || polls(&f, "low") >= after + 3);
    assert!(from(&f, "low").is_empty(), "a line under the cut was written: {:?}", from(&f, "low"));
    assert!(host.stderr().contains("low: under the event lane's cut (1.0 of 3.0), not written: low is up"), "{}", host.stderr());
    assert!(from(&f, "chat")[0].1.contains("|lane:message|"), "{:?}", from(&f, "chat"));
    let input = f.home.join("up.json");
    std::fs::write(&input, r#"{"up": true}"#).unwrap();
    let fire: Value = serde_json::from_str(&f.ok(&["sensor", "fire", "low", "--input", &input.to_string_lossy(), "--json"])).unwrap();
    assert_eq!((fire["cut"].as_bool(), fire["signals"].is_null()), (Some(true), true), "{fire}");
    assert!(from(&f, "low").is_empty());
}

/// A session trigger whose line the governor holds is not offered again
/// while it waits, so its magnitude does not build up past the cut. Once
/// the governor reopens the line is cut, once, and spent: it is never
/// written, and the sensor's interval backs off instead of staying at its
/// minimum.
#[test]
fn a_line_the_governor_holds_is_cut_once_and_not_offered_again() {
    let f = quiet("host-held");
    std::fs::write(f.home.join("config").join("attend").join("config.yaml"), "cleanup:\n  enabled: false\ngovernor:\n  base_cooldown: 5\n").unwrap();
    beacon(&f, "high", "memory: session");
    let (flag, low_polls) = (f.home.join("low.flag"), f.home.join("low.polls"));
    let script = format!("echo x >> '{}'; if [ -f '{}' ]; then printf '{{\"up\": true}}'; else printf '{{\"up\": false}}'; fi", low_polls.display(), flag.display());
    f.sensor(
        "low",
        &format!(
            "kind: command\ninterval: 4\nmin_interval: 1\ndecay_threshold: 1\nthreshold: 1\nargv: [sh, -c, {}]\noutput: json\n\
             triggers:\n  - {{name: up, path: \"$\", on: enters, where: {{up: true}}, magnitude: 1, memory: session, description: \"low is up\"}}\n",
            serde_json::to_string(&script).unwrap()
        ),
    );
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("high's signal", Duration::from_secs(20), || !from(&f, "high").is_empty());
    std::fs::write(&flag, "").unwrap();
    host.wait_for("low held by the governor", Duration::from_secs(20), || host.stderr().contains("sensors ready but governor holding"));
    host.wait_for("the governor to reopen", Duration::from_secs(40), || host.stderr().contains("low: under the event lane's cut") || !from(&f, "low").is_empty());
    let (low_before, high_before) = (polls(&f, "low"), polls(&f, "high"));
    host.wait_for("ten more polls of high", Duration::from_secs(30), || polls(&f, "high") >= high_before + 10);
    assert!(from(&f, "low").is_empty(), "the held line built up past the cut and was written: {:?}", from(&f, "low"));
    let log = host.stderr();
    assert_eq!(log.matches("low: under the event lane's cut (1.0 of 3.0), not written: low is up").count(), 1, "{log}");
    let low_after = polls(&f, "low") - low_before;
    assert!(low_after <= 6, "low polled {low_after} times while high polled ten: its interval did not back off");
}

/// A trigger kept for the session is spent when its line is written, and
/// the checkpoint follows at once, so a host killed after the write and
/// that checkpoint, and started again, does not fire it a second time.
#[test]
fn a_session_trigger_fires_once_across_a_kill_after_its_write_and_checkpoint() {
    let f = quiet("host-kill");
    beacon(&f, "beacon", "memory: session");
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("the signal and its checkpoint", Duration::from_secs(20), || !from(&f, "beacon").is_empty() && host.stderr().contains(CHECKPOINTED));
    host.kill();
    let before = polls(&f, "beacon");
    let again = Host::start(&f, &f.sid, &[]);
    again.wait_for("three polls after the restart", Duration::from_secs(20), || polls(&f, "beacon") >= before + 3);
    assert_eq!(from(&f, "beacon").len(), 1, "the trigger fired again after the kill; the restarted host logged:\n{}", again.stderr());
    assert!(again.stdout().is_empty(), "{}", again.stdout());
}

/// `--moved-from` carries the old id's sensor stores to the new id, and the
/// memory of a trigger kept for the session, so a trigger spent under the
/// old id stays spent.
#[test]
fn moved_from_carries_the_stores_and_the_spent_triggers() {
    let f = quiet("host-moved");
    beacon(&f, "beacon", "memory: session");
    beacon(&f, "stored", "memory: store");
    let store_dir = f.home.join("state").join("agent-ways").join("attend").join("sensor-state");
    let sessions = || -> Vec<String> {
        std::fs::read_dir(&store_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| serde_json::from_str::<Value>(&std::fs::read_to_string(e.path()).ok()?).ok())
            .map(|r| format!("{}:{}", r["sensor"].as_str().unwrap_or(""), r["session"].as_str().unwrap_or("")))
            .collect()
    };
    let old = Host::start(&f, "sess-old", &[]);
    old.wait_for("the old id's signal, its checkpoint and stores", Duration::from_secs(20), || {
        !from(&f, "beacon").is_empty() && old.stderr().contains(CHECKPOINTED) && sessions().iter().any(|s| s == "stored:sess-old")
    });
    old.kill();
    let before = polls(&f, "beacon");
    let new = Host::start(&f, "sess-new", &["--moved-from", "sess-old"]);
    new.wait_for("three polls under the new id", Duration::from_secs(20), || polls(&f, "beacon") >= before + 3);
    let beacon = from(&f, "beacon");
    assert_eq!(beacon.len(), 1, "the spent trigger fired again under the new id: {beacon:?}\n{}", new.stderr());
    let stores = sessions();
    assert!(stores.iter().any(|s| s == "stored:sess-new"), "{stores:?}");
    assert!(!stores.iter().any(|s| s.ends_with(":sess-old")), "the old id kept a store: {stores:?}");
}

/// A signal the host cannot write spends nothing: the trigger fires again,
/// and once the tray can be written its line goes out, once.
#[test]
fn a_failed_write_leaves_the_session_trigger_to_fire_again() {
    let f = quiet("host-unwritable");
    beacon(&f, "beacon", "memory: session");
    let tray = f.signals().join(f.project_tray());
    std::fs::create_dir_all(f.signals()).unwrap();
    std::fs::write(&tray, "a file where the tray's directory goes").unwrap();
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("two failed writes", Duration::from_secs(20), || host.stderr().matches("beacon: not written").count() >= 2);
    assert!(!host.stderr().contains(CHECKPOINTED), "a failed write spent the trigger:\n{}", host.stderr());
    assert!(host.stdout().is_empty(), "a cue for a signal not written: {}", host.stdout());
    std::fs::remove_file(&tray).unwrap();
    host.wait_for("the signal once the tray is writable", Duration::from_secs(20), || !from(&f, "beacon").is_empty() && host.stderr().contains(CHECKPOINTED));
    let after = polls(&f, "beacon");
    host.wait_for("three more polls", Duration::from_secs(20), || polls(&f, "beacon") >= after + 3);
    assert_eq!(from(&f, "beacon").len(), 1, "{:?}", from(&f, "beacon"));
}

/// The control for the test above: without `--moved-from`, a new id starts
/// with its own session memory and fires once of its own.
#[test]
fn without_moved_from_a_new_id_fires_once_of_its_own() {
    let f = quiet("host-fresh");
    beacon(&f, "beacon", "memory: session");
    let old = Host::start(&f, "sess-old", &[]);
    old.wait_for("the old id's signal", Duration::from_secs(20), || !from(&f, "beacon").is_empty());
    old.kill();
    let new = Host::start(&f, "sess-new", &[]);
    new.wait_for("the new id's signal", Duration::from_secs(20), || from(&f, "beacon").len() == 2);
    assert!(from(&f, "beacon").iter().any(|(_, t)| t.contains("|to:sess-new|")));
}

/// The name `attend peer whoami --display` prints for the fixture's
/// session.
fn display_name(f: &Fixture) -> String {
    f.ok(&["peer", "whoami", "--display"]).trim().to_string()
}

/// The instance `sid` holds in the fixture's project, from the registry.
fn instance(f: &Fixture, sid: &str) -> Option<String> {
    attend_instances::Registry::with_base(f.cache().join("instances")).lookup(&f.origin, sid)
}

/// The host takes the session's instance slot as it starts, so its name
/// carries a suffix: nothing else registers a listening session (#1108).
#[test]
fn a_host_registers_the_session_and_whoami_shows_its_suffix() {
    let f = quiet("host-register");
    let bare = display_name(&f);
    assert!(!bare.contains('-'), "no suffix before a host runs: {bare}");
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("the instance slot", Duration::from_secs(20), || instance(&f, &f.sid).is_some());
    assert_eq!(instance(&f, &f.sid).as_deref(), Some("alpha"));
    assert_eq!(display_name(&f), format!("{bare}-alpha"));
}

/// After `/clear` the host started with `--moved-from` moves the old id's
/// slot to the new id before it registers, so the suffix stays. A second
/// session holds alpha, so the old id holds beta, and a new id registering
/// on its own would take gamma.
#[test]
fn a_host_with_moved_from_keeps_the_old_suffix() {
    let f = quiet("host-rename");
    let other = Host::start(&f, "sess-other", &[]);
    other.wait_for("the other session's slot", Duration::from_secs(20), || instance(&f, "sess-other").is_some());
    let old = Host::start(&f, "sess-old", &[]);
    old.wait_for("the old id's slot", Duration::from_secs(20), || instance(&f, "sess-old").is_some());
    assert_eq!(instance(&f, "sess-old").as_deref(), Some("beta"));
    drop(old);
    f.set_session_id("sess-new");
    let bare = display_name(&f);
    let new = Host::start(&f, "sess-new", &["--moved-from", "sess-old"]);
    new.wait_for("the new id's slot", Duration::from_secs(20), || instance(&f, "sess-new").is_some());
    assert_eq!(instance(&f, "sess-new").as_deref(), Some("beta"), "the new id keeps the old suffix");
    assert_eq!(display_name(&f), format!("{bare}-beta"));
}

/// A command the host runs does not act as the session: its `attend
/// message send` is refused, and writes no signal.
#[test]
fn a_command_the_host_runs_does_not_act_as_the_session() {
    let f = quiet("host-guard");
    let probe = f.home.join("probe");
    std::fs::create_dir_all(&probe).unwrap();
    let script = format!(
        "a='{bin}'; o='{out}'; \"$a\" message send from-a-sensor 2> \"$o/send.err\"; echo $? > \"$o/send.code\"; echo '{{}}'",
        bin = env!("CARGO_BIN_EXE_attend"),
        out = probe.display()
    );
    f.sensor("probe", &format!("kind: command\ninterval: 1\nmin_interval: 1\nargv: [sh, -c, {}]\noutput: json\n", serde_json::to_string(&script).unwrap()));
    let host = Host::start(&f, &f.sid, &[]);
    host.wait_for("the probe", Duration::from_secs(20), || probe.join("send.code").exists() && std::fs::read_to_string(probe.join("send.code")).is_ok_and(|c| !c.is_empty()));
    let read = |n: &str| std::fs::read_to_string(probe.join(n)).unwrap_or_default();
    assert_eq!(read("send.code").trim(), "3", "{}", read("send.err"));
    assert!(read("send.err").contains("this attend was started by a sensor command, so it does not act as the session"), "{}", read("send.err"));
    let open = f.signals().join(attend_groups::BROADCAST_DIR);
    assert!(signals(&open).is_empty(), "the refused send wrote a signal");
}

/// A command that detaches from the host is still refused as the session:
/// a send backgrounded from a subshell that exits, `( … & )`, and one
/// started by `setsid -f`. Each waits a second first, so by the time it
/// runs its parent has exited and the host is no longer among its
/// ancestors; the marker the host set in its environment is what refuses
/// it, and the refusal names that marker. Neither writes a signal. Where
/// no `setsid` is on PATH, as on stock macOS, the `setsid -f` form is
/// skipped and the subshell form still runs.
#[test]
fn a_command_that_detaches_from_the_host_is_still_refused() {
    let f = quiet("host-detached");
    let probe = f.home.join("probe");
    std::fs::create_dir_all(&probe).unwrap();
    // Each form runs `sh -c '<send>' <attend> <probe dir> <form>`.
    let send = "sleep 1; \"$0\" message send \"detached-$2\" > /dev/null 2> \"$1/$2.err\"; echo $? > \"$1/$2.code\"";
    let script = format!(
        "a='{bin}'; o='{out}'; s='{send}'; mkdir \"$o/once\" 2> /dev/null || {{ echo '{{}}'; exit 0; }}; \
         ( sh -c \"$s\" \"$a\" \"$o\" bg < /dev/null > /dev/null 2>&1 & ); \
         if command -v setsid > /dev/null; then setsid -f sh -c \"$s\" \"$a\" \"$o\" setsid < /dev/null > /dev/null 2>&1; else echo skip > \"$o/setsid.code\"; fi; \
         echo '{{}}'",
        bin = env!("CARGO_BIN_EXE_attend"),
        out = probe.display(),
    );
    f.sensor("probe", &format!("kind: command\ninterval: 1\nmin_interval: 1\nargv: [sh, -c, {}]\noutput: json\n", serde_json::to_string(&script).unwrap()));
    let host = Host::start(&f, &f.sid, &[]);
    let read = |n: &str| std::fs::read_to_string(probe.join(n)).unwrap_or_default();
    host.wait_for("both detached sends", Duration::from_secs(20), || ["bg", "setsid"].iter().all(|form| !read(&format!("{form}.code")).is_empty()));
    let pid = host.child.id();
    let refusal = format!(
        "this attend does not act as the session: its environment carries ATTEND_SENSOR_HOST={}, the marker attend {pid} sets",
        attend_presence::session::marker_value(pid)
    );
    for form in ["bg", "setsid"] {
        if read(&format!("{form}.code")).trim() == "skip" {
            assert_eq!(form, "setsid", "only setsid may be skipped");
            eprintln!("setsid: not on this host, so the setsid -f form is skipped");
            continue;
        }
        assert_eq!(read(&format!("{form}.code")).trim(), "3", "{form}: {}", read(&format!("{form}.err")));
        assert!(read(&format!("{form}.err")).contains(&refusal), "{form}: {}", read(&format!("{form}.err")));
    }
    let open = f.signals().join(attend_groups::BROADCAST_DIR);
    assert!(signals(&open).is_empty(), "a refused send wrote a signal to #open");
    let tray = signals(&f.signals().join(f.project_tray()));
    assert!(tray.iter().all(|(_, t)| !t.contains("detached-")), "a refused send wrote a signal to the project: {tray:?}");
}

/// The `timeout` program on PATH, as `timeout` or as coreutils' `gtimeout`,
/// or none: stock macOS ships neither.
fn timeout_program() -> Option<&'static str> {
    ["timeout", "gtimeout"].into_iter().find(|t| {
        std::process::Command::new("sh").args(["-c", "command -v \"$0\" > /dev/null", t]).status().is_ok_and(|s| s.success())
    })
}

/// The last poll `attend sensor list --json` reports for `sensor`, once it
/// has one.
fn last_poll(f: &Fixture, sensor: &str) -> Option<Value> {
    let out = f.attend(&["sensor", "list", "--json"]);
    let all: Value = serde_json::from_slice(&out.stdout).ok()?;
    all.as_array()?.iter().find(|e| e["name"] == sensor).map(|e| e["last_poll"].clone()).filter(|p| !p.is_null())
}

/// The pids whose command line carries `sensor host` for `f`'s session and
/// directory and still does a moment later. A refused attend exits at once,
/// so what stays is a host or the wrapper that runs one. The directory is
/// the fixture's own, so another run's processes do not count.
fn lasting_hosts(f: &Fixture) -> Vec<String> {
    let pattern = format!("sensor host --session {} --cwd {}", f.sid, f.origin);
    let look = || {
        let out = std::process::Command::new("pgrep").args(["-f", &pattern]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).split_whitespace().map(str::to_string).collect::<Vec<_>>()
    };
    let first = look();
    std::thread::sleep(Duration::from_millis(300));
    let second = look();
    first.into_iter().filter(|p| second.contains(p)).collect()
}

/// A host started through an argv wrapper that stays its parent, as
/// `timeout` does, runs: the wrapper names a host without being one. Where
/// no `timeout` is on PATH, as on stock macOS, the wrapper is a shell that
/// runs the host and waits for it. A command that host runs is still
/// refused as the session: a verb behind `env`, `timeout`, `nohup`,
/// `setsid -w`, a shell's `exec`, `exec -a timeout`, or a shell whose own
/// argv ends with the verb's; another host with the very same arguments
/// under a wrapper of its own; and a sensor whose argv is exactly the
/// host's, whose parent is the host itself.
#[test]
fn a_host_under_a_wrapper_runs_and_its_commands_are_still_refused() {
    let f = quiet("host-wrapped");
    beacon(&f, "beacon", "memory: session");
    let probe = f.home.join("probe");
    std::fs::create_dir_all(&probe).unwrap();
    let bin = env!("CARGO_BIN_EXE_attend");
    let timeout = timeout_program();
    // A shell that runs the host and then exits, so it stays the parent and
    // does not exec the host in its place.
    let shell_wrap = "\"$0\" \"$@\"; exit $?";
    let script = format!(
        "a='{bin}'; o='{out}'; t='{t}'; env A=1 \"$a\" message send from-a-sensor 2> \"$o/send.err\"; echo $? > \"$o/send.code\"; \
         if [ -n \"$t\" ]; then \"$t\" 5 \"$a\" message send x 2> \"$o/timeout.err\"; echo $? > \"$o/timeout.code\"; else echo skip > \"$o/timeout.code\"; fi; \
         sh -c 'exec \"$0\" message send x' \"$a\" 2> \"$o/exec.err\"; echo $? > \"$o/exec.code\"; \
         bash -c 'exec -a timeout \"$0\" message send x' \"$a\" 2> \"$o/exec-a.err\"; echo $? > \"$o/exec-a.code\"; \
         nohup \"$a\" message send x > /dev/null 2> \"$o/nohup.err\"; echo $? > \"$o/nohup.code\"; \
         if command -v setsid > /dev/null; then setsid -w \"$a\" message send x 2> \"$o/setsid.err\"; echo $? > \"$o/setsid.code\"; else echo skip > \"$o/setsid.code\"; fi; \
         sh -c '\"$0\" message send x' \"$a\" message send x 2> \"$o/crafted.err\"; echo $? > \"$o/crafted.code\"; \
         mkdir \"$o/once\" 2> /dev/null || {{ echo '{{}}'; exit 0; }}; \
         if [ -n \"$t\" ]; then \"$t\" 20 \"$a\" sensor host --session '{sid}' --cwd '{cwd}'; else sh -c '{shell_wrap}' \"$a\" sensor host --session '{sid}' --cwd '{cwd}'; fi > /dev/null 2> \"$o/host.err\"; echo $? > \"$o/host.code\"; echo '{{}}'",
        out = probe.display(),
        t = timeout.unwrap_or_default(),
        sid = f.sid,
        cwd = f.origin,
    );
    f.sensor("probe", &format!("kind: command\ninterval: 1\nmin_interval: 1\nargv: [sh, -c, {}]\noutput: json\ntimeout: 30\n", serde_json::to_string(&script).unwrap()));
    let own = [bin, "sensor", "host", "--session", &f.sid, "--cwd", &f.origin];
    f.sensor("nested", &format!("kind: command\ninterval: 1\nmin_interval: 1\nargv: {}\noutput: json\ntimeout: 5\n", serde_json::to_string(&own).unwrap()));
    let err = f.home.join("wrapped.err");
    let mut wrapped = match timeout {
        Some(t) => {
            let mut c = f.command_from(Path::new(t));
            c.arg("120");
            c
        }
        None => {
            eprintln!("timeout: no timeout or gtimeout on PATH; the host runs under sh -c '{shell_wrap}'");
            let mut c = f.command_from(Path::new("sh"));
            c.args(["-c", shell_wrap]);
            c
        }
    };
    let child = wrapped
        .args(own)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&err).unwrap()))
        .spawn()
        .unwrap();
    let host = Host { child, out: f.home.join("wrapped.out"), err };
    host.wait_for("the beacon's signal", Duration::from_secs(20), || !from(&f, "beacon").is_empty());
    host.wait_for("the probe", Duration::from_secs(30), || std::fs::read_to_string(probe.join("host.code")).is_ok_and(|c| !c.is_empty()));
    let read = |n: &str| std::fs::read_to_string(probe.join(n)).unwrap_or_default();
    let refusal = "this attend was started by a sensor command, so it does not act as the session";
    for form in ["send", "timeout", "exec", "exec-a", "nohup", "setsid", "crafted", "host"] {
        if read(&format!("{form}.code")).trim() == "skip" {
            match form {
                "setsid" => eprintln!("setsid: not on this host, skipped"),
                "timeout" => eprintln!("timeout: no timeout or gtimeout on PATH, skipped"),
                _ => panic!("{form} was skipped, and only setsid and timeout may be"),
            }
            continue;
        }
        assert_eq!(read(&format!("{form}.code")).trim(), "3", "{form}: {}", read(&format!("{form}.err")));
        assert!(read(&format!("{form}.err")).contains(refusal), "{form}: {}", read(&format!("{form}.err")));
    }
    host.wait_for("the nested sensor's poll", Duration::from_secs(20), || last_poll(&f, "nested").is_some());
    let poll = last_poll(&f, "nested").unwrap();
    let detail = poll["detail"].as_str().unwrap_or_default();
    assert_eq!(poll["ok"], false, "{poll}");
    assert!(detail.contains("exited with status 3") && detail.contains(refusal), "{poll}");
    let wrapper = host.child.id().to_string();
    let out = std::process::Command::new("pgrep").args(["-P", &wrapper]).output().unwrap();
    let inner = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let hosts = lasting_hosts(&f);
    assert!(!inner.is_empty() && hosts.contains(&inner), "the look finds the host itself: {hosts:?}, the host is {inner:?} under {wrapper}");
    assert!(hosts.iter().all(|p| *p == wrapper || *p == inner), "a nested host runs: {hosts:?}, the host is {inner} under {wrapper}");
}

/// A session id no `to` tag can carry, or a directory that is not one, is
/// refused before anything runs.
#[test]
fn a_session_id_no_signal_can_carry_or_a_missing_directory_is_refused() {
    let f = quiet("host-bad-id");
    let missing = f.home.join("no-such-dir").to_string_lossy().into_owned();
    for (session, cwd) in [("two words", f.origin.as_str()), (f.sid.as_str(), missing.as_str())] {
        let out = f.attend(&["sensor", "host", "--session", session, "--cwd", cwd]);
        assert_eq!(out.status.code(), Some(2), "{session} {cwd}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(out.stdout.is_empty());
    }
}

/// A host whose parent exits, as when Claude Code dies without ending the
/// mod's loop, exits too rather than write to a session that is gone.
#[test]
fn a_host_whose_parent_is_gone_exits() {
    let f = quiet("host-orphan");
    beacon(&f, "beacon", "memory: store");
    let err = f.home.join("orphan.err");
    let out = f
        .command_from(Path::new("sh"))
        .args([
            "-c",
            "\"$0\" sensor host --session \"$1\" --cwd \"$2\" > /dev/null 2> \"$3\" & echo $!; \
             until grep -q 'sensor host for session' \"$3\" 2> /dev/null; do sleep 0.05; done",
            env!("CARGO_BIN_EXE_attend"),
            &f.sid,
            &f.origin,
            &err.to_string_lossy(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    /// Ends the host when the test does, should it fail to exit.
    struct Reap(String);
    impl Drop for Reap {
        fn drop(&mut self) {
            std::process::Command::new("kill").args(["-KILL", &self.0]).stderr(Stdio::null()).status().ok();
        }
    }
    let _reap = Reap(String::from_utf8_lossy(&out.stdout).trim().to_string());
    let start = Instant::now();
    let read = || std::fs::read_to_string(&err).unwrap_or_default();
    while !read().contains("the process that started the sensor host is gone; it exits") {
        assert!(start.elapsed() < Duration::from_secs(15), "the orphaned host kept running:\n{}", read());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(polls(&f, "beacon") >= 1 || read().contains("running 1 sensor(s): beacon"), "the host ran before its parent left:\n{}", read());
}

/// The host's status once it exits, or a panic with its log after `limit`.
fn exited(host: &mut Host, limit: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = host.child.try_wait().unwrap() {
            return status;
        }
        assert!(start.elapsed() < limit, "the host kept running:\n{}", host.stderr());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `--parent` names the process the host belongs to, the mod's Claude
/// Code. One already gone when the host starts, as when Claude Code exits
/// between the spawn and attend's main and the host's own parent already
/// reads as the adopter, ends the host before a sensor runs. One that goes
/// later ends it too, though the host's own parent stays.
#[test]
fn a_host_whose_named_parent_is_gone_exits() {
    const GONE: &str = "the process that started the sensor host is gone; it exits";
    let f = quiet("host-named-parent");
    beacon(&f, "beacon", "memory: store");

    let mut early = std::process::Command::new("true").spawn().unwrap();
    early.wait().unwrap();
    let mut host = Host::start(&f, &f.sid, &["--parent", &early.id().to_string()]);
    let status = exited(&mut host, Duration::from_secs(10));
    let log = host.stderr();
    assert!(status.success() && log.contains(GONE), "{status:?}: {log}");
    assert!(!log.contains("running 1 sensor(s)") && polls(&f, "beacon") == 0, "a sensor ran for a parent that was gone:\n{log}");

    let mut parent = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    let mut host = Host::start(&f, &f.sid, &["--parent", &parent.id().to_string()]);
    host.wait_for("the sensors to start", Duration::from_secs(20), || host.stderr().contains("running 1 sensor(s)"));
    assert!(host.child.try_wait().unwrap().is_none(), "the host left while its parent ran:\n{}", host.stderr());
    parent.kill().unwrap();
    parent.wait().unwrap();
    let status = exited(&mut host, Duration::from_secs(10));
    assert!(status.success() && host.stderr().contains(GONE), "{status:?}: {}", host.stderr());
}

/// A host whose stdout is closed, as when the mod stopped reading, writes
/// the signal, checkpoints, and exits on the failed cue.
#[test]
fn a_host_whose_cues_meet_a_closed_pipe_exits_after_its_checkpoint() {
    let f = quiet("host-closed");
    beacon(&f, "beacon", "memory: session");
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let err = f.home.join("closed.err");
    let mut child = f
        .command()
        .args(["sensor", "host", "--session", &f.sid, "--cwd", &f.origin])
        .stdout(writer)
        .stderr(Stdio::from(std::fs::File::create(&err).unwrap()))
        .spawn()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(20) {
            child.kill().ok();
            child.wait().ok();
            panic!("the host kept running on a closed stdout:\n{}", std::fs::read_to_string(&err).unwrap_or_default());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let log = std::fs::read_to_string(&err).unwrap_or_default();
    assert!(status.success(), "{status:?}: {log}");
    assert_eq!(from(&f, "beacon").len(), 1);
    let (checkpointed, exits) = (log.find(CHECKPOINTED), log.find("stdout is closed, so no one reads the cues; the sensor host exits"));
    assert!(checkpointed.is_some() && exits.is_some() && checkpointed < exits, "{log}");
}

/// `attend sensor poll` and `fire` deliver through the host's writer: a
/// poll's line and a fire's `[test]` line reach the session's tray as
/// system signals with the trigger's expiry, `--session` names another
/// session, and with no session at all they deliver nothing and say so.
#[test]
fn poll_and_fire_deliver_to_the_session_through_the_same_writer() {
    let f = quiet("poll-fire");
    beacon(&f, "beacon", "memory: session, expires: 60");
    let input = f.home.join("up.json");
    std::fs::write(&input, r#"{"up": true}"#).unwrap();
    let input = input.to_string_lossy().into_owned();
    let parse = |text: &str| -> (String, Option<String>, Option<u64>) {
        let s = agent_identity::parse_signal(agent_identity::signal::wire_line(text)).unwrap();
        (s.message.to_string(), s.to.map(str::to_string), s.expires)
    };

    let poll: Value = serde_json::from_str(&f.ok(&["sensor", "poll", "beacon", "--json"])).unwrap();
    assert_eq!(poll["signals"].as_array().map(Vec::len), Some(1), "{poll}");
    let written = from(&f, "beacon");
    let (message, to, expires) = parse(&written[0].1);
    assert_eq!((message.as_str(), to.as_deref()), ("beacon is up", Some(f.sid.as_str())));
    assert!(expires.is_some_and(|e| e.abs_diff(now() + 60) <= 10), "{expires:?}");

    let fire: Value = serde_json::from_str(&f.ok(&["sensor", "fire", "beacon", "--input", &input, "--json"])).unwrap();
    let id = fire["signals"][0].as_str().unwrap_or_else(|| panic!("{fire}"));
    let text = std::fs::read_to_string(f.signals().join(f.project_tray()).join(format!("{id}.signal"))).unwrap();
    let (message, to, expires) = parse(&text);
    assert_eq!((message.as_str(), to.as_deref()), ("[test] beacon is up", Some(f.sid.as_str())));
    assert!(expires.is_some_and(|e| e.abs_diff(now() + 60) <= 10), "the trigger's expiry, through the [test] mark: {expires:?}");
    let inbox: Value = serde_json::from_str(&f.ok(&["message", "inbox", "--json"])).unwrap();
    assert!(inbox["messages"].as_array().unwrap().iter().any(|m| m["message"] == "[test] beacon is up" && m["from_kind"] == "system"), "{inbox}");

    let before = from(&f, "beacon").len();
    for dry in [false, true] {
        let mut args = vec!["sensor", "fire", "beacon", "--input", &input, "--session", "sess-other"];
        args.extend(dry.then_some("--dry-run"));
        let refused = f.attend(&args);
        assert_eq!(refused.status.code(), Some(3), "{}", String::from_utf8_lossy(&refused.stderr));
        assert!(String::from_utf8_lossy(&refused.stderr).contains("no record of session sess-other; its tray is unknown"), "{}", String::from_utf8_lossy(&refused.stderr));
    }
    assert_eq!(from(&f, "beacon").len(), before, "a session with no record gets nothing, and no other tray does");
    // Another live session of the project, with a record of its own.
    std::fs::write(f.home.join(".claude").join("sessions").join("4000000.json"), f.record("4000000", "sess-other", None)).unwrap();
    let text = f.ok(&["sensor", "fire", "beacon", "--input", &input, "--session", "sess-other"]);
    assert!(text.contains("delivered on the event lane to session sess-other: beacon-"), "{text}");
    assert!(from(&f, "beacon").iter().any(|(_, t)| t.contains("|to:sess-other|")));

    let before = from(&f, "beacon").len();
    for args in [&["sensor", "fire", "beacon", "--input", &input][..], &["sensor", "poll", "beacon"][..]] {
        let out = f.without_session(|| f.attend(args));
        assert_eq!(out.status.code(), Some(3), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).contains("no session to deliver to"), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let dry = f.without_session(|| f.attend(&["sensor", "fire", "beacon", "--input", &input, "--dry-run"]));
    assert!(dry.status.success() && String::from_utf8_lossy(&dry.stdout).contains("would deliver on the event lane (dry run)"), "{}", String::from_utf8_lossy(&dry.stdout));
    assert_eq!(from(&f, "beacon").len(), before, "nothing was written without a session");
}
