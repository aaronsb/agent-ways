// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use super::*;
use sensor_spec::Server;
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

/// An mcp sensor on the fixture server in `mode`, calling `tool` on the
/// result file `items` holds, its pids noted in `dir`.
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
    let block = McpBlock { server, tool: tool.into(), args, allow_unannotated, probe: None };
    let lookup = Lookup { user_json: dir.join("no-claude.json"), trusted: false };
    Mcp::new(block, dir.to_str().unwrap(), Inherit::Stripped, lookup)
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

/// Wait up to two seconds for every pid in `dir` to be gone, and name
/// any that is not.
fn left_behind(dir: &Path) -> Vec<String> {
    let until = Instant::now() + Duration::from_secs(2);
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
    let at = |name: &str, trusted: bool| resolve(&Server::Named(name.into()), &project, Inherit::Stripped, &Lookup { user_json: user_json.clone(), trusted });
    let local = at("local", false).unwrap();
    assert_eq!((local.program.as_str(), local.args.clone(), local.env.get("K").map(String::as_str), local.inherit), ("local-cmd", vec!["-x".to_string()], Some("v"), Inherit::Stripped));
    assert_eq!(at("u", false).unwrap().program, "user-u");
    let repo = at("both", true).unwrap();
    assert_eq!((repo.program.as_str(), repo.inherit), ("repo-both", Inherit::Project), "a .mcp.json server takes the project rule");
    assert!(at("both", false).unwrap_err().contains("the project is not in attend.project_sensors"));
    assert_eq!(at("web", false).unwrap_err(), "server web is a http server; attend runs stdio servers only");
    assert!(at("none", false).unwrap_err().starts_with("no MCP server named none in "));
}
