//! Session Simulator Integration Test
//!
//! Exercises the `ways` binary by replaying synthetic sessions.
//! Assertions read the on-disk session markers (the source of truth), not
//! output parsing.
//!
//! Each scenario gets a unique session ID and cleans up after itself.

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Session root (MUST match session::sessions_root()) ────────
// Mirror of the production resolver so assertions look where the binary writes.
// If they drift, every scenario fails — which is exactly what caught the
// missing Windows branch when sessions_root() was hardened.

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
        let uid = Command::new("id").arg("-u").output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "0".to_string());
        format!("/tmp/.claude-sessions-{uid}")
    }
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

    // The corpus goes to XDG_CACHE_HOME/agent-ways/user/ways-corpus.jsonl
    let actual = corpus_dir.join("agent-ways/user/ways-corpus.jsonl");
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
            // home_dir() prefers USERPROFILE on Windows, so set both or the
            // fixture-home redirection is ignored and the binary reads the real
            // ~/.claude. See util::home_dir().
            .env("USERPROFILE", fixture_home())
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
            // home_dir() prefers USERPROFILE on Windows, so set both or the
            // fixture-home redirection is ignored and the binary reads the real
            // ~/.claude. See util::home_dir().
            .env("USERPROFILE", fixture_home())
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
            // home_dir() prefers USERPROFILE on Windows, so set both or the
            // fixture-home redirection is ignored and the binary reads the real
            // ~/.claude. See util::home_dir().
            .env("USERPROFILE", fixture_home())
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
            // home_dir() prefers USERPROFILE on Windows, so set both or the
            // fixture-home redirection is ignored and the binary reads the real
            // ~/.claude. See util::home_dir().
            .env("USERPROFILE", fixture_home())
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
            .env("USERPROFILE", home) // see scan_prompt: home_dir() prefers USERPROFILE on Windows
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan prompt");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_state(&self) -> String {
        self.scan_state_full(None).0
    }

    /// Run `ways scan state`, optionally passing `--hook-event`, and return
    /// `(stdout, stderr)`. Lets tests assert both content delivery and the
    /// misroute warning channel.
    fn scan_state_full(&self, hook_event: Option<&str>) -> (String, String) {
        let mut args: Vec<&str> = vec![
            "scan", "state",
            "--session", &self.id,
            "--project", "/tmp/nonexistent-project",
        ];
        if let Some(ev) = hook_event {
            args.push("--hook-event");
            args.push(ev);
        }
        let output = Command::new(ways_bin())
            .args(&args)
            .env("HOME", fixture_home())
            // home_dir() prefers USERPROFILE on Windows, so set both or the
            // fixture-home redirection is ignored and the binary reads the real
            // ~/.claude. See util::home_dir().
            .env("USERPROFILE", fixture_home())
            .env("XDG_CACHE_HOME", self.corpus.parent().unwrap().parent().unwrap().parent().unwrap())
            .output()
            .expect("Failed to run ways scan state");

        (
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
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
        // Put the fixture ways where the binary expects them
        // ($HOME/.claude/hooks/ways).
        #[cfg(unix)]
        std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).ok();
        // Windows symlinks need admin / Developer Mode (the Makefile copies for
        // the same reason), so copy the tree in. Stage in a pid-unique dir and
        // atomically rename, so parallel tests never observe a half-copy.
        #[cfg(windows)]
        {
            let staging =
                home.join(format!(".claude/hooks/ways.staging-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&staging);
            if copy_dir_all(&fixture_ways_dir(), &staging).is_ok()
                && std::fs::rename(&staging, &ways_link).is_err()
            {
                // Another test won the race; discard our copy.
                let _ = std::fs::remove_dir_all(&staging);
            }
        }
    }
    home
}

#[cfg(windows)]
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
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

/// Assert `stdout` is exactly the canonical PreToolUse envelope (#528):
/// `hookSpecificOutput` with `hookEventName: "PreToolUse"` and a non-empty
/// `additionalContext` within Claude Code's 10,000-character cap. No top-level
/// `decision` or `additionalContext`, and no `permissionDecision`, so the hook
/// never changes the tool's permission outcome. Returns the context.
fn assert_pretooluse_envelope(stdout: &str, needle: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("PreToolUse stdout is not one JSON object ({e}): {stdout:?}"));
    let obj = v.as_object().expect("PreToolUse stdout is a JSON object");
    assert_eq!(
        obj.keys().collect::<Vec<_>>(),
        vec!["hookSpecificOutput"],
        "only the canonical envelope at top level; got: {stdout}"
    );
    let hso = &v["hookSpecificOutput"];
    assert_eq!(hso["hookEventName"], "PreToolUse");
    assert!(
        hso.get("permissionDecision").is_none(),
        "a guidance hook must not make a permission decision; got: {stdout}"
    );
    let ctx = hso["additionalContext"].as_str().expect("additionalContext is a string");
    assert!(!ctx.is_empty());
    assert!(ctx.encode_utf16().count() <= 10_000, "context over the cap: {} chars", ctx.len());
    assert!(ctx.contains(needle), "context missing {needle:?}: {ctx:?}");
    ctx.to_string()
}

