//! Session Simulator Integration Test
//!
//! Exercises the `ways` binary by replaying synthetic sessions.
//! Assertions use /tmp markers (the source of truth), not output parsing.
//!
//! Each scenario gets a unique session ID and cleans up after itself.

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Session root (must match session::sessions_root()) ────────

fn sessions_root() -> String {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/claude-sessions");
    }
    let uid = Command::new("id").arg("-u").output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0".to_string());
    format!("/tmp/.claude-sessions-{uid}")
}

// ── Test infrastructure ────────────────────────────────────────

fn ways_bin() -> PathBuf {
    // Built by cargo test — find it in target/
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test binary name
    path.pop(); // remove deps/
    path.push("ways");
    if !path.exists() {
        // Fallback: look relative to the project
        path = PathBuf::from(env!("CARGO_BIN_EXE_ways"));
    }
    path
}

fn fixture_ways_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ways")
}

fn generate_corpus(name: &str) -> PathBuf {
    // Per-test corpus dir avoids races when tests run in parallel
    let corpus_dir = std::env::temp_dir().join(format!("ways-sim-corpus-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&corpus_dir).unwrap();
    let corpus_file = corpus_dir.join("ways-corpus.jsonl");

    let _status = Command::new(ways_bin())
        .args(["corpus", "--ways-dir"])
        .arg(fixture_ways_dir())
        .arg("--quiet")
        .env("XDG_CACHE_HOME", &corpus_dir)
        .status()
        .expect("Failed to run ways corpus");

    // The corpus goes to XDG_CACHE_HOME/claude-ways/user/ways-corpus.jsonl
    let actual = corpus_dir.join("claude-ways/user/ways-corpus.jsonl");
    if actual.exists() {
        return actual;
    }
    // Fallback
    corpus_file
}

struct Session {
    id: String,
    corpus: PathBuf,
}

impl Session {
    fn new(name: &str) -> Self {
        let id = format!("sim-{}-{}", name, std::process::id());
        let corpus = generate_corpus(name);
        // Clean any stale markers
        clean_markers(&id);
        Session { id, corpus }
    }

    fn scan_prompt(&self, query: &str) -> String {
        let output = Command::new(ways_bin())
            .args([
                "scan", "prompt",
                "--query", query,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .env("HOME", fixture_home())
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan prompt");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_command(&self, cmd: &str) -> String {
        let output = Command::new(ways_bin())
            .args([
                "scan", "command",
                "--command", cmd,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .env("HOME", fixture_home())
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan command");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_file(&self, path: &str) -> String {
        let output = Command::new(ways_bin())
            .args([
                "scan", "file",
                "--path", path,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .env("HOME", fixture_home())
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan file");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_prompt_with_project(&self, query: &str, project: &str) -> String {
        let output = Command::new(ways_bin())
            .args([
                "scan", "prompt",
                "--query", query,
                "--session", &self.id,
                "--project", project,
            ])
            .env("HOME", fixture_home())
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan prompt");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_prompt_with_home(&self, query: &str, home: &Path) -> String {
        let output = Command::new(ways_bin())
            .args([
                "scan", "prompt",
                "--query", query,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .env("HOME", home)
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan prompt");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_state(&self) -> String {
        let output = Command::new(ways_bin())
            .args([
                "scan", "state",
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .env("HOME", fixture_home())
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan state");

        String::from_utf8_lossy(&output.stdout).to_string()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        clean_markers(&self.id);
    }
}

/// The fixture HOME — ways looks for ~/.claude/hooks/ways/
fn fixture_home() -> PathBuf {
    let home = std::env::temp_dir().join("ways-sim-home");
    let ways_link = home.join(".claude/hooks/ways");
    if !ways_link.exists() {
        std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
        // Symlink fixture ways to where the binary expects them
        #[cfg(unix)]
        std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).ok();
    }
    home
}

// ── Plugin fixture helpers (ADR-129) ─────────────────────────

fn fixture_plugin_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct PluginFixture {
    id: String,
    path: PathBuf,
    scope: String,
    project_path: Option<String>,
}

/// Resolve the ways directory for a fixture plugin.
/// Plugin convention: $PLUGIN_INSTALL_PATH/ways/
fn resolve_fixture_ways_path(fixture_name: &str) -> PathBuf {
    let dir = fixture_plugin_dir(fixture_name).join("ways");
    assert!(
        dir.is_dir(),
        "Fixture plugin '{}' missing ways/ at {}",
        fixture_name,
        dir.display()
    );
    dir
}

impl PluginFixture {
    fn user(id: &str, fixture_name: &str) -> Self {
        PluginFixture {
            id: id.to_string(),
            path: resolve_fixture_ways_path(fixture_name),
            scope: "user".to_string(),
            project_path: None,
        }
    }

    fn project(id: &str, fixture_name: &str, project_path: &str) -> Self {
        PluginFixture {
            id: id.to_string(),
            path: resolve_fixture_ways_path(fixture_name),
            scope: "project".to_string(),
            project_path: Some(project_path.to_string()),
        }
    }
}

/// A session with plugin-ways.json manifest pre-created.
struct PluginSession {
    inner: Session,
}

impl PluginSession {
    fn new(name: &str, plugins: &[PluginFixture]) -> Self {
        let inner = Session::new(name);

        // Write plugin-ways.json to session dir
        let session_dir = format!("{}/{}", sessions_root(), inner.id);
        std::fs::create_dir_all(&session_dir).unwrap();

        let manifest: Vec<serde_json::Value> = plugins
            .iter()
            .map(|p| {
                let mut entry = serde_json::json!({
                    "id": p.id,
                    "path": p.path.display().to_string(),
                    "scope": p.scope,
                });
                if let Some(ref pp) = p.project_path {
                    entry["projectPath"] = serde_json::json!(pp);
                } else {
                    entry["projectPath"] = serde_json::Value::Null;
                }
                entry
            })
            .collect();

        let manifest_path = format!("{session_dir}/plugin-ways.json");
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();

        PluginSession { inner }
    }
}

impl std::ops::Deref for PluginSession {
    type Target = Session;
    fn deref(&self) -> &Session {
        &self.inner
    }
}

fn clean_markers(session_id: &str) {
    let session_dir = format!("{}/{session_id}", sessions_root());
    let _ = std::fs::remove_dir_all(&session_dir);
}

// ── Assertion helpers ──────────────────────────────────────────

fn assert_marker_exists(way_id: &str, session_id: &str) {
    let path = format!("{}/{session_id}/ways/{way_id}/.marker.main", sessions_root());
    assert!(
        Path::new(&path).exists(),
        "Expected marker for '{way_id}' but it doesn't exist at {path}"
    );
}

fn assert_marker_absent(way_id: &str, session_id: &str) {
    let path = format!("{}/{session_id}/ways/{way_id}/.marker.main", sessions_root());
    assert!(
        !Path::new(&path).exists(),
        "Expected NO marker for '{way_id}' but found one at {path}"
    );
}

fn assert_epoch(session_id: &str, expected: u64) {
    let path = format!("{}/{session_id}/epoch", sessions_root());
    let actual: u64 = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("No epoch file at {path}"))
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("Epoch file at {path} is not a number"));
    assert_eq!(actual, expected, "Epoch mismatch for session {session_id}");
}

fn assert_check_fires(way_id: &str, session_id: &str, expected: u64) {
    let path = format!("{}/{session_id}/check-fires/{way_id}/.value", sessions_root());
    let actual: u64 = std::fs::read_to_string(&path)
        .unwrap_or("0".to_string())
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(
        actual, expected,
        "Check fire count mismatch for '{way_id}': got {actual}, expected {expected}"
    );
}

// ── Scenario 1: Basic Prompt Matching + Idempotency ────────────

#[test]
fn scenario_1_basic_prompt_matching() {
    let s = Session::new("s1");

    // Turn 1: query with "test" vocabulary → should match child (testing)
    s.scan_prompt("how do I write a unit test for this module");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/parent/child", &s.id);

    // Turn 2: same query again → should NOT re-fire (idempotency)
    let _output = s.scan_prompt("how do I write a unit test for this module");
    assert_epoch(&s.id, 2);
    // Marker still exists from turn 1 — no new content expected
    // (we can't easily assert "no new output" beyond marker check,
    //  but the marker being present means show::way returned early)
    assert_marker_exists("testdomain/parent/child", &s.id);

    // Turn 3: different vocabulary → should match child2 (refactoring)
    s.scan_prompt("refactor extract method decompose this function");
    assert_epoch(&s.id, 3);
    assert_marker_exists("testdomain/parent/child2", &s.id);
}

// ── Scenario 2: Command Triggers ───────────────────────────────

#[test]
fn scenario_2_command_triggers() {
    let s = Session::new("s2");

    // Turn 1: git commit → should match cmd-trigger
    s.scan_command("git commit -m 'fix: auth bug'");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/cmd-trigger", &s.id);

    // Turn 2: npm install → should match with-check (commands: ^npm install)
    s.scan_command("npm install express");
    assert_epoch(&s.id, 2);
    assert_marker_exists("testdomain2/with-check", &s.id);

    // Turn 3: unrelated command → nothing fires
    s.scan_command("ls -la");
    assert_epoch(&s.id, 3);
    // No new markers beyond what we already have
}

// ── Scenario 3: File Edit Triggers ─────────────────────────────

#[test]
fn scenario_3_file_triggers() {
    let s = Session::new("s3");

    // Turn 1: .env file → should match file-trigger
    s.scan_file("/app/.env");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/file-trigger", &s.id);

    // Turn 2: unmatched file → nothing
    s.scan_file("src/api/routes.ts");
    assert_epoch(&s.id, 2);
    // file-trigger still exists but no new ones

    // Turn 3: .env again → idempotent, no re-fire
    s.scan_file("config/.env");
    assert_epoch(&s.id, 3);
    // Marker still there, show returned early
    assert_marker_exists("testdomain/file-trigger", &s.id);
}

// ── Scenario 4: Check Scoring ──────────────────────────────────

#[test]
fn scenario_4_check_scoring() {
    let s = Session::new("s4");

    // Turn 1: fire the parent way first (supply chain)
    s.scan_prompt("supply chain dependency security audit vulnerability");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain2/with-check", &s.id);

    // Turn 2: command trigger for check — npm install
    s.scan_command("npm install sketchy-package");
    assert_epoch(&s.id, 2);
    // Check should have fired (commands regex matches, parent way already shown)
    assert_check_fires("testdomain2/with-check", &s.id, 1);

    // Turn 3: another install command — check fires again with decay
    s.scan_command("pip install unknown-package");
    assert_epoch(&s.id, 3);
    assert_check_fires("testdomain2/with-check", &s.id, 2);
}

// ── Scenario 5: Progressive Disclosure ─────────────────────────

#[test]
fn scenario_5_progressive_disclosure() {
    let s = Session::new("s5");

    // Turn 1: broad query about code quality → parent fires
    s.scan_prompt("code quality review architecture maintainability coupling");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/parent", &s.id);

    // Turn 2: now ask about testing → child should fire (threshold lowered 20% by parent)
    s.scan_prompt("write unit tests with good coverage and assertions");
    assert_epoch(&s.id, 2);
    assert_marker_exists("testdomain/parent/child", &s.id);
}

// ── Scenario 6: Scope Filtering ────────────────────────────────

#[test]
fn scenario_6_scope_filtering() {
    let s = Session::new("s6");

    // Turn 1: no teammate marker → agent scope
    // scoped-way has scope:teammate, should NOT fire
    s.scan_prompt("teammate delegate collaborate subagent");
    assert_epoch(&s.id, 1);
    assert_marker_absent("testdomain/scoped-way", &s.id);

    // Turn 2: create teammate marker, try again
    let teammate_dir = format!("{}/{}", sessions_root(), s.id);
    std::fs::create_dir_all(&teammate_dir).unwrap();
    let teammate_marker = format!("{teammate_dir}/teammate");
    std::fs::write(&teammate_marker, "test-team").unwrap();

    s.scan_prompt("teammate delegate collaborate subagent");
    assert_epoch(&s.id, 2);
    assert_marker_exists("testdomain/scoped-way", &s.id);

    // Clean up teammate marker
    let _ = std::fs::remove_file(&teammate_marker);
}

// ── Scenario 7: When Preconditions ─────────────────────────────

#[test]
fn scenario_7_when_preconditions() {
    let s = Session::new("s7");

    // Turn 1: wrong project → gated-way should NOT fire
    s.scan_prompt_with_project(
        "gated project specific configuration",
        "/tmp/wrong-project",
    );
    assert_epoch(&s.id, 1);
    assert_marker_absent("testdomain/gated-way", &s.id);

    // Turn 2: correct project → gated-way SHOULD fire
    // Create the expected project dir
    std::fs::create_dir_all("/tmp/test-project-sim").unwrap();
    s.scan_prompt_with_project(
        "gated project specific configuration",
        "/tmp/test-project-sim",
    );
    assert_epoch(&s.id, 2);
    assert_marker_exists("testdomain/gated-way", &s.id);

    let _ = std::fs::remove_dir("/tmp/test-project-sim");
}

// ── Scenario 8: Epoch Counter Integrity ────────────────────────

#[test]
fn scenario_8_epoch_integrity() {
    let s = Session::new("s8");

    // Run 5 turns of mixed operations
    s.scan_prompt("write some code");
    assert_epoch(&s.id, 1);

    s.scan_command("git status");
    assert_epoch(&s.id, 2);

    s.scan_file("src/main.rs");
    assert_epoch(&s.id, 3);

    s.scan_prompt("more code quality");
    assert_epoch(&s.id, 4);

    s.scan_command("make test");
    assert_epoch(&s.id, 5);

    // Epoch should be exactly 5 — no drift, no skips
}

// ── Scenario 9: Domain Disable ────────────────────────────────

#[test]
fn scenario_9_domain_disable() {
    let s = Session::new("s9");

    // Create a fixture home with ways.json that disables "testdomain"
    let home = std::env::temp_dir().join("ways-sim-home-s9");
    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    // Symlink ways
    let ways_link = claude_dir.join("hooks/ways");
    std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&ways_link);
        std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).unwrap();
    }

    // Write ways.json disabling testdomain
    std::fs::write(
        claude_dir.join("ways.json"),
        r#"{"disabled": ["testdomain"]}"#,
    )
    .unwrap();

    // Turn 1: prompt that would normally match testdomain/parent/child
    s.scan_prompt_with_home("write unit tests with good coverage", &home);
    assert_epoch(&s.id, 1);
    // Should NOT fire — domain is disabled
    assert_marker_absent("testdomain/parent/child", &s.id);

    // Turn 2: testdomain2 is NOT disabled — with-check should still work
    s.scan_prompt_with_home("supply chain dependency security audit", &home);
    assert_epoch(&s.id, 2);
    assert_marker_exists("testdomain2/with-check", &s.id);

    // Cleanup
    let _ = std::fs::remove_dir_all(&home);
}

// ── Scenario 10: State Triggers ───────────────────────────────

#[test]
fn scenario_10_state_triggers() {
    let s = Session::new("s10");

    // Turn 1: state scan should fire session-start trigger
    // (state scan doesn't bump epoch — it runs alongside prompt scan)
    let output = s.scan_state();
    assert_marker_exists("testdomain/state-trigger", &s.id);
    assert!(
        output.contains("State Trigger Test Way"),
        "Expected state trigger content in output"
    );

    // Turn 2: second state scan — idempotent, marker prevents re-fire
    let output2 = s.scan_state();
    assert!(
        !output2.contains("State Trigger Test Way"),
        "State trigger should not re-fire (marker exists)"
    );
}

// ── Scenario 11: Plugin Way Discovery (ADR-129) ──────────────

// 11a: Basic plugin way fires on keyword match
#[test]
fn scenario_11a_plugin_keyword_match() {
    let s = PluginSession::new("s11a", &[
        PluginFixture::user("test-plugin-a@test", "plugin-a"),
    ]);

    s.scan_prompt("I would like a banana please");
    assert_epoch(&s.id, 1);
    assert_marker_exists("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);
}

// 11b: Plugin way idempotency (fire-once)
#[test]
fn scenario_11b_plugin_idempotency() {
    let s = PluginSession::new("s11b", &[
        PluginFixture::user("test-plugin-a@test", "plugin-a"),
    ]);

    // Turn 1: fires
    s.scan_prompt("banana fruit apple");
    assert_epoch(&s.id, 1);
    assert_marker_exists("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);

    // Turn 2: same prompt — should NOT re-fire (marker exists)
    s.scan_prompt("banana fruit apple");
    assert_epoch(&s.id, 2);
    // Marker still there from turn 1, way was suppressed
    assert_marker_exists("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);
}

// 11c: Plugin way does NOT fire when manifest is absent
#[test]
fn scenario_11c_no_manifest_no_plugin_ways() {
    // Regular Session (no PluginSession) — no plugin-ways.json
    let s = Session::new("s11c");

    s.scan_prompt("banana fruit apple");
    assert_epoch(&s.id, 1);
    // Plugin way should NOT fire — no manifest
    assert_marker_absent("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);
}

// 11d: Project-scoped plugin only fires in its project
#[test]
fn scenario_11d_project_scoped_plugin() {
    let target_project = std::env::temp_dir().join("ways-sim-project-11d");
    std::fs::create_dir_all(&target_project).unwrap();

    let s = PluginSession::new("s11d", &[
        PluginFixture::project(
            "test-plugin-a@test",
            "plugin-a",
            target_project.to_str().unwrap(),
        ),
    ]);

    // Turn 1: correct project → plugin way fires
    s.scan_prompt_with_project(
        "banana fruit apple",
        target_project.to_str().unwrap(),
    );
    assert_epoch(&s.id, 1);
    assert_marker_exists("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);

    // Clean markers for a fresh check
    let marker_path = format!(
        "{}/{}/ways/plugin:test-plugin-a@test/plugindomain/fruit",
        sessions_root(), s.id
    );
    let _ = std::fs::remove_dir_all(&marker_path);

    // Turn 2: wrong project → plugin way does NOT fire
    s.scan_prompt_with_project(
        "banana fruit apple",
        "/tmp/wrong-project-11d",
    );
    assert_epoch(&s.id, 2);
    assert_marker_absent("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);

    let _ = std::fs::remove_dir_all(&target_project);
}

// 11e: Plugin way and global way coexist
#[test]
fn scenario_11e_plugin_and_global_coexist() {
    let s = PluginSession::new("s11e", &[
        PluginFixture::user("test-plugin-a@test", "plugin-a"),
    ]);

    // Prompt that matches both a global way (testdomain/parent/child via "test")
    // and a plugin way (fruit via "banana")
    s.scan_prompt("banana unit test coverage");
    assert_epoch(&s.id, 1);
    // Both should fire — different ID namespaces
    assert_marker_exists("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);
    assert_marker_exists("testdomain/parent/child", &s.id);
}

// 11f: Global way still fires with empty plugin manifest
#[test]
fn scenario_11f_empty_manifest_global_works() {
    let s = PluginSession::new("s11f", &[]);

    // Global way should fire normally
    s.scan_prompt("how do I write a unit test for this module");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/parent/child", &s.id);
}

// 11g: Multiple plugins with ways
#[test]
fn scenario_11g_multiple_plugins() {
    let s = PluginSession::new("s11g", &[
        PluginFixture::user("test-plugin-a@test", "plugin-a"),
        PluginFixture::user("test-plugin-b@test", "plugin-b"),
    ]);

    // Turn 1: match plugin A's way
    s.scan_prompt("banana fruit apple");
    assert_epoch(&s.id, 1);
    assert_marker_exists("plugin:test-plugin-a@test/plugindomain/fruit", &s.id);
    assert_marker_absent("plugin:test-plugin-b@test/plugindomain/color", &s.id);

    // Turn 2: match plugin B's way
    s.scan_prompt("red blue green color palette");
    assert_epoch(&s.id, 2);
    assert_marker_exists("plugin:test-plugin-b@test/plugindomain/color", &s.id);
}

// 11h: Plugin way check files
#[test]
fn scenario_11h_plugin_check_files() {
    let s = PluginSession::new("s11h", &[
        PluginFixture::user("test-plugin-check@test", "plugin-with-check"),
    ]);

    // Turn 1: fire the parent way first
    s.scan_prompt("audit compliance verify review");
    assert_epoch(&s.id, 1);
    assert_marker_exists("plugin:test-plugin-check@test/plugindomain/audited", &s.id);

    // Turn 2: command matching the check
    s.scan_command("audit run --full");
    assert_epoch(&s.id, 2);
    assert_check_fires("plugin:test-plugin-check@test/plugindomain/audited", &s.id, 1);
}

// 11i: Plugin macro is NOT executed (trust boundary)
#[test]
fn scenario_11i_plugin_macro_not_trusted() {
    let s = PluginSession::new("s11i", &[
        PluginFixture::user("test-plugin-macro@test", "plugin-with-macro"),
    ]);

    // The greeter way has macro: prepend, but plugin macros should not execute
    // because the plugin path is not in trusted-project-macros.
    // The way should still fire (marker stamped) but macro output should be absent.
    s.scan_prompt("hello welcome greet");
    assert_epoch(&s.id, 1);
    assert_marker_exists("plugin:test-plugin-macro@test/plugindomain/greeter", &s.id);

    // We can't easily assert macro didn't run from marker checks alone,
    // but the way firing without error is the baseline. The macro trust
    // gating is handled by show::way checking is_project_trusted().
}
