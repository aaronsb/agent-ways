//! `ways lookup`, the interface behind the MCP lookup tools (ADR-701 §5).
//!
//! Drives the binary against an isolated HOME, state and cache, and asserts on
//! the JSON it prints, the on-disk session stamps and the event log.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn ways_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ways"))
}

/// Mirror of the production resolver, as in `session_sim.rs`.
fn sessions_root() -> String {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/claude-sessions");
    }
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        format!("{base}/claude-ways/sessions")
    }
    #[cfg(not(windows))]
    {
        let uid = Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "0".to_string());
        format!("/tmp/.claude-sessions-{uid}")
    }
}

struct Env {
    base: PathBuf,
    home: PathBuf,
    project: PathBuf,
    session: String,
}

impl Env {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("ways-lookup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let project = base.join("project");
        std::fs::create_dir_all(home.join(".claude/hooks/ways")).unwrap();
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        let session = format!("lookup-{name}-{}", std::process::id());
        let _ = std::fs::remove_dir_all(format!("{}/{session}", sessions_root()));
        Env { base, home, project, session }
    }

    fn ways_root(&self) -> PathBuf {
        self.home.join(".claude/hooks/ways")
    }

    fn state(&self) -> PathBuf {
        self.base.join("state")
    }

    fn cache(&self) -> PathBuf {
        self.base.join("cache")
    }

    fn way(&self, id: &str, front: &str, body: &str) {
        let (dir, leaf) = (self.ways_root().join(id), id.rsplit('/').next().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{leaf}.md")), format!("---\n{front}---\n{body}")).unwrap();
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(ways_bin());
        c.current_dir(&self.project)
            .env("PWD", &self.project)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CACHE_HOME", self.cache())
            .env("XDG_STATE_HOME", self.state())
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_BIN_HOME", self.home.join(".local/bin"))
            .env("CLAUDE_CONTEXT_WINDOW", "100000");
        for var in ["CLAUDE_PROJECT_DIR", "CLAUDE_AGENT_ID", "CLAUDE_SESSION_ID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
            c.env_remove(var);
        }
        c.args(args);
        c
    }

    /// Run a `ways lookup` verb; the JSON object it printed and the exit code.
    fn lookup(&self, args: &[&str]) -> (Value, i32) {
        let project = self.project.to_string_lossy().into_owned();
        let mut full = vec!["lookup", "--project", project.as_str()];
        full.extend_from_slice(args);
        let out = self.cmd(&full).output().expect("run ways lookup");
        let json = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("stdout is one JSON object: {e}: {}", String::from_utf8_lossy(&out.stdout)));
        (json, out.status.code().unwrap_or(-1))
    }

    fn read(&self, id: &str) -> (Value, i32) {
        self.lookup(&["read", id, "--session", &self.session])
    }

