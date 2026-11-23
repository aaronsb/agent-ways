// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! An mcp sensor end to end, on a stdio server written in sh: it stays
//! disabled until `attend sensor probe` records two matching calls and the
//! operator's confirmation at a terminal, the record lives in the user's
//! file and is bound to the server's definition, a tool that is not
//! declared read-only is refused and the refusal recorded, and under the
//! sensor host a trigger on the tool's result becomes a system signal. No
//! server outlives the process that started it: not a re-exec, not a host
//! ended by TERM, and not a host killed with SIGKILL, whose servers the
//! next host ends.
//!
//! The tests have no terminal, so a probe records `confirmed: false`; a
//! test stands in for the operator's answer by editing the record.

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
/// on the fixture server in `mode` over the items file at `items`, its
/// servers' pids noted in `pids`, with one trigger that fires for each
/// item it has not seen this session.
fn sensor_text(name: &str, mode: &str, tool: &str, items: &Path, pids: &Path) -> String {
    format!(
        "# {name}: an mcp sensor on the fixture server\n\
         kind: mcp\ninterval: 1\nmin_interval: 1\nthreshold: 1\n\
         server: {{command: sh, args: [{script}, {mode}], env: {{FIXTURE_PIDS: {pids}}}}}\n\
         tool: {tool}\nargs: {{file: {items}}}\n\
         triggers:\n  - {{name: new, path: \"$.items[*]\", key: id, on: added, memory: session, magnitude: 3, description: \"new item {{id}}\"}}\n",
        script = server_script(),
        pids = pids.display(),
        items = items.display(),
    )
}

/// The items file and pid file of the sensor `name`, with items `a`.
fn files(f: &Fixture, name: &str) -> (PathBuf, PathBuf) {
    let items = f.home.join(format!("{name}.items.json"));
    std::fs::write(&items, json!({"items": [{"id": "a"}]}).to_string()).unwrap();
    (items, f.home.join(format!("{name}.pids")))
}

/// Write the user sensor `name` on `tool` of the fixture server in `mode`,
/// and return the path of its pid file.
fn mcp_sensor(f: &Fixture, name: &str, mode: &str, tool: &str) -> PathBuf {
    let (items, pids) = files(f, name);
    f.sensor(name, &sensor_text(name, mode, tool, &items, &pids));
    pids
}

fn user_path(f: &Fixture, name: &str) -> PathBuf {
    f.home.join("config").join("attend").join("sensors").join(format!("{name}.yaml"))
}

fn user_file(f: &Fixture, name: &str) -> String {
    std::fs::read_to_string(user_path(f, name)).unwrap()
}

/// Probe `name` with no terminal, which records the calls unconfirmed,
/// and return what it printed.
fn probe(f: &Fixture, name: &str) -> Value {
    let out = f.attend(&["sensor", "probe", name, "--json"]);
    assert_eq!(out.status.code(), Some(3), "an unconfirmed record does not hold: {}", String::from_utf8_lossy(&out.stderr));
    doc(&out.stdout)
}

/// Probe `name`, then answer yes for the operator, as a terminal would.
fn probe_and_confirm(f: &Fixture, name: &str) {
    let rec = probe(f, name);
    assert_eq!((rec["calls_match"].as_bool(), rec["confirmed"].as_bool()), (Some(true), Some(false)), "{rec}");
    let path = user_path(f, name);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("confirmed: false").count(), 1, "{text}");
    std::fs::write(&path, text.replace("confirmed: false", "confirmed: true")).unwrap();
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