// ── Scenario 1: Basic Prompt Matching + Idempotency ────────────

#[test]
fn scenario_1_basic_prompt_matching() {
    let s = Session::new("s1");

    // Turn 1: query with "test" vocabulary → should match child (testing)
    let output = s.scan_prompt("how do I write a unit test for this module");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/parent/child", &s.id);
    // Regression guard: scan::prompt must emit the UserPromptSubmit envelope
    // when a way matches. Before this guard, the function silently discarded
    // show::way output (`let _ = ...`), and after the first envelope fix it
    // briefly emitted the wrong shape (`additionalContext` at top-level
    // instead of `hookSpecificOutput`). Both regressions pass marker-only
    // assertions because show::way still stamps state for its side effects.
    assert!(
        output.contains("hookSpecificOutput"),
        "scan_prompt must emit hookSpecificOutput envelope when a way matches; got: {output:?}"
    );

    // Turn 2: same query again → should NOT re-fire (idempotency)
    let output_repeat = s.scan_prompt("how do I write a unit test for this module");
    assert_epoch(&s.id, 2);
    assert_marker_exists("testdomain/parent/child", &s.id);
    // Idempotency: marker stops show::way before body is emitted, so output
    // should be empty on the repeat.
    assert!(
        output_repeat.is_empty(),
        "scan_prompt must be idempotent within a session; got: {output_repeat:?}"
    );

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
    let out = s.scan_command("git commit -m 'fix: auth bug'");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/cmd-trigger", &s.id);
    // #528: PreToolUse context reaches the model only in the canonical
    // envelope; the old top-level `decision`/`additionalContext` went to the
    // debug log.
    assert_pretooluse_envelope(&out, "# Commit Messages");

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
    let out = s.scan_file("/app/.env");
    assert_epoch(&s.id, 1);
    assert_marker_exists("testdomain/file-trigger", &s.id);
    assert_pretooluse_envelope(&out, "# Environment Config");

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

    // Place fixture ways under this scenario's private home.
    let ways_link = claude_dir.join("hooks/ways");
    std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&ways_link);
        std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).unwrap();
    }
    // Windows: copy instead of symlink (needs admin/Developer Mode otherwise).
    #[cfg(windows)]
    {
        let _ = std::fs::remove_dir_all(&ways_link);
        copy_dir_all(&fixture_ways_dir(), &ways_link).unwrap();
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
    // (state scan doesn't bump epoch — it runs alongside prompt scan).
    // Pass --hook-event explicitly — this is what check-state.sh actually
    // sends; the fallback path is scenario 11's job.
    let (output, _) = s.scan_state_full(Some("SessionStart"));
    assert_marker_exists("testdomain/state-trigger", &s.id);
    assert!(
        output.contains("State Trigger Test Way"),
        "Expected state trigger content in output"
    );
    // Envelope guard: SessionStart must emit the canonical
    // `hookSpecificOutput` wrapper. The bare top-level `additionalContext`
    // this test previously locked in was believed to be a harness-accepted
    // legacy shape; session transcripts showed the harness never delivered
    // it — the SessionStart payload (ways catalog + core posture) reached
    // zero sessions. Undocumented shapes are dropped silently, so this
    // guard now points the other way.
    assert!(
        output.contains("hookSpecificOutput"),
        "scan_state on SessionStart must emit the canonical hookSpecificOutput envelope; got: {output:?}"
    );

    // Turn 2: second state scan — idempotent, marker prevents re-fire
    let output2 = s.scan_state();
    assert!(
        !output2.contains("State Trigger Test Way"),
        "State trigger should not re-fire (marker exists)"
    );
}

// ── Scenario 11: Hook-event misroute trace ─────────────────────

