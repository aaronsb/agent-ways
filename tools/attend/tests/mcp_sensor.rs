// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! An mcp sensor end to end, on a stdio server written in sh: it stays
//! disabled until `attend sensor probe` records two matching calls and
//! the operator's confirmation, a tool that is not declared read-only is
//! refused and the refusal recorded, and under the sensor host a trigger
//! on the tool's result becomes a system signal. No server outlives the
//! process that started it, a re-exec included.

#![cfg(unix)]

mod common;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::Fixture;
use serde_json::{json, Value};

fn server_script() -> String {
    format!("{}/tests/fixtures/mcp-server.sh", env!("CARGO_MANIFEST_DIR"))
}

fn doc(out: &[u8]) -> Value {
    serde_json::from_slice(out).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(out)))
}

/// The fixture with peers off too, so only the test's own sensor runs.
fn quiet(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.sensor("peers", "enabled: false\n");
    f
}

/// The text of the mcp sensor `name`, polled every second, calling `tool`
/// on the fixture server over the items file at `items`, its servers'
/// pids noted in `pids`, with one trigger that fires for each item it has
/// not seen this session.
fn sensor_text(name: &str, tool: &str, items: &Path, pids: &Path) -> String {
    format!(
        "# {name}: an mcp sensor on the fixture server\n\
         kind: mcp\ninterval: 1\nmin_interval: 1\nthreshold: 1\n\
         server: {{command: sh, args: [{script}, serve], env: {{FIXTURE_PIDS: {pids}}}}}\n\
         tool: {tool}\nargs: {{file: {items}}}\n\
         triggers:\n  - {{name: new, path: \"$.items[*]\", key: id, on: added, memory: session, magnitude: 3, description: \"new item {{id}}\"}}\n",
        script = server_script(),
        pids = pids.display(),
        items = items.display(),
    )
}

/// Write the sensor `name` on `tool` over items `a`, and return the path
/// of its pid file.
fn mcp_sensor(f: &Fixture, name: &str, tool: &str) -> PathBuf {
    let items = f.home.join(format!("{name}.items.json"));
    std::fs::write(&items, json!({"items": [{"id": "a"}]}).to_string()).unwrap();
    let pids = f.home.join(format!("{name}.pids"));
    f.sensor(name, &sensor_text(name, tool, &items, &pids));
    pids
}

fn user_file(f: &Fixture, name: &str) -> String {
    std::fs::read_to_string(f.home.join("config").join("attend").join("sensors").join(format!("{name}.yaml"))).unwrap()
}

/// The sensor `name` as `attend sensor list --json` shows it.
fn listed(f: &Fixture, name: &str) -> Value {
    let all = doc(f.ok(&["sensor", "list", "--json"]).as_bytes());
    all.as_array().unwrap().iter().find(|e| e["name"] == name).unwrap_or_else(|| panic!("no {name} in {all}")).clone()
}

fn noted(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file).unwrap_or_default().lines().map(str::to_string).collect()
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
}