/// The pids of `file` still running after up to eight seconds.
fn left_behind(file: &Path) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        let live: Vec<String> = noted(file).into_iter().filter(|p| alive(p)).collect();
        if live.is_empty() || Instant::now() > until {
            return live;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// An mcp sensor reads degraded and does not poll until a probe records
/// two matching calls and a confirmation; with no terminal the probe
/// records none and says where to answer.
#[test]
fn an_mcp_sensor_runs_only_once_a_confirmed_probe_holds() {
    let f = quiet("mcp-probe");
    let pids = mcp_sensor(&f, "watch", "serve", "list_items");
    let row = listed(&f, "watch");
    assert_eq!((row["kind"].as_str(), row["state"].as_str()), (Some("mcp"), Some("degraded")), "{row}");
    assert_eq!(row["detail"].as_str(), Some("no probe record; run `attend sensor probe watch` at a terminal"), "{row}");

    let poll = f.attend(&["sensor", "poll", "watch", "--dry-run"]);
    assert_eq!(poll.status.code(), Some(3), "an unprobed sensor does not poll");
    assert!(String::from_utf8_lossy(&poll.stderr).contains("no probe record"), "{}", String::from_utf8_lossy(&poll.stderr));
    assert!(noted(&pids).is_empty(), "the refused poll started no server");

    let rec = probe(&f, "watch");
    assert_eq!((rec["calls_match"].as_bool(), rec["confirmed"].as_bool(), rec["holds"].as_bool()), (Some(true), Some(false), Some(false)), "{rec}");
    assert!(rec["then"].as_str().unwrap().contains("`! attend sensor probe watch` in Claude Code"), "{rec}");
    assert!(rec["binding"].as_str().unwrap().starts_with("inline:"), "{rec}");
    assert!(listed(&f, "watch")["detail"].as_str().unwrap().contains("no confirmation"));
    let text = user_file(&f, "watch");
    assert!(text.starts_with("# watch: an mcp sensor on the fixture server\nkind: mcp\n"), "the rest of the file is kept: {text}");

    probe_and_confirm(&f, "watch");
    assert_eq!(listed(&f, "watch")["state"].as_str(), Some("active"));
    let out = doc(f.ok(&["sensor", "poll", "watch", "--dry-run", "--json"]).as_bytes());
    assert_eq!(out["failure"], Value::Null, "{out}");
    assert_eq!(out["result"], json!([{"magnitude": 3.0, "description": "new item a"}]), "{out}");
    assert_eq!(left_behind(&pids), Vec::<String>::new(), "each command ended the servers it started");
}

/// A poll of a disabled mcp sensor with no record that holds is refused
/// too, and starts no server.
#[test]
fn a_disabled_unprobed_sensor_does_not_poll() {
    let f = quiet("mcp-off");
    let (items, pids) = files(&f, "off");
    f.sensor("off", &(sensor_text("off", "serve", "list_items", &items, &pids) + "enabled: false\n"));
    assert_eq!(listed(&f, "off")["state"].as_str(), Some("off"));
    let poll = f.attend(&["sensor", "poll", "off", "--dry-run"]);
    assert_eq!(poll.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&poll.stderr).contains("off has no probe record that holds: no probe record"), "{}", String::from_utf8_lossy(&poll.stderr));
    assert!(noted(&pids).is_empty(), "the refused poll started no server");
}

/// A tool that declares itself read-only and still changes its listing
/// when read fails the probe's comparison, and stays disabled.
#[test]
fn a_tool_whose_second_call_loses_items_fails_the_probe() {
    let f = quiet("mcp-drain");
    mcp_sensor(&f, "drain", "serve", "drain_items");
    let rec = probe(&f, "drain");
    assert_eq!((rec["calls_match"].as_bool(), rec["confirmed"].as_bool()), (Some(false), Some(false)), "{rec}");
    assert_eq!(rec["difference"].as_str(), Some("trigger new: the second call did not return the item a"), "{rec}");
    let row = listed(&f, "drain");
    assert_eq!(row["state"].as_str(), Some("degraded"));
    assert!(row["detail"].as_str().unwrap().contains("did not return what the first did"), "{row}");
}

