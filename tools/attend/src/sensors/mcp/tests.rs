// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use super::*;
use serde_json::json;
use std::process::{Command as Process, Stdio};
use std::time::Instant;
use super::lookup::Vars;
use sensor_spec::{ProbeRecord, Server};
use std::path::Path;

fn fixture() -> String {
    format!("{}/tests/fixtures/mcp-server.sh", env!("CARGO_MANIFEST_DIR"))
}

/// A scratch directory for one test.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("attend-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The probe record a sensor on `server` calling `tool` holds.
fn record(server: &Server, tool: &str, args: &serde_yaml::Mapping) -> ProbeRecord {
    let binding = inline_binding(server);
    ProbeRecord { server: server.label().into(), binding, project: None, tool: tool.into(), args: args.clone(), calls_match: true, confirmed: true, at: None }
}

/// A lookup that reads no user file and no environment.
fn no_lookup(dir: &Path) -> Lookup {
    Lookup { user_json: dir.join("no-claude.json"), trusted: false, vars: |_| None }
}

/// An mcp sensor `t` on the fixture server in `mode`, calling `tool` on
/// the result file `items` holds, with a probe record bound to its server,
/// its pids and its ledger in `dir`.
fn sensor(dir: &Path, mode: &str, tool: &str, allow_unannotated: bool) -> Mcp {
    let items = dir.join("items.json");
    if !items.exists() {
        std::fs::write(&items, r#"{"items":[{"id":"a"}],"count":1}"#).unwrap();
    }
    let server = Server::Inline {
        command: "sh".into(),
        args: vec![fixture(), mode.into()],
        env: BTreeMap::from([("FIXTURE_PIDS".to_string(), dir.join("pids").display().to_string())]),
    };
    let mut args = serde_yaml::Mapping::new();
    args.insert("file".into(), items.display().to_string().into());
    let probe = Some(record(&server, tool, &args));
    let block = McpBlock { server, tool: tool.into(), args, allow_unannotated, probe };
    Mcp::new("t", block, dir.to_str().unwrap(), Inherit::Stripped, no_lookup(dir)).with_ledger(&dir.join("ledger"))
}

fn quick(m: Mcp) -> Mcp {
    let step = Duration::from_secs(1);
    m.with_limits(Limits { start: step, list: step, call: step })
}

fn pids(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("pids")).unwrap_or_default().lines().map(str::to_string).collect()
}

fn alive(pid: &str) -> bool {
    Process::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
}