#[test]
fn scenario_11_hook_event_misroute_warning() {
    // `ways scan state` invoked without `--hook-event` falls back to
    // SessionStart, which is also the shell's jq fallback in
    // `check-state.sh` — two layers of the same default mean a misrouted
    // hook would silently record the wrong hookEventName (the envelope
    // shape itself is canonical for every event). The fallback itself is
    // preserved (behavior unchanged) but a defensive stderr trace surfaces
    // the misroute in hook-execution logs.
    let s = Session::new("s11");

    // Without --hook-event: stderr trace must surface, stdout must still
    // carry content (the SessionStart fallback still runs).
    let (stdout, stderr) = s.scan_state_full(None);
    assert!(
        stderr.contains("[ways]") && stderr.contains("--hook-event"),
        "scan state without --hook-event must emit a [ways] stderr trace mentioning the missing flag; got stderr: {stderr:?}"
    );
    assert!(
        stdout.contains("State Trigger Test Way"),
        "scan state without --hook-event must still default-fire SessionStart; got stdout: {stdout:?}"
    );
}

// ── Scenario 12: PreToolUse context cap and way_suppressed (#528) ──

/// Run `ways scan command` against an isolated HOME and XDG state dir, so the
/// test owns its ways corpus and reads its own event log.
fn scan_command_isolated(session: &str, cmd: &str, home: &Path, state: &Path) -> String {
    let output = Command::new(ways_bin())
        .args([
            "scan", "command",
            "--command", cmd,
            "--session", session,
            "--project", "/tmp/nonexistent-project",
        ])
        .env("HOME", home)
        .env("USERPROFILE", home) // see scan_prompt
        .env("XDG_STATE_HOME", state)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("CLAUDE_AGENT_ID")
        .output()
        .expect("Failed to run ways scan command");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// `(way, reason)` for every `event` row of `session` in the isolated log.
/// `reason` is empty for events that carry none.
fn events_of(state: &Path, session: &str, event: &str) -> Vec<(String, String)> {
    let log = state.join("agent-ways/events.jsonl");
    std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["event"] == event && v["session"] == session)
        .map(|v| {
            (
                v["way"].as_str().unwrap_or("").to_string(),
                v["reason"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn fired_ways(state: &Path, session: &str, event: &str) -> Vec<String> {
    let mut ways: Vec<String> = events_of(state, session, event).into_iter().map(|(w, _)| w).collect();
    ways.sort();
    ways
}

/// Write a way under `root` that fires on `^git commit`, with `body_chars`
/// of filler after its `# Marker <id>` heading, and an optional macro script.
fn write_commit_way(root: &Path, id: &str, body_chars: usize, macro_sh: Option<&str>) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!("# Marker {id}\n\n{}\n", "lorem ipsum ".repeat(body_chars / 12));
    let macro_field = if macro_sh.is_some() { "macro: append\n" } else { "" };
    std::fs::write(
        dir.join(format!("{id}.md")),
        format!("---\ndescription: test way {id}\ncommands: ^git\\ commit\nscope: agent\nrefire: 0.15\n{macro_field}---\n{body}"),
    )
    .unwrap();
    if let Some(script) = macro_sh {
        std::fs::write(dir.join("macro.sh"), script).unwrap();
    }
}

#[test]
fn scenario_12_pretooluse_cap_withholds_without_firing() {
    let base = std::env::temp_dir().join(format!("ways-sim-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    let ways_root = home.join(".claude/hooks/ways/capdomain");

    // Three ways on the same command, about 4,000 characters each: any two
    // fit under the 10,000-character cap, all three do not.
    let ids = ["cap-a", "cap-b", "cap-c"];
    for id in ids {
        write_commit_way(&ways_root, id, 3960, None);
    }

    let session = format!("sim-s12-{}", std::process::id());
    clean_markers(&session);
    let full = |id: &str| format!("capdomain/{id}");

    // Turn 1: two ways fit, the third is withheld for the cap.
    let out = scan_command_isolated(&session, "git commit -m x", &home, &state);
    let ctx = assert_pretooluse_envelope(&out, "# Marker cap-");
    let shown: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| ctx.contains(&format!("# Marker {id}\n")))
        .collect();
    assert_eq!(shown.len(), 2, "two whole ways fit under the cap; got {shown:?}");
    let withheld = ids.iter().copied().find(|id| !shown.contains(id)).unwrap();
    let mut shown_full: Vec<String> = shown.iter().map(|id| full(id)).collect();
    shown_full.sort();

    for id in &shown {
        assert_marker_exists(&full(id), &session);
    }
    // The withheld way was never shown, so it must not be recorded as fired.
    assert_marker_absent(&full(withheld), &session);
    let engagement = format!(
        "{}/{session}/way-engagement/capdomain__{withheld}.json",
        sessions_root()
    );
    assert!(!Path::new(&engagement).exists(), "withheld way started its refire curve");
    assert_eq!(fired_ways(&state, &session, "way_fired"), shown_full, "turn 1 fires only the shown ways");
    assert_eq!(
        events_of(&state, &session, "way_suppressed"),
        vec![(full(withheld), "context_cap".to_string())]
    );

    // Turn 2: the two shown ways are inside their refire window and are
    // suppressed; the withheld way is free to fire now, as a first fire.
    let out = scan_command_isolated(&session, "git commit -m y", &home, &state);
    let ctx = assert_pretooluse_envelope(&out, &format!("# Marker {withheld}\n"));
    for id in &shown {
        assert!(
            !ctx.contains(&format!("# Marker {id}\n")),
            "{id} re-delivered inside its refire window"
        );
    }
    assert_marker_exists(&full(withheld), &session);
    let mut all_fired = shown_full.clone();
    all_fired.push(full(withheld));
    all_fired.sort();
    assert_eq!(fired_ways(&state, &session, "way_fired"), all_fired, "turn 2 first-fires the withheld way");
    assert!(
        fired_ways(&state, &session, "way_redisclosed").is_empty(),
        "the withheld way was never delivered, so its turn-2 fire is not a redisclosure"
    );
    let refire_rows = || {
        let mut w: Vec<String> = events_of(&state, &session, "way_suppressed")
            .into_iter()
            .filter(|(_, r)| r == "refire")
            .map(|(w, _)| w)
            .collect();
        w.sort();
        w
    };
    assert_eq!(refire_rows(), shown_full, "each refire suppression is logged");

    // Turn 3: all three are inside their windows. Refire suppressions log at
    // most once per way per fire window, so only the turn-2 fire adds a row.
    scan_command_isolated(&session, "git commit -m z", &home, &state);
    assert_eq!(refire_rows(), all_fired, "refire rows are deduplicated per fire window");

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenario 13: concurrent hooks deliver a way once (#528 review) ──

#[test]
fn scenario_13_concurrent_scans_fire_a_way_once() {
    let base = std::env::temp_dir().join(format!("ways-sim-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    // The macro sleeps between the refire decision and the record, which is
    // the window two parallel PreToolUse hooks race through without a lock.
    write_commit_way(
        &home.join(".claude/hooks/ways/racedomain"),
        "slow",
        200,
        Some("sleep 1\necho macro-done\n"),
    );

    let session = format!("sim-s13-{}", std::process::id());
    clean_markers(&session);

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let (s, h, st) = (session.clone(), home.clone(), state.clone());
            std::thread::spawn(move || scan_command_isolated(&s, "git commit -m x", &h, &st))
        })
        .collect();
    let outs: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let delivered = outs.iter().filter(|o| o.contains("# Marker slow")).count();
    assert_eq!(delivered, 1, "exactly one concurrent hook delivers the way; got {outs:?}");
    // The two hooks append to one log at nearly the same moment (the winner's
    // way_fired, the loser's way_suppressed). Each record must land whole: a
    // record split across two writes interleaves and corrupts both lines.
    let log = std::fs::read_to_string(state.join("agent-ways/events.jsonl")).unwrap_or_default();
    for line in log.lines() {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "corrupt event log line (interleaved appends?): {line:?}"
        );
    }
    assert_eq!(
        fired_ways(&state, &session, "way_fired"),
        vec!["racedomain/slow".to_string()],
        "one way_fired across both hooks"
    );
    assert_eq!(
        events_of(&state, &session, "way_suppressed"),
        vec![("racedomain/slow".to_string(), "refire".to_string())],
        "the losing hook logs a refire suppression"
    );

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenario 14: `show way --budget-used` for check-post.sh ─────

#[test]
fn scenario_14_show_way_budget_used_withholds_with_exit_3() {
    let base = std::env::temp_dir().join(format!("ways-sim-show-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    write_commit_way(&home.join(".claude/hooks/ways/showdomain"), "big", 3960, None);
    let session = format!("sim-s14-{}", std::process::id());
    clean_markers(&session);

    let show = |used: &str| {
        Command::new(ways_bin())
            .args(["show", "way", "showdomain/big", "--session", &session, "--trigger", "postcheck"])
            .arg(format!("--budget-used={used}"))
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_STATE_HOME", &state)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("CLAUDE_PROJECT_DIR", "/tmp/nonexistent-project")
            .env_remove("CLAUDE_AGENT_ID")
            .output()
            .expect("Failed to run ways show way")
    };

    // 7,000 already spent: a 4,000-character way does not fit.
    let out = show("7000");
    assert_eq!(out.status.code(), Some(3), "withheld for the cap exits 3");
    assert!(out.stdout.is_empty());
    assert_marker_absent("showdomain/big", &session);
    assert_eq!(
        events_of(&state, &session, "way_suppressed"),
        vec![("showdomain/big".to_string(), "context_cap".to_string())]
    );

    // Nothing spent yet: the way is shown and recorded.
    let out = show("0");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("# Marker big"));
    assert_marker_exists("showdomain/big", &session);

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenarios 15–16: the file lane's admission order (#634) ────

fn scan_file_isolated(session: &str, path: &str, home: &Path, state: &Path) -> String {
    let output = Command::new(ways_bin())
        .args([
            "scan", "file",
            "--path", path,
            "--session", session,
            "--project", "/tmp/nonexistent-project",
        ])
        .env("HOME", home)
        .env("USERPROFILE", home) // see scan_prompt
        .env("XDG_STATE_HOME", state)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("CLAUDE_AGENT_ID")
        .output()
        .expect("Failed to run ways scan file");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Write a way at `root/rel` with a `files:` trigger, a `# Marker <name>`
/// heading, and `body_chars` of filler.
fn write_file_way(root: &Path, rel: &str, files: &str, body_chars: usize) {
    let name = rel.rsplit('/').next().unwrap();
    let dir = root.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    let body = format!("# Marker {name}\n\n{}\n", "lorem ipsum ".repeat(body_chars / 12));
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!("---\nfiles: {files}\nscope: agent\nrefire: 0.15\n---\n{body}"),
    )
    .unwrap();
}

/// Positions of each `# Marker <name>` heading in `ctx`, in `names` order;
/// `None` for a way that was not delivered.
fn marker_positions(ctx: &str, names: &[&str]) -> Vec<Option<usize>> {
    names.iter().map(|n| ctx.find(&format!("# Marker {n}\n"))).collect()
}

#[test]
fn scenario_15_file_lane_orders_by_specificity_and_skips_what_does_not_fit() {
    let base = std::env::temp_dir().join(format!("ways-sim-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    let root = home.join(".claude/hooks/ways/orderdomain");

    // `zz-named` names the file; `aa-glob` and `mm-small` match it by
    // extension. By id alone `aa-glob` would go first and take the room
    // `zz-named` needs, so this fails without the specificity order in
    // scan/order.rs. `zz-named/aa-child` sorts before its parent by name but
    // must follow it. `aa-glob` does not fit once the named tree is in, and is
    // skipped; the smaller `mm-small` after it still fits.
    write_file_way(&root, "zz-named", r"notes\.ordered$", 5000);
    write_file_way(&root, "zz-named/aa-child", r"notes\.ordered$", 1000);
    write_file_way(&root, "aa-glob", r"\.ordered$", 6000);
    write_file_way(&root, "mm-small", r"\.ordered$", 500);

    let session = format!("sim-s15-{}", std::process::id());
    clean_markers(&session);

    let out = scan_file_isolated(&session, "/work/notes.ordered", &home, &state);
    let ctx = assert_pretooluse_envelope(&out, "# Marker zz-named\n");
    let pos = marker_positions(&ctx, &["zz-named", "aa-child", "aa-glob", "mm-small"]);
    let (named, child, glob, small) = (pos[0], pos[1], pos[2], pos[3]);
    assert!(named < child, "a parent precedes its child: {pos:?}");
    assert!(child.is_some() && small.is_some(), "both fit: {pos:?}");
    assert!(child < small, "the named tree precedes the extension match: {pos:?}");
    assert!(glob.is_none(), "aa-glob does not fit and is skipped: {pos:?}");
    assert_eq!(
        events_of(&state, &session, "way_suppressed"),
        vec![("orderdomain/aa-glob".to_string(), "context_cap".to_string())]
    );

    // The next matching edit delivers the skipped way; the others are inside
    // their refire windows.
    let out = scan_file_isolated(&session, "/work/notes.ordered", &home, &state);
    assert_pretooluse_envelope(&out, "# Marker aa-glob\n");

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn scenario_16_readme_edit_delivers_validate_in_the_first_hook() {
    // The tier-1 live fixture's case: an edit to README.md matches the
    // documentation tree and the extension-wide branching way, more than one
    // hook holds. The ways that name README go first, the `\.md$` way is
    // skipped, and branching still fits after it.
    let base = std::env::temp_dir().join(format!("ways-sim-readme-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    let root = home.join(".claude/hooks/ways");

    write_file_way(&root, "documentation", r"README\.md$|docs/.*\.md$|mkdocs\.ya?ml$", 2940);
    write_file_way(&root, "documentation/markdown", r"\.md$", 3720);
    write_file_way(
        &root,
        "documentation/validate",
        r"README\.md$|docs/.*(guide|tutorial|getting.?started|onboarding|index)\.md$",
        3823,
    );
    write_file_way(&root, "softwaredev/delivery/branching", r"\.(md|rs|sh|py|js|ts|json)$", 1289);

    let session = format!("sim-s16-{}", std::process::id());
    clean_markers(&session);

    let out = scan_file_isolated(&session, "/repo/README.md", &home, &state);
    let ctx = assert_pretooluse_envelope(&out, "# Marker validate\n");
    let pos = marker_positions(&ctx, &["documentation", "validate", "markdown", "branching"]);
    assert!(pos[0].is_some() && pos[0] < pos[1], "documentation, then validate: {pos:?}");
    assert!(pos[1] < pos[3], "branching follows the documentation tree: {pos:?}");
    assert!(pos[2].is_none(), "markdown does not fit and is skipped: {pos:?}");
    assert_eq!(
        events_of(&state, &session, "way_suppressed"),
        vec![("documentation/markdown".to_string(), "context_cap".to_string())]
    );

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenario 17: token position reads this session's transcript ──

/// Write a transcript for `session` under Claude Code's projects dir for
/// `slug`, whose one assistant turn reports `tokens` of context.
fn write_transcript(home: &Path, slug: &str, session: &str, tokens: u64) {
    let dir = home.join(".claude/projects").join(slug);
    std::fs::create_dir_all(&dir).unwrap();
    let line = format!(
        r#"{{"type":"assistant","message":{{"model":"claude-opus-5-5","usage":{{"input_tokens":{tokens},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
    );
    std::fs::write(dir.join(format!("{session}.jsonl")), format!("{line}\n")).unwrap();
}

/// `ways scan command` in `project`, with the isolation of
/// [`scan_command_isolated`]; returns the `token_position` of each `way_fired`
/// row for `session`.
fn fired_token_positions(session: &str, project: &str, home: &Path, state: &Path) -> Vec<u64> {
    Command::new(ways_bin())
        .args([
            "scan", "command",
            "--command", "git commit -m x",
            "--session", session,
            "--project", project,
        ])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("CLAUDE_PROJECT_DIR", project)
        .env_remove("CLAUDE_AGENT_ID")
        .output()
        .expect("Failed to run ways scan command");
    let log = std::fs::read_to_string(state.join("agent-ways/events.jsonl")).unwrap_or_default();
    log.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["event"] == "way_fired" && v["session"] == session)
        .filter_map(|v| v["token_position"].as_str().and_then(|s| s.parse().ok()))
        .collect()
}

#[test]
fn scenario_17_token_position_ignores_a_newer_sibling_session() {
    // Two sessions in one project: the other one wrote its transcript last.
    // The firing session's tick must come from its own transcript.
    let base = std::env::temp_dir().join(format!("ways-sim-tick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    write_commit_way(&home.join(".claude/hooks/ways/tickdomain"), "tick", 200, None);

    let project = "/srv/tick-project";
    let session = format!("sim-s17-{}", std::process::id());
    clean_markers(&session);
    write_transcript(&home, "-srv-tick-project", &session, 40_000);
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_transcript(&home, "-srv-tick-project", "other-session", 150_000);

    assert_eq!(fired_token_positions(&session, project, &home, &state), vec![40_000]);

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn scenario_18_token_position_finds_an_underscore_project() {
    // Claude Code maps every non-alphanumeric character of the project path
    // to '-', so `_prod` is stored as `-prod`.
    let base = std::env::temp_dir().join(format!("ways-sim-slug-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    write_commit_way(&home.join(".claude/hooks/ways/slugdomain"), "slug", 200, None);

    let project = "/srv/mcp/_prod";
    let session = format!("sim-s18-{}", std::process::id());
    clean_markers(&session);
    write_transcript(&home, "-srv-mcp--prod", &session, 40_000);

    assert_eq!(fired_token_positions(&session, project, &home, &state), vec![40_000]);

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}