/// The pids of `file` still running after up to five seconds.
fn left_behind(file: &Path) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let live: Vec<String> = noted(file).into_iter().filter(|p| alive(p)).collect();
        if live.is_empty() || Instant::now() > until {
            return live;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// An mcp sensor reads degraded and does not poll until a probe records
/// two matching calls and a confirmation; a probe without the
/// confirmation records that and leaves it disabled.
#[test]
fn an_mcp_sensor_runs_only_once_a_confirmed_probe_holds() {
    let f = quiet("mcp-probe");
    let pids = mcp_sensor(&f, "watch", "list_items");
    let row = listed(&f, "watch");
    assert_eq!((row["kind"].as_str(), row["state"].as_str()), (Some("mcp"), Some("degraded")), "{row}");
    assert_eq!(row["detail"].as_str(), Some("no probe record; run `attend sensor probe watch`"), "{row}");

    let poll = f.attend(&["sensor", "poll", "watch", "--dry-run"]);
    assert_eq!(poll.status.code(), Some(3), "an unprobed sensor does not poll");
    assert!(String::from_utf8_lossy(&poll.stderr).contains("no probe record"), "{}", String::from_utf8_lossy(&poll.stderr));
    assert!(noted(&pids).is_empty(), "the refused poll started no server");

    let unconfirmed = f.attend(&["sensor", "probe", "watch", "--json"]);
    assert_eq!(unconfirmed.status.code(), Some(3), "a record without confirmation does not hold");
    let rec = doc(&unconfirmed.stdout);
    assert_eq!((rec["calls_match"].as_bool(), rec["confirmed"].as_bool(), rec["holds"].as_bool()), (Some(true), Some(false), Some(false)), "{rec}");
    assert!(listed(&f, "watch")["detail"].as_str().unwrap().contains("no confirmation"));

    let confirmed = doc(f.ok(&["sensor", "probe", "watch", "--confirm", "--json"]).as_bytes());
    assert_eq!((confirmed["calls_match"].as_bool(), confirmed["confirmed"].as_bool(), confirmed["holds"].as_bool()), (Some(true), Some(true), Some(true)), "{confirmed}");
    let text = user_file(&f, "watch");
    assert!(text.starts_with("# watch: an mcp sensor on the fixture server\nkind: mcp\n"), "the rest of the file is kept: {text}");
    assert!(text.contains("probe:") && text.contains("calls_match: true") && text.contains("confirmed: true"), "{text}");
    assert_eq!(listed(&f, "watch")["state"].as_str(), Some("active"));

    let out = doc(f.ok(&["sensor", "poll", "watch", "--dry-run", "--json"]).as_bytes());
    assert_eq!(out["failure"], Value::Null, "{out}");
    assert_eq!(out["result"], json!([{"magnitude": 3.0, "description": "new item a"}]), "{out}");
    assert_eq!(left_behind(&pids), Vec::<String>::new(), "each command ended the servers it started");
}

/// A tool that declares itself read-only and still changes its listing
/// when read fails the probe's comparison, and stays disabled.
#[test]
fn a_tool_whose_second_call_loses_items_fails_the_probe() {
    let f = quiet("mcp-drain");
    mcp_sensor(&f, "drain", "drain_items");
    let out = f.attend(&["sensor", "probe", "drain", "--confirm", "--json"]);
    assert_eq!(out.status.code(), Some(3));
    let rec = doc(&out.stdout);
    assert_eq!((rec["calls_match"].as_bool(), rec["confirmed"].as_bool()), (Some(false), Some(false)), "{rec}");
    assert_eq!(rec["difference"].as_str(), Some("trigger new: the second call did not return the item a"), "{rec}");
    let row = listed(&f, "drain");
    assert_eq!(row["state"].as_str(), Some("degraded"));
    assert!(row["detail"].as_str().unwrap().contains("did not return what the first did"), "{row}");
}

/// A tool without `readOnlyHint` is refused: the probe writes no record,
/// and a poll under a hand-written record fails saying why. A host
/// records the refusal as the sensor's state.
#[test]
fn a_tool_without_read_only_hint_is_refused_and_the_refusal_recorded() {
    let f = quiet("mcp-bare");
    let pids = mcp_sensor(&f, "bare", "bare");
    let refusal = "tool bare does not declare readOnlyHint: true, so attend does not call it; allow_unannotated: true calls it anyway";
    let probe = f.attend(&["sensor", "probe", "bare", "--confirm"]);
    assert_eq!(probe.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&probe.stderr).contains(refusal), "{}", String::from_utf8_lossy(&probe.stderr));
    assert!(!user_file(&f, "bare").contains("probe:"), "a refused probe writes no record");

    let items = f.home.join("bare.items.json");
    let record = format!("probe: {{server: sh, tool: bare, args: {{file: {}}}, calls_match: true, confirmed: true}}\n", items.display());
    f.sensor("bare", &(sensor_text("bare", "bare", &items, &pids) + &record));
    assert_eq!(listed(&f, "bare")["state"].as_str(), Some("active"), "the hand-written record holds");
    let out = doc(f.ok(&["sensor", "poll", "bare", "--dry-run", "--json"]).as_bytes());
    assert_eq!(out["failure"].as_str(), Some(refusal), "{out}");

    let host = Host::start(&f);
    host.wait_for("the host's poll", Duration::from_secs(20), || listed(&f, "bare")["state"] == "failing");
    assert_eq!(listed(&f, "bare")["detail"].as_str(), Some(refusal));
    drop(host);
    assert_eq!(left_behind(&pids), Vec::<String>::new());
}