/// Wait up to five seconds for every pid in `dir` to be gone, and name
/// any that is not.
fn left_behind(dir: &Path) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let live: Vec<String> = pids(dir).into_iter().filter(|p| alive(p)).collect();
        if live.is_empty() || Instant::now() > until {
            return live;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_read_only_tool_s_structured_result_is_the_poll_result() {
    let dir = scratch("structured");
    let mut m = sensor(&dir, "serve", "list_items", false);
    let Ok(Fetched::Json(v)) = m.fetch() else { panic!("{:?}", m.call()) };
    assert_eq!(v, json!({"items": [{"id": "a"}], "count": 1}));
    std::fs::write(dir.join("items.json"), r#"{"items":[],"count":0}"#).unwrap();
    assert_eq!(m.call().unwrap(), json!({"items": [], "count": 0}), "the next poll calls again");
    assert_eq!(pids(&dir).len(), 1, "one server across polls");
    drop(m);
    assert_eq!(left_behind(&dir), Vec::<String>::new(), "dropping the last sensor ends its server");
}

#[test]
fn a_text_result_that_is_json_is_read_as_json() {
    let dir = scratch("text");
    let mut m = sensor(&dir, "serve", "text_items", false);
    assert_eq!(m.call().unwrap(), json!({"items": [{"id": "a"}], "count": 1}));
}

/// A tool with no `readOnlyHint` is refused unless the file allows it,
/// and one that declares `readOnlyHint: false` is refused either way.
#[test]
fn a_tool_not_declared_read_only_is_refused() {
    let dir = scratch("readonly");
    let refused = sensor(&dir, "serve", "bare", false).call().unwrap_err();
    assert_eq!(refused, "tool bare does not declare readOnlyHint: true, so attend does not call it; allow_unannotated: true calls it anyway");
    assert!(sensor(&dir, "serve", "bare", true).call().is_ok(), "allow_unannotated calls it");
    for allow in [false, true] {
        assert_eq!(sensor(&dir, "serve", "mark_read", allow).call().unwrap_err(), "tool mark_read declares readOnlyHint: false, so attend does not call it");
    }
    assert_eq!(sensor(&dir, "serve", "nope", true).call().unwrap_err(), "server sh has no tool nope");
}

/// A server that answers nothing fails the poll at its start limit,
/// and neither it nor what it started is left running.
#[test]
fn a_server_that_hangs_times_out_and_leaves_no_process() {
    let dir = scratch("hang");
    let started = Instant::now();
    let why = quick(sensor(&dir, "hang", "list_items", false)).call().unwrap_err();
    assert_eq!(why, "server sh timed out after 1s at initialize");
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert_eq!(pids(&dir).len(), 2, "the server and its child started");
    assert_eq!(left_behind(&dir), Vec::<String>::new());
}

/// A call that never returns fails the poll at the call limit and ends
/// the server's group; the next poll starts it again.
#[test]
fn a_call_that_hangs_times_out_and_ends_the_server() {
    let dir = scratch("hang-call");
    let mut m = quick(sensor(&dir, "hang-call", "list_items", false));
    assert_eq!(m.call().unwrap_err(), "server sh timed out after 1s at tools/call");
    assert_eq!(left_behind(&dir), Vec::<String>::new());
    let before = pids(&dir).len();
    assert!(m.call().is_err());
    assert!(pids(&dir).len() > before, "the next poll started a new server");
    assert_eq!(left_behind(&dir), Vec::<String>::new());
}

#[test]
fn a_server_that_crashes_says_how() {
    let dir = scratch("crash");
    assert_eq!(sensor(&dir, "crash", "list_items", false).call().unwrap_err(), "server sh exited with status 3: fixture crashed on purpose");
    assert_eq!(left_behind(&dir), Vec::<String>::new());
}

/// A server that exits between polls fails the next poll, which says
/// so, and the poll after starts it again.
#[test]
fn a_server_that_exits_is_restarted_on_the_next_poll() {
    let dir = scratch("once");
    let mut m = sensor(&dir, "once", "list_items", false);
    assert!(m.call().is_ok());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(m.call().unwrap_err(), "server sh exited with status 0");
    assert!(m.call().is_ok(), "restarted");
    assert_eq!(pids(&dir).len(), 2);
}

/// Two sensors on one launch share its server.
#[test]
fn sensors_on_one_server_share_it() {
    let dir = scratch("shared");
    let (mut a, mut b) = (sensor(&dir, "serve", "list_items", false), sensor(&dir, "serve", "text_items", false));
    assert!(a.call().is_ok() && b.call().is_ok());
    assert_eq!(pids(&dir).len(), 1);
    drop(a);
    assert!(b.call().is_ok(), "the other sensor keeps it");
    assert_eq!(pids(&dir).len(), 1);
    drop(b);
    assert_eq!(left_behind(&dir), Vec::<String>::new(), "the last sensor's drop ends it");
}

#[test]
fn a_missing_program_cannot_start() {
    let dir = scratch("missing");
    let mut m = sensor(&dir, "serve", "list_items", false);
    m.block.server = Server::Inline { command: "/nonexistent/mcp-server".into(), args: vec![], env: BTreeMap::new() };
    m.block.probe = Some(record(&m.block.server, "list_items", &m.block.args));
    assert!(m.call().unwrap_err().starts_with("cannot start server /nonexistent/mcp-server: "));
}

/// A named server is the project's entry in `~/.claude.json`, then the
/// project's `.mcp.json` when the project is trusted, then the user's
/// entry; only a stdio server runs.
#[test]
fn a_named_server_is_looked_up_as_claude_code_does() {
    let dir = scratch("lookup");
    let project = dir.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let user_json = dir.join("claude.json");
    let p = project.display().to_string();
    std::fs::write(
        &user_json,
        json!({
            "mcpServers": { "u": { "command": "user-u" }, "both": { "command": "user-both" }, "web": { "type": "http", "url": "https://example.invalid" } },
            "projects": { p.clone(): { "mcpServers": { "local": { "type": "stdio", "command": "local-cmd", "args": ["-x"], "env": { "K": "v" } } } } }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(project.join(".mcp.json"), json!({ "mcpServers": { "both": { "command": "repo-both" } } }).to_string()).unwrap();
    let at = |name: &str, trusted: bool| resolve(&Server::Named(name.into()), &project, Inherit::Stripped, &Lookup { user_json: user_json.clone(), trusted, vars: |_| None });
    let local = at("local", false).unwrap();
    assert_eq!((local.program.as_str(), local.args.clone(), local.env.get("K").map(String::as_str), local.inherit), ("local-cmd", vec!["-x".to_string()], Some("v"), Inherit::Stripped));
    assert_eq!(at("u", false).unwrap().program, "user-u");
    let repo = at("both", true).unwrap();
    assert_eq!((repo.program.as_str(), repo.inherit), ("repo-both", Inherit::Project), "a .mcp.json server takes the project rule");
    assert!(at("both", false).unwrap_err().contains("the project is not in attend.project_sensors"));
    assert_eq!(at("web", false).unwrap_err(), "server web is a http server; attend runs stdio servers only");
    assert!(at("none", false).unwrap_err().starts_with("no MCP server named none in "));
}

/// A poll runs only under a record bound to the server as it is defined
/// now: an inline server's changed args void it.
#[test]
fn a_record_taken_with_another_server_definition_refuses_the_poll() {
    let dir = scratch("binding");
    let mut m = sensor(&dir, "serve", "list_items", false);
    let Server::Inline { args, .. } = &mut m.block.server else { panic!() };
    args.push("--extra".into());
    assert_eq!(m.call().unwrap_err(), "the probe record was not taken with server sh as it is defined now; run `attend sensor probe t`");
    assert!(pids(&dir).is_empty(), "the refused poll started no server");
    assert!(m.probe_call().is_ok(), "the probe itself takes the record, so no record binds it");
}

/// A server that keeps running when its input closes gets TERM, and one
/// that exits on TERM is not killed: its TERM handler ran.
#[test]
fn an_ending_server_gets_term_before_kill() {
    let dir = scratch("term");
    let mut m = sensor(&dir, "deaf", "list_items", false);
    assert!(m.call().is_ok());
    assert_eq!(std::fs::read_dir(dir.join("ledger")).unwrap().count(), 1, "the running server is in the ledger");
    let started = Instant::now();
    drop(m);
    assert_eq!(left_behind(&dir), Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(dir.join("pids.term")).unwrap_or_default(), "term\n", "the server handled TERM");
    assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
    let entries: usize = std::fs::read_dir(dir.join("ledger")).unwrap().flatten().map(|d| std::fs::read_dir(d.path()).unwrap().count()).sum();
    assert_eq!(entries, 0, "the ended server's entry is removed");
}

/// The sweep leaves a server whose owner runs, such as another host's,
/// and ends one whose owner is gone.
#[test]
fn the_sweep_ends_only_servers_whose_owner_is_gone() {
    let dir = scratch("sweep");
    let mut m = sensor(&dir, "deaf", "list_items", false);
    assert!(m.call().is_ok());
    assert_eq!(ledger::sweep(&dir.join("ledger"), &dir), 0, "this process owns it and runs");
    assert_eq!(left_behind_now(&dir), pids(&dir), "the sweep left a live owner's server running");

    let mut orphan = Process::new("sh");
    orphan.args(["-c", "sleep 600"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut orphan, 0);
    let mut orphan = orphan.spawn().unwrap();
    let group = orphan.id();
    let mut done = Process::new("true").spawn().unwrap();
    let gone = done.id();
    done.wait().unwrap();
    let entry = json!({ "group": group, "leader_start": attend_presence::process::start_time(group), "owner": gone, "owner_start": "a process that ended" });
    let at = std::fs::read_dir(dir.join("ledger")).unwrap().flatten().next().unwrap().path();
    std::fs::write(at.join(format!("{group}.json")), entry.to_string()).unwrap();
    let swept = ledger::sweep(&dir.join("ledger"), &dir);
    let until = Instant::now() + Duration::from_secs(3);
    while orphan.try_wait().unwrap().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    let ended = orphan.try_wait().unwrap().is_some();
    if !ended {
        // SAFETY: kill(2) on the group the unreaped child leads, so a
        // failed sweep leaves nothing running.
        unsafe { libc::kill(-(group as libc::pid_t), libc::SIGKILL) };
        orphan.wait().ok();
    }
    assert_eq!((swept, ended), (1, true), "the orphaned group was not ended");
    assert!(!at.join(format!("{group}.json")).exists(), "its entry is removed");
    drop(m);
    assert_eq!(left_behind(&dir), Vec::<String>::new());
}

/// The pids of `dir` running now.
fn left_behind_now(dir: &Path) -> Vec<String> {
    pids(dir).into_iter().filter(|p| alive(p)).collect()
}

/// A `~/.claude.json` entry expands `${NAME}` and `${NAME:-default}` from
/// attend's environment and names a reference it cannot expand; a
/// repository's `.mcp.json` entry expands nothing and is refused, naming
/// the reference.
#[test]
fn a_user_entry_expands_variables_and_a_repository_entry_is_refused() {
    let dir = scratch("vars");
    let project = dir.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let user_json = dir.join("claude.json");
    std::fs::write(
        &user_json,
        json!({ "mcpServers": {
            "u": { "command": "${MCP_BIN}", "args": ["--at", "${MCP_HOME:-/fallback}/x"], "env": { "TOKEN": "${MCP_TOKEN}" } },
            "unset": { "command": "srv", "env": { "TOKEN": "${MCP_MISSING}" } }
        } })
        .to_string(),
    )
    .unwrap();
    std::fs::write(project.join(".mcp.json"), json!({ "mcpServers": { "repo": { "command": "srv", "env": { "KEY": "${HOME}" } } } }).to_string()).unwrap();
    let vars: Vars = |k| match k {
        "MCP_BIN" => Some("/bin/srv".into()),
        "MCP_TOKEN" => Some("t0k".into()),
        _ => None,
    };
    let at = |name: &str| resolve(&Server::Named(name.into()), &project, Inherit::Stripped, &Lookup { user_json: user_json.clone(), trusted: true, vars });
    let u = at("u").unwrap();
    assert_eq!((u.program.as_str(), u.args.clone(), u.env.get("TOKEN").map(String::as_str)), ("/bin/srv", vec!["--at".to_string(), "/fallback/x".to_string()], Some("t0k")));
    assert!(u.binding.starts_with("user:"), "{}", u.binding);
    assert_eq!(at("unset").unwrap_err(), "server unset uses ${MCP_MISSING}, which is not set in attend's environment, so it is left unexpanded");
    assert_eq!(at("repo").unwrap_err(), "server repo in the project's .mcp.json uses ${HOME}; attend does not expand variables for a repository's server");
}

/// A malformed `.mcp.json` is read only when the project's entry misses,
/// leaves the user's servers usable, and fails only a server nothing else
/// defines.
#[test]
fn a_malformed_mcp_json_fails_only_a_server_it_alone_could_define() {
    let dir = scratch("malformed");
    let project = dir.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let user_json = dir.join("claude.json");
    let p = project.display().to_string();
    std::fs::write(&user_json, json!({ "mcpServers": { "u": { "command": "user-u" } }, "projects": { p: { "mcpServers": { "local": { "command": "local-cmd" } } } } }).to_string()).unwrap();
    std::fs::write(project.join(".mcp.json"), "{ not json").unwrap();
    let at = |name: &str| resolve(&Server::Named(name.into()), &project, Inherit::Stripped, &Lookup { user_json: user_json.clone(), trusted: true, vars: |_| None });
    assert_eq!(at("local").unwrap().program, "local-cmd");
    assert_eq!(at("u").unwrap().program, "user-u");
    let why = at("none").unwrap_err();
    assert!(why.contains(".mcp.json is not JSON"), "{why}");
}

/// A named server's binding follows its definition, and an inline
/// server's follows its command, args and env.
#[test]
fn a_binding_changes_with_the_definition() {
    let dir = scratch("bind");
    let user_json = dir.join("claude.json");
    let at = |def: Value| {
        std::fs::write(&user_json, json!({ "mcpServers": { "s": def } }).to_string()).unwrap();
        resolve(&Server::Named("s".into()), &dir, Inherit::Stripped, &Lookup { user_json: user_json.clone(), trusted: false, vars: |_| Some("v".into()) }).unwrap().binding
    };
    let one = at(json!({ "command": "srv", "env": { "T": "${X}" } }));
    assert_eq!(one, at(json!({ "command": "srv", "env": { "T": "${X}" } })));
    assert_ne!(one, at(json!({ "command": "srv", "env": { "T": "${Y}" } })));
    assert!(!one.contains("srv"), "the binding holds no value of the definition: {one}");
    let inline = |args: &[&str]| inline_binding(&Server::Inline { command: "srv".into(), args: args.iter().map(|a| a.to_string()).collect(), env: BTreeMap::new() });
    assert_ne!(inline(&["a"]), inline(&["b"]));
}