    fn scan_command(&self, command: &str) -> String {
        let project = self.project.to_string_lossy().into_owned();
        let out = self
            .cmd(&["scan", "command", "--command", command, "--session", &self.session, "--project", project.as_str()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn events(&self, event: &str) -> Vec<Value> {
        std::fs::read_to_string(self.state().join("agent-ways/events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v["event"] == event && v["session"] == self.session.as_str())
            .collect()
    }

    fn marker(&self, way: &str) -> PathBuf {
        PathBuf::from(format!("{}/{}/ways/{way}/.marker.main", sessions_root(), self.session))
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(format!("{}/{}", sessions_root(), self.session));
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

const GUIDE: &str = "description: guide way\ncommands: ^git\\ commit\nscope: agent\nrefire: 0.15\n";

fn guide_env(name: &str) -> Env {
    let env = Env::new(name);
    env.way("lookupdomain/guide", GUIDE, "# Marker guide\n\nBody text.\n");
    env
}

impl Env {
    /// `ways hook pull`, as the PostToolUse hook on `ways_read` runs it.
    fn hook_pull(&self, id: &str, agent: Option<&str>, transcript: Option<&std::path::Path>) -> std::process::Output {
        self.hook_pull_with(id, agent, transcript, None)
    }

    /// [`Env::hook_pull`] with the `tool_response` the payload carries.
    fn hook_pull_with(&self, id: &str, agent: Option<&str>, transcript: Option<&std::path::Path>, response: Option<Value>) -> std::process::Output {
        use std::io::Write;
        let mut payload = serde_json::json!({
            "session_id": self.session,
            "hook_event_name": "PostToolUse",
            "tool_name": "mcp__agent-ways__ways_read",
            "tool_input": { "id": id },
            "cwd": self.project,
        });
        if let Some(r) = response {
            payload["tool_response"] = r;
        }
        if let Some(a) = agent {
            payload["agent_id"] = a.into();
        }
        if let Some(t) = transcript {
            payload["transcript_path"] = t.to_string_lossy().as_ref().into();
        }
        let mut child = self.cmd(&["hook", "pull"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    fn marker_of(&self, way: &str, agent: &str) -> PathBuf {
        PathBuf::from(format!("{}/{}/ways/{way}/.marker.{agent}", sessions_root(), self.session))
    }
}

#[test]
fn a_read_serves_the_body_and_stamps_nothing() {
    let env = guide_env("shape");
    let (v, code) = env.read("lookupdomain/guide");
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["contract"], 1);
    assert_eq!(v["way"], "lookupdomain/guide");
    assert_eq!(v["route"], "lookupdomain > guide");
    assert!(v["body"].as_str().unwrap().contains("# Marker guide"), "{v}");
    assert_eq!(v["scope"], "agent");
    assert!(v.get("note").is_none());
    assert!(!env.marker("lookupdomain/guide").exists(), "the server cannot tell which agent asked, so it stamps no one");
    assert!(env.events("way_pulled").is_empty());
}

#[test]
fn the_hook_stamps_the_pull_so_a_following_scan_does_not_refire_the_way() {
    let env = guide_env("stamp");
    assert!(env.hook_pull("lookupdomain/guide", None, None).status.success());
    assert!(env.marker("lookupdomain/guide").exists(), "the pull wrote the disclosure marker injection writes");
    let out = env.scan_command("git commit -m x");
    assert!(!out.contains("# Marker guide"), "the way was re-injected right after a pull: {out}");
    assert!(env.events("way_fired").is_empty(), "no injection fire follows a pull");
    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 1);
    assert_eq!((pulls[0]["way"].as_str(), pulls[0]["window"].as_str()), (Some("lookupdomain/guide"), Some("first_fire")));
    assert_eq!((&pulls[0]["out_of_band"], &pulls[0]["stamped"]), (&Value::Bool(false), &Value::Bool(true)));
}

#[test]
fn a_subagents_pull_is_stamped_for_the_subagent_and_not_for_main() {
    let env = guide_env("subagent");
    assert!(env.hook_pull("lookupdomain/guide", Some("sub1"), None).status.success());
    assert!(env.marker_of("lookupdomain/guide", "sub1").exists(), "the stamp lands on the calling agent");
    assert!(!env.marker("lookupdomain/guide").exists(), "main's marker stays absent");
    // Main has not seen the way, so injection still delivers it there.
    assert!(env.scan_command("git commit -m x").contains("# Marker guide"));
}

#[test]
fn a_pull_inside_the_refire_window_is_out_of_band_and_restarts_the_window() {
    let env = guide_env("ticks");
    let transcript = env.base.join(format!("{}.jsonl", env.session));
    let at = |tokens: u64| {
        std::fs::write(
            &transcript,
            format!(
                r#"{{"type":"assistant","message":{{"model":"claude-opus-5-5","usage":{{"input_tokens":{tokens},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
            ) + "\n",
        )
        .unwrap();
    };
    let scan = |env: &Env| {
        let project = env.project.to_string_lossy().into_owned();
        let out = env
            .cmd(&["scan", "command", "--command", "git commit -m x", "--session", &env.session, "--project", &project, "--transcript", &transcript.to_string_lossy()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // The window is 0.15 of 100k tokens: a way re-discloses once more than 15k tokens have passed.
    at(20_000);
    assert!(scan(&env).contains("# Marker guide"), "T1: injection fires the way");
    at(30_000);
    assert!(env.hook_pull("lookupdomain/guide", None, Some(&transcript)).status.success());
    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 1);
    assert_eq!((pulls[0]["window"].as_str(), &pulls[0]["out_of_band"]), (Some("suppressed"), &Value::Bool(true)));
    assert!(pulls[0]["epoch_distance"].is_number(), "{}", pulls[0]);
    assert_eq!(pulls[0]["token_position"], "30000", "the pull is stamped at the tick it happened at");
    let stamp_tick = std::fs::read_to_string(env.marker("lookupdomain/guide")).unwrap();
    assert!(stamp_tick.starts_with("30000\t"), "the marker carries the tick: {stamp_tick:?}");
    at(40_000);
    assert!(!scan(&env).contains("# Marker guide"), "T2+10k is inside the window the pull restarted (T1+20k would have refired)");
    at(50_000);
    assert!(scan(&env).contains("# Marker guide"), "T2+20k is past the window: the way refires");
    assert_eq!(env.events("way_redisclosed").len(), 1);
}

#[test]
fn a_disabled_way_is_refused_with_or_without_a_session_and_the_toggle_is_named() {
    let env = guide_env("disabled");
    std::fs::write(env.project.join(".claude/ways.yaml"), "ways:\n  lookupdomain/*: false\n").unwrap();
    for args in [vec!["read", "lookupdomain/guide", "--session", env.session.as_str()], vec!["read", "lookupdomain/guide"]] {
        let (v, code) = env.lookup(&args);
        assert_eq!(code, 1, "{v}");
        let err = v["error"].as_str().unwrap();
        assert!(err.contains("disabled") && err.contains("lookupdomain/*: false") && err.contains("ways.yaml"), "{err}");
        assert!(v.get("body").is_none(), "a disabled way is not served");
    }
    assert!(env.hook_pull("lookupdomain/guide", None, None).status.success());
    assert!(!env.marker("lookupdomain/guide").exists(), "a disabled way is not stamped either");
    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 1, "the refusal is logged");
    assert_eq!((pulls[0]["reason"].as_str(), &pulls[0]["stamped"], pulls[0]["window"].as_str()), (Some("disabled"), &Value::Bool(false), Some("none")));
}

#[test]
fn a_failed_read_is_not_stamped_and_is_logged_as_refused() {
    let env = guide_env("failed");
    for response in [serde_json::json!({ "isError": true, "content": [] }), serde_json::json!({ "error": "timed out" }), serde_json::json!({ "structuredContent": { "error": "x" } })] {
        assert!(env.hook_pull_with("lookupdomain/guide", None, None, Some(response)).status.success());
    }
    assert!(!env.marker("lookupdomain/guide").exists(), "a read that failed disclosed nothing");
    assert!(env.scan_command("git commit -m x").contains("# Marker guide"), "injection is not held back by a failed read");
    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 3);
    for p in &pulls {
        assert_eq!((p["reason"].as_str(), &p["stamped"], p["window"].as_str()), (Some("read failed"), &Value::Bool(false), Some("none")));
    }
    // A successful response stamps.
    env.hook_pull_with("lookupdomain/guide", None, None, Some(serde_json::json!({ "isError": false, "content": [] })));
    assert!(env.marker("lookupdomain/guide").exists());
}

#[test]
fn a_refused_pull_is_logged_with_a_reason_and_the_model_supplied_id_is_cut_short() {
    let env = guide_env("refused");
    let long = format!("../{}", "a".repeat(500));
    env.hook_pull(&long, None, None);
    env.hook_pull("lookupdomain/nothing", None, None);
    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 2);
    assert_eq!(pulls[0]["reason"], "invalid id");
    assert!(pulls[0]["way"].as_str().unwrap().chars().count() <= 64, "{}", pulls[0]["way"]);
    assert_eq!(pulls[1]["reason"], "not found");
    assert!(pulls.iter().all(|p| p["stamped"] == false && p["window"] == "none"));
}

#[test]
fn an_id_that_leaves_the_ways_roots_is_refused_by_every_verb() {
    let env = guide_env("escape");
    let outside = env.base.join("outside/secret");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.md"), "---\ndescription: x\nrefire: once\n---\nTOP SECRET\n").unwrap();
    // A link inside the root that resolves outside it.
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, env.ways_root().join("lookupdomain/link")).unwrap();
    let absolute = outside.to_string_lossy().into_owned();
    for id in ["../../../../outside/secret", "lookupdomain/../../../../outside/secret", absolute.as_str(), "lookupdomain\\guide", "lookupdomain//guide", "./lookupdomain/guide", ""] {
        for verb in ["read", "neighbors"] {
            let (v, code) = env.lookup(&[verb, id]);
            assert_eq!(code, 1, "{verb} {id:?}: {v}");
            assert!(!v.to_string().contains("TOP SECRET"), "{verb} {id:?} leaked the file");
        }
        env.hook_pull(id, None, None);
    }
    // The link exists on Unix only (Windows needs a privilege to create one),
    // so only there is a pull refused as "outside roots".
    #[cfg(unix)]
    {
        let (v, code) = env.read("lookupdomain/link");
        assert_eq!(code, 1, "{v}");
        assert!(!v.to_string().contains("TOP SECRET"));
        env.hook_pull("lookupdomain/link", None, None);
    }
    let escaped = format!("{}/{}/ways", sessions_root(), env.session);
    let stamped: Vec<_> = std::fs::read_dir(&escaped).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(stamped.is_empty(), "nothing was stamped for a refused id: {stamped:?}");
    assert!(env.events("way_pulled").iter().all(|p| p["stamped"] == false), "every pull of these ids was refused");
    #[cfg(unix)]
    assert!(env.events("way_pulled").iter().any(|p| p["reason"] == "outside roots"), "the link is named as such");
    assert!(env.read("lookupdomain/guide").1 == 0, "a plain id still reads");
}

#[test]
fn a_project_with_ways_switched_off_serves_no_search_or_read() {
    let env = guide_env("off");
    std::fs::write(env.project.join(".claude/ways.yaml"), "enabled: false\n").unwrap();
    for args in [vec!["read", "lookupdomain/guide"], vec!["search", "commit"]] {
        let (v, code) = env.lookup(&args);
        assert_eq!(code, 1, "{v}");
        assert!(v["error"].as_str().unwrap().contains("enabled: false"), "{v}");
    }
    env.hook_pull("lookupdomain/guide", None, None);
    assert!(!env.marker("lookupdomain/guide").exists());
}

#[test]
fn a_read_ignores_the_ways_scope_and_says_when_it_does_not_match_the_session() {
    let env = Env::new("scope");
    env.way("lookupdomain/sub", "description: sub only\nscope: subagent\nrefire: 0.15\n", "# Marker sub\n");
    let (v, code) = env.lookup(&["read", "lookupdomain/sub", "--session", &env.session]);
    assert_eq!(code, 0, "scope never refuses a pull: {v}");
    assert!(v["body"].as_str().unwrap().contains("# Marker sub"));
    assert_eq!(v["scope"], "subagent");
    let note = v["note"].as_str().expect("a note names the mismatch");
    assert!(note.contains("subagent") && note.contains("agent"), "{note}");
}

#[test]
fn a_neighbour_out_of_scope_for_the_session_is_marked() {
    let env = Env::new("scopenb");
    env.way("nd/a", "description: a\n", "# A\n\n## See Also\n\n- b(nd) — b\n- c(nd) — c\n");
    env.way("nd/b", "description: b\nscope: subagent\n", "# B\n");
    env.way("nd/c", "description: c\n", "# C\n");
    let (v, code) = env.lookup(&["neighbors", "nd/a", "--session", &env.session]);
    assert_eq!(code, 0, "{v}");
    let all = v["neighbors"].as_array().unwrap();
    let find = |w: &str| all.iter().find(|n| n["way"] == w).unwrap();
    assert_eq!(find("nd/b")["in_scope"], false);
    assert!(find("nd/c").get("in_scope").is_none());
}

fn embedding(env: &Env, rows: &[(&str, [f64; 3])]) {
    let dir = env.cache().join("agent-ways/user");
    std::fs::create_dir_all(&dir).unwrap();
    let lines: Vec<String> = rows.iter().map(|(id, v)| serde_json::json!({ "id": id, "embedding": v }).to_string()).collect();
    std::fs::write(dir.join("ways-corpus.jsonl"), lines.join("\n") + "\n").unwrap();
}

#[test]
fn neighbours_label_the_tree_a_see_also_edge_and_a_semantic_neighbour() {
    let env = Env::new("neighbours");
    env.way("nd/code", "description: code\n", "# Code\n\n## See Also\n\n- tests(nd) — how to test\n");
    env.way("nd/code/quality", "description: quality\n", "# Quality\n");
    env.way("nd/tests", "description: tests\n", "# Tests\n");
    env.way("nd/lint", "description: lint\n", "# Lint\n");
    embedding(
        &env,
        &[("nd/code", [1.0, 0.0, 0.0]), ("nd/lint", [0.8, 0.6, 0.0]), ("nd/tests", [0.0, 0.0, 1.0]), ("nd/code/quality", [0.0, 1.0, 0.0])],
    );

    let (v, code) = env.lookup(&["neighbors", "nd/code"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["contract"], 1);
    let all = v["neighbors"].as_array().unwrap();
    let of = |kind: &str| -> Vec<&str> { all.iter().filter(|n| n["kind"] == kind).map(|n| n["way"].as_str().unwrap()).collect() };
    assert_eq!(of("child"), ["nd/code/quality"]);
    assert_eq!(of("see_also"), ["nd/tests"]);
    assert_eq!(of("semantic"), ["nd/lint"], "nd/lint has no See Also edge to nd/code and sits at cosine 0.8");
    let sem = all.iter().find(|n| n["kind"] == "semantic").unwrap();
    assert!((sem["cosine"].as_f64().unwrap() - 0.8).abs() < 1e-3);
    assert!(v.get("note").is_none());
}

#[test]
fn a_search_without_an_embedding_engine_is_an_error_not_an_empty_list() {
    let env = guide_env("search");
    let (v, code) = env.lookup(&["search", "commit the change", "--session", &env.session]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["contract"], 1, "an error carries the contract too");
    assert!(v["error"].as_str().unwrap().contains("embedding"), "{v}");
}
