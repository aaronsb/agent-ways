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
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0".to_string());
    format!("/tmp/.claude-sessions-{uid}")
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
            .env("XDG_BIN_HOME", self.home.join(".local/bin"));
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

    fn scan_prompt(&self, query: &str) {
        let project = self.project.to_string_lossy().into_owned();
        self.cmd(&["scan", "prompt", "--query", query, "--session", &self.session, "--project", project.as_str()]).output().unwrap();
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

#[test]
fn a_read_returns_the_body_and_logs_a_pull() {
    let env = guide_env("shape");
    let (v, code) = env.read("lookupdomain/guide");
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["way"], "lookupdomain/guide");
    assert_eq!(v["route"], "lookupdomain > guide");
    assert!(v["body"].as_str().unwrap().contains("# Marker guide"), "{v}");
    assert_eq!(v["out_of_band"], false);
    assert_eq!(v["stamped"], true);
    assert!(v.get("epoch_distance").is_none());

    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 1);
    assert_eq!(pulls[0]["way"], "lookupdomain/guide");
    assert_eq!(pulls[0]["out_of_band"], false);
    assert_eq!(pulls[0]["window"], "first_fire");
}

#[test]
fn a_pull_stamps_disclosure_so_a_following_scan_does_not_refire_the_way() {
    let env = guide_env("stamp");
    env.read("lookupdomain/guide");
    assert!(env.marker("lookupdomain/guide").exists(), "the pull wrote the disclosure marker injection writes");

    let out = env.scan_command("git commit -m x");
    assert!(!out.contains("# Marker guide"), "the way was re-injected right after a pull: {out}");
    assert!(env.events("way_fired").is_empty(), "no injection fire follows a pull");
}

#[test]
fn a_pull_inside_the_refire_window_returns_the_body_and_logs_the_epoch_distance() {
    let env = guide_env("window");
    // Injection delivers the way first, at epoch 0.
    let out = env.scan_command("git commit -m x");
    assert!(out.contains("# Marker guide"), "injection fires the way: {out}");
    // Two prompts and one more tool call pass (each scan bumps the epoch); the way is still inside its window.
    env.scan_prompt("hello there");
    env.scan_prompt("and again");
    // Injection would hold the way back now.
    assert!(!env.scan_command("git commit -m y").contains("# Marker guide"));

    let (v, code) = env.read("lookupdomain/guide");
    assert_eq!(code, 0, "{v}");
    assert!(v["body"].as_str().unwrap().contains("# Marker guide"), "a pull always returns the way: {v}");
    assert_eq!(v["out_of_band"], true);
    assert_eq!(v["epoch_distance"], 3, "disclosed at epoch 1, read at epoch 4");

    let pulls = env.events("way_pulled");
    assert_eq!(pulls.len(), 1);
    assert_eq!(pulls[0]["out_of_band"], true);
    assert_eq!(pulls[0]["epoch_distance"], 3);
    assert_eq!(pulls[0]["window"], "suppressed");
}

#[test]
fn a_disabled_way_is_refused_and_the_toggle_is_named() {
    let env = guide_env("disabled");
    std::fs::write(env.project.join(".claude/ways.yaml"), "ways:\n  lookupdomain/*: false\n").unwrap();
    let (v, code) = env.read("lookupdomain/guide");
    assert_eq!(code, 1, "{v}");
    let err = v["error"].as_str().unwrap();
    assert!(err.contains("disabled") && err.contains("lookupdomain/*: false") && err.contains("ways.yaml"), "{err}");
    assert!(v.get("body").is_none(), "a disabled way is not served");
    assert!(env.events("way_pulled").is_empty());
    assert!(!env.marker("lookupdomain/guide").exists());
}

#[test]
fn an_unknown_way_is_an_error() {
    let env = guide_env("unknown");
    let (v, code) = env.read("lookupdomain/nothing");
    assert_eq!(code, 1);
    assert!(v["error"].as_str().unwrap().contains("no way named"), "{v}");
}

#[test]
fn a_read_without_a_session_serves_the_body_unstamped() {
    let env = guide_env("nosession");
    let (v, code) = env.lookup(&["read", "lookupdomain/guide"]);
    assert_eq!(code, 0, "{v}");
    assert!(v["body"].as_str().unwrap().contains("# Marker guide"));
    assert_eq!(v["stamped"], false);
    assert!(v["note"].as_str().unwrap().contains("no session"));
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
    assert!(v["error"].as_str().unwrap().contains("embedding"), "{v}");
}
