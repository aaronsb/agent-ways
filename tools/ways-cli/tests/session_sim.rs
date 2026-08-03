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