/// A tool without `readOnlyHint` is refused: the probe writes no record,
/// and a poll under a record taken on the same server fails saying why. A
/// host records the refusal as the sensor's state.
#[test]
fn a_tool_without_read_only_hint_is_refused_and_the_refusal_recorded() {
    let f = quiet("mcp-bare");
    let pids = mcp_sensor(&f, "bare", "serve", "bare");
    let refusal = "tool bare does not declare readOnlyHint: true, so attend does not call it; allow_unannotated: true calls it anyway";
    let out = f.attend(&["sensor", "probe", "bare"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains(refusal), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!user_file(&f, "bare").contains("probe:"), "a refused probe writes no record");

    // A record taken on the same server with a readable tool, then pointed
    // at bare, as a hand edit would.
    mcp_sensor(&f, "bare", "serve", "list_items");
    probe_and_confirm(&f, "bare");
    std::fs::write(user_path(&f, "bare"), user_file(&f, "bare").replace("tool: list_items", "tool: bare")).unwrap();
    assert_eq!(listed(&f, "bare")["state"].as_str(), Some("active"), "the edited record holds");
    let out = doc(f.ok(&["sensor", "poll", "bare", "--dry-run", "--json"]).as_bytes());
    assert_eq!(out["failure"].as_str(), Some(refusal), "{out}");

    let host = Host::start(&f);
    host.wait_for("the host's poll", Duration::from_secs(20), || listed(&f, "bare")["state"] == "failing");
    assert_eq!(listed(&f, "bare")["detail"].as_str(), Some(refusal));
    drop(host);
    assert_eq!(left_behind(&pids), Vec::<String>::new());
}

/// The record is bound to the server's definition: an inline server's
/// changed args void it at once, and a named server's changed definition
/// voids it at the next call.
#[test]
fn a_changed_server_definition_voids_the_record() {
    let f = quiet("mcp-bind");
    mcp_sensor(&f, "inline", "serve", "list_items");
    probe_and_confirm(&f, "inline");
    assert_eq!(listed(&f, "inline")["state"].as_str(), Some("active"));
    std::fs::write(user_path(&f, "inline"), user_file(&f, "inline").replace(", serve]", ", serve, --extra]")).unwrap();
    let row = listed(&f, "inline");
    assert_eq!((row["state"].as_str(), row["detail"].as_str()), (Some("degraded"), Some("the probe record was taken with another server, tool or args; run `attend sensor probe inline` at a terminal")), "{row}");

    let (items, pids) = files(&f, "named");
    let entry = |env: Value| json!({ "mcpServers": { "fx": { "command": "sh", "args": [server_script(), "serve"], "env": env } } }).to_string();
    let claude_json = f.home.join(".claude.json");
    std::fs::write(&claude_json, entry(json!({ "FIXTURE_PIDS": pids }))).unwrap();
    f.sensor("named", &sensor_text("named", "serve", "list_items", &items, &pids).replace(&format!("server: {{command: sh, args: [{}, serve], env: {{FIXTURE_PIDS: {}}}}}", server_script(), pids.display()), "server: fx"));
    probe_and_confirm(&f, "named");
    let out = doc(f.ok(&["sensor", "poll", "named", "--dry-run", "--json"]).as_bytes());
    assert_eq!(out["failure"], Value::Null, "{out}");
    std::fs::write(&claude_json, entry(json!({ "FIXTURE_PIDS": pids, "EXTRA": "1" }))).unwrap();
    let out = doc(f.ok(&["sensor", "poll", "named", "--dry-run", "--json"]).as_bytes());
    assert_eq!(out["failure"].as_str(), Some("the probe record was not taken with server fx as it is defined now; run `attend sensor probe named`"), "{out}");
    assert_eq!(left_behind(&pids), Vec::<String>::new());
}

/// A project's sensor keeps its record in a user overlay naming the
/// project; a record in the project's own file is ignored, with a note.
#[test]
fn a_project_sensor_s_record_lives_in_the_user_overlay() {
    let f = quiet("mcp-project");
    std::fs::write(f.home.join("config").join("attend").join("config.yaml"), format!("cleanup:\n  enabled: false\nproject_sensors:\n  - {}\n", f.origin)).unwrap();
    let (items, pids) = files(&f, "repo");
    let dir = Path::new(&f.origin).join(".claude").join("attend").join("sensors");
    std::fs::create_dir_all(&dir).unwrap();
    let text = sensor_text("repo", "serve", "list_items", &items, &pids);
    std::fs::write(dir.join("repo.yaml"), &text).unwrap();
    assert_eq!(listed(&f, "repo")["state"].as_str(), Some("degraded"));

    probe_and_confirm(&f, "repo");
    let overlay = user_file(&f, "repo");
    let key = std::fs::canonicalize(&f.origin).unwrap().display().to_string();
    assert!(overlay.starts_with("probes:") && overlay.contains(&key), "an overlay holds the records alone, by project: {overlay}");
    assert!(!overlay.contains("kind:"), "{overlay}");
    assert_eq!(listed(&f, "repo")["state"].as_str(), Some("active"));

    std::fs::write(dir.join("repo.yaml"), format!("{text}{overlay}")).unwrap();
    std::fs::remove_file(user_path(&f, "repo")).unwrap();
    let row = listed(&f, "repo");
    assert_eq!((row["state"].as_str(), row["detail"].as_str()), (Some("degraded"), Some("no probe record for this project; run `attend sensor probe repo` at a terminal")), "{row}");
    assert!(row["notes"].to_string().contains("a probe record is read only from your user sensor file; this one in a project file is ignored"), "{row}");
}

/// Two projects with a sensor of the same name each keep their record: a
/// probe writes only its own project's entry.
#[test]
fn same_named_sensors_of_two_projects_keep_a_record_each() {
    let f = quiet("mcp-two");
    let second = f.home.join("second");
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(f.home.join("config").join("attend").join("config.yaml"), format!("cleanup:\n  enabled: false\nproject_sensors:\n  - {}\n  - {}\n", f.origin, second.display())).unwrap();
    let (items, pids) = files(&f, "repo");
    let text = sensor_text("repo", "serve", "list_items", &items, &pids);
    for project in [Path::new(&f.origin), second.as_path()] {
        let dir = project.join(".claude").join("attend").join("sensors");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("repo.yaml"), &text).unwrap();
    }
    // The second project is reached by working directory, with no session.
    let in_second = |args: &[&str]| f.without_session(|| f.command().current_dir(&second).args(args).output().unwrap());
    let state_in_second = || {
        let all = doc(&in_second(&["sensor", "list", "--json"]).stdout);
        all.as_array().unwrap().iter().find(|e| e["name"] == "repo").unwrap()["state"].as_str().unwrap().to_string()
    };

    probe_and_confirm(&f, "repo");
    assert_eq!(listed(&f, "repo")["state"].as_str(), Some("active"));
    assert_eq!(state_in_second(), "degraded", "the first project's record is not the second's");

    let out = in_second(&["sensor", "probe", "repo", "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stderr));
    let path = user_path(&f, "repo");
    std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("confirmed: false", "confirmed: true")).unwrap();
    let overlay = user_file(&f, "repo");
    for project in [Path::new(&f.origin), second.as_path()] {
        let key = std::fs::canonicalize(project).unwrap().display().to_string();
        assert!(overlay.contains(&key), "{key} has no record: {overlay}");
    }
    assert_eq!(listed(&f, "repo")["state"].as_str(), Some("active"), "the second probe kept the first project's record");
    assert_eq!(state_in_second(), "active");
    assert_eq!(left_behind(&pids), Vec::<String>::new());
}

/// Under the host, a trigger on the tool's result is a system signal to
/// the session. A sensor file change re-executes the host, which ends the
/// server it held first; TERM ends the host, which ends its servers, even
/// one that keeps running when its input closes.
#[test]
fn the_host_writes_an_mcp_trigger_as_a_system_signal_and_leaves_no_server() {
    let f = quiet("mcp-host");
    let pids = mcp_sensor(&f, "watch", "deaf", "list_items");
    probe_and_confirm(&f, "watch");
    assert_eq!(left_behind(&pids), Vec::<String>::new(), "the probe's servers ended with it");
    std::fs::remove_file(&pids).unwrap();

    let host = Host::start(&f);
    host.wait_for("the signal", Duration::from_secs(20), || !from(&f, "watch").is_empty());
    let sent = from(&f, "watch");
    assert_eq!(sent.len(), 1, "{sent:?}");
    let s = agent_identity::parse_signal(agent_identity::signal::wire_line(&sent[0].1)).unwrap();
    assert_eq!((s.from.as_ref(), s.from_kind, s.to, s.message), ("system:watch", Some("system"), Some(f.sid.as_str()), "new item a"), "{}", sent[0].1);
    let first = noted(&pids);
    assert_eq!(first.len(), 1, "one server across the host's polls: {first:?}");

    std::fs::write(user_path(&f, "watch"), user_file(&f, "watch") + "# touched\n").unwrap();
    host.wait_for("the re-executed host's server", Duration::from_secs(30), || noted(&pids).len() >= 3);
    assert!(!alive(&first[0]), "the server from before the re-exec is still running");
    assert_eq!(from(&f, "watch").len(), 1, "the trigger kept for the session stays spent");

    drop(host);
    assert_eq!(left_behind(&pids), Vec::<String>::new(), "the host's end left a server running");
}

/// A host killed with SIGKILL ends nothing, and its server runs on; the
/// next host for the project ends it before it starts its own.
#[test]
fn the_next_host_ends_the_servers_a_killed_host_left() {
    let f = quiet("mcp-sweep");
    let pids = mcp_sensor(&f, "watch", "deaf", "list_items");
    probe_and_confirm(&f, "watch");
    assert_eq!(left_behind(&pids), Vec::<String>::new());
    std::fs::remove_file(&pids).unwrap();

    let host = Host::start(&f);
    // Killed once its server has answered, so the server is waiting on its
    // input and not writing to a reader that is gone.
    host.wait_for("the host's poll", Duration::from_secs(20), || !from(&f, "watch").is_empty());
    let orphan = noted(&pids)[0].clone();
    host.kill();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(alive(&orphan), "a server that ignores the end of its input outlives a killed host");

    let next = Host::start(&f);
    next.wait_for("the sweep", Duration::from_secs(10), || !alive(&orphan));
    drop(next);
    assert_eq!(left_behind(&pids), Vec::<String>::new());
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
        let n = std::fs::read_dir(&f.home).map(|d| d.count()).unwrap_or(0);
        let err = f.home.join(format!("host-{n}.err"));
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

    /// End it at once, as `kill -9` would.
    fn kill(mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(None)) {
            return;
        }
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