/// Under the host, a trigger on the tool's result is a system signal to
/// the session. A sensor file change re-executes the host, which ends the
/// server it held first; the host's end leaves none.
#[test]
fn the_host_writes_an_mcp_trigger_as_a_system_signal_and_leaves_no_server() {
    let f = quiet("mcp-host");
    let pids = mcp_sensor(&f, "watch", "list_items");
    f.ok(&["sensor", "probe", "watch", "--confirm"]);
    assert_eq!(left_behind(&pids), Vec::<String>::new(), "the probe's server ended with it");
    let probed = pids.with_extension("probe-pids");
    std::fs::rename(&pids, &probed).unwrap();

    let host = Host::start(&f);
    host.wait_for("the signal", Duration::from_secs(20), || !from(&f, "watch").is_empty());
    let sent = from(&f, "watch");
    assert_eq!(sent.len(), 1, "{sent:?}");
    let s = agent_identity::parse_signal(agent_identity::signal::wire_line(&sent[0].1)).unwrap();
    assert_eq!((s.from.as_ref(), s.from_kind, s.to, s.message), ("system:watch", Some("system"), Some(f.sid.as_str()), "new item a"), "{}", sent[0].1);
    let first = noted(&pids);
    assert_eq!(first.len(), 1, "one server across the host's polls: {first:?}");

    let file = f.home.join("config").join("attend").join("sensors").join("watch.yaml");
    std::fs::write(&file, user_file(&f, "watch") + "# touched\n").unwrap();
    host.wait_for("the re-executed host's server", Duration::from_secs(30), || noted(&pids).len() >= 2);
    assert!(!alive(&first[0]), "the server from before the re-exec is still running");
    assert_eq!(from(&f, "watch").len(), 1, "the trigger kept for the session stays spent");

    drop(host);
    assert_eq!(left_behind(&pids), Vec::<String>::new(), "the host's end left a server running");
}

/// The signals sensor `sensor` wrote to the fixture's project tray, each
/// with its text.
fn from(f: &Fixture, sensor: &str) -> Vec<(String, String)> {
    let tray = f.signals().join(f.project_tray());
    let mut out: Vec<(String, String)> = std::fs::read_dir(tray)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "signal"))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
        .filter(|(_, t)| t.starts_with(&format!("system:{sensor}|")))
        .collect();
    out.sort();
    out
}

/// A running `attend sensor host` for the fixture's session, in a process
/// group of its own, its stderr in a file. Drop ends it as the mod's exit
/// would: SIGTERM, then SIGKILL after five seconds.
struct Host {
    child: Child,
    err: PathBuf,
}

impl Host {
    fn start(f: &Fixture) -> Host {
        let err = f.home.join("host.err");
        let child = f
            .command()
            .args(["sensor", "host", "--session", &f.sid, "--cwd", &f.origin])
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&err).unwrap()))
            .spawn()
            .unwrap();
        Host { child, err }
    }

    fn wait_for(&self, what: &str, limit: Duration, done: impl Fn() -> bool) {
        let start = Instant::now();
        while start.elapsed() < limit {
            if done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timed out waiting for {what}; the host logged:\n{}", std::fs::read_to_string(&self.err).unwrap_or_default());
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: kill(2) on the process group the unreaped child leads.
        unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGTERM) };
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.child.kill().ok();
        self.child.wait().ok();
    }
}
