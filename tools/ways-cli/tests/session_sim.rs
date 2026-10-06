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

/// A `ways` command isolated from the operator's install and session.
///
/// HOME and every XDG base dir point at the test's own tree, so the binary
/// never reads the real `~/.claude`, user config, app data, or state. The
/// working directory and `PWD` point at an empty project dir the test owns:
/// with `CLAUDE_PROJECT_DIR` unset, project config (`.claude/ways.yaml`) is
/// read from `PWD`, so the directory `cargo test` was started in would
/// otherwise decide which ways are enabled. The variables Claude Code and
/// agent-ways export into an agent shell are cleared. Callers that need one
/// of them set it after this call. `XDG_RUNTIME_DIR` is inherited: the
/// session markers live under it and [`sessions_root`] reads the same value.
fn ways_cmd(home: &Path, cache: &Path, state: &Path) -> Command {
    let project = sim_root().join("project");
    let mut cmd = Command::new(ways_bin());
    cmd.current_dir(&project)
        .env("PWD", &project)
        .env("HOME", home)
        // home_dir() prefers USERPROFILE on Windows, so set both or the
        // fixture-home redirection is ignored and the binary reads the real
        // ~/.claude. See util::home_dir().
        .env("USERPROFILE", home)
        .env("XDG_CACHE_HOME", cache)
        .env("XDG_STATE_HOME", state)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_BIN_HOME", home.join(".local/bin"));
    for var in [
        "CLAUDE_PROJECT_DIR",
        "CLAUDE_AGENT_ID",
        "CLAUDE_SESSION_ID",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CONFIG_DIR",
        "WAYS_AGENT_SOCK",
        "WAYS_CLAUDE_BIN",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

/// Build the fixture corpus into a per-scenario XDG cache root and return
/// that root. The embedding engine is looked up under the fixture data dir,
/// where none is installed, so the corpus is keyword-only on every machine.
fn generate_corpus(name: &str) -> PathBuf {
    // Per-test cache root avoids races when tests run in parallel
    let cache = sim_root().join(format!("corpus-{name}"));
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::create_dir(&cache).unwrap();

    let home = fixture_home();
    let output = ways_cmd(&home, &cache, &home.join(".local/state"))
        .args(["corpus", "--ways-dir"])
        .arg(fixture_ways_dir())
        .arg("--quiet")
        .output()
        .expect("Failed to run ways corpus");
    assert!(
        output.status.success(),
        "ways corpus failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let corpus = cache.join("agent-ways/user/ways-corpus.jsonl");
    assert!(corpus.exists(), "ways corpus wrote no corpus at {}", corpus.display());
    cache
}

struct Session {
    id: String,
    /// The XDG cache root holding this session's corpus.
    cache: PathBuf,
}

impl Session {
    /// A `ways` command against the fixture home and this session's corpus.
    fn cmd(&self) -> Command {
        self.cmd_with_home(&fixture_home())
    }

    /// A `ways` command against `home` and this session's corpus.
    fn cmd_with_home(&self, home: &Path) -> Command {
        ways_cmd(home, &self.cache, &home.join(".local/state"))
    }

    fn new(name: &str) -> Self {
        let id = format!("sim-{}-{}", name, std::process::id());
        let cache = generate_corpus(name);
        // Clean any stale markers
        clean_markers(&id);
        Session { id, cache }
    }

    fn scan_prompt(&self, query: &str) -> String {
        let output = self.cmd()
            .args([
                "scan", "prompt",
                "--query", query,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .output()
            .expect("Failed to run ways scan prompt");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_command(&self, cmd: &str) -> String {
        let output = self.cmd()
            .args([
                "scan", "command",
                "--command", cmd,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .output()
            .expect("Failed to run ways scan command");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_file(&self, path: &str) -> String {
        let output = self.cmd()
            .args([
                "scan", "file",
                "--path", path,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
            .output()
            .expect("Failed to run ways scan file");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_prompt_with_project(&self, query: &str, project: &str) -> String {
        let output = self.cmd()
            .args([
                "scan", "prompt",
                "--query", query,
                "--session", &self.id,
                "--project", project,
            ])
            .output()
            .expect("Failed to run ways scan prompt");

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn scan_prompt_with_home(&self, query: &str, home: &Path) -> String {
        let output = self.cmd_with_home(home)
            .args([
                "scan", "prompt",
                "--query", query,
                "--session", &self.id,
                "--project", "/tmp/nonexistent-project",
            ])
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
        let output = self.cmd()
            .args(&args)
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

/// Prefix of the per-process sim root under the temp dir.
const SIM_ROOT_PREFIX: &str = "ways-sim-home-";

/// Sim roots older than this belong to finished runs and are swept.
const SIM_ROOT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// This test process's private tree: the fixture home (`home/`), the
/// per-scenario corpus caches (`corpus-<name>/`), scenario homes, and the
/// empty project dir `ways_cmd` runs in (`project/`).
///
/// One per test process, built fresh on first use. A home shared by path
/// across runs outlived the checkout it linked to: a removed worktree left
/// `hooks/ways` dangling, `exists()` reported it missing, the relink failed
/// silently on the existing link, and every fixture way went unseen.
///
/// A test binary has no global teardown, so each run sweeps the sim roots of
/// runs older than [`SIM_ROOT_MAX_AGE`] instead of removing its own.
fn sim_root() -> &'static Path {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let tmp = std::env::temp_dir();
        let root = tmp.join(format!("{SIM_ROOT_PREFIX}{}", std::process::id()));
        sweep_stale_sim_roots(&tmp, &root);
        let _ = std::fs::remove_dir_all(&root); // a reused pid's leftovers
        // create_dir, not create_dir_all: a directory that survived the wipe
        // (one another user owns in a shared /tmp) fails here, loudly.
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("project")).unwrap();

        let ways_link = root.join("home/.claude/hooks/ways");
        std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
        // Put the fixture ways where the binary expects them
        // ($HOME/.claude/hooks/ways).
        #[cfg(unix)]
        std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).unwrap();
        // Windows symlinks need admin / Developer Mode (the Makefile copies for
        // the same reason), so copy the tree in.
        #[cfg(windows)]
        copy_dir_all(&fixture_ways_dir(), &ways_link).unwrap();
        root
    })
}

/// Remove sibling sim roots last modified more than [`SIM_ROOT_MAX_AGE`] ago.
/// A run still in progress created its root moments ago, so it is never swept.
fn sweep_stale_sim_roots(tmp: &Path, own: &Path) {
    let Ok(entries) = std::fs::read_dir(tmp) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_sim_root = entry.file_name().to_string_lossy().starts_with(SIM_ROOT_PREFIX);
        if !is_sim_root || path == own {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > SIM_ROOT_MAX_AGE);
        if stale {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// The fixture HOME — ways looks for ~/.claude/hooks/ways/
fn fixture_home() -> PathBuf {
    sim_root().join("home")
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

#[test]
fn scenario_1_ignores_the_invokers_project_config() {
    // Project config is read from PWD when CLAUDE_PROJECT_DIR is unset. Run
    // scenario 1 as a child test process from a directory whose ways.yaml
    // disables the way it expects to fire: ways_cmd must not let it through.
    let cwd = sim_root().join("invoker-cwd");
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(cwd.join(".claude")).unwrap();
    std::fs::write(
        cwd.join(".claude/ways.yaml"),
        "ways:\n  testdomain/parent/child: false\n",
    )
    .unwrap();

    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "scenario_1_basic_prompt_matching", "--test-threads=1"])
        .current_dir(&cwd)
        .env("PWD", &cwd)
        .env_remove("CLAUDE_PROJECT_DIR")
        .output()
        .expect("Failed to rerun scenario 1");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "scenario 1 failed when run from a dir that disables its way:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
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

    // Create a fixture home whose user config disables "testdomain"
    // (inside this process's sim root, so no other run shares it)
    let home = sim_root().join("home-s9");
    let _ = std::fs::remove_dir_all(&home);
    let claude_dir = home.join(".claude");

    // Place fixture ways under this scenario's private home.
    let ways_link = claude_dir.join("hooks/ways");
    std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).unwrap();
    // Windows: copy instead of symlink (needs admin/Developer Mode otherwise).
    #[cfg(windows)]
    copy_dir_all(&fixture_ways_dir(), &ways_link).unwrap();

    // Write the user config disabling testdomain
    let cfg_dir = home.join(".config/agent-ways");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("config.yaml"), "disabled_domains: [testdomain]\n").unwrap();

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
    let output = ways_cmd(home, &home.join(".cache"), state)
        .args([
            "scan", "command",
            "--command", cmd,
            "--session", session,
            "--project", "/tmp/nonexistent-project",
        ])
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

// ── Scenario 14: the post-tool scan shares one budget (#702) ────

#[cfg(unix)]
#[test]
fn scenario_14_post_tool_scan_shares_one_budget_across_postchecks() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!("ways-sim-post-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    let root = home.join(".claude/hooks/ways/postdomain");
    // Two 6,000-character ways whose postchecks both request firing: the
    // first is admitted, the second does not fit what is left of the cap.
    for id in ["a", "b"] {
        write_commit_way(&root, id, 6000, None);
        let check = root.join(id).join("postcheck.sh");
        std::fs::write(&check, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
        std::fs::set_permissions(&check, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let session = format!("sim-s14-{}", std::process::id());
    clean_markers(&session);

    let mut child = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["hook", "post-tool"])
        .env("CLAUDE_PROJECT_DIR", "/tmp/nonexistent-project")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to run ways hook post-tool");
    let payload = format!(r#"{{"session_id":"{session}","hook_event_name":"PostToolUse","tool_name":"Edit"}}"#);
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim()).expect("one JSON envelope");
    assert_eq!(envelope["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let ctx = envelope["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(ctx.contains("# Marker a") && !ctx.contains("# Marker b"), "{ctx:.80}");
    assert_marker_exists("postdomain/a", &session);
    assert_marker_absent("postdomain/b", &session);
    assert_eq!(
        events_of(&state, &session, "way_suppressed"),
        vec![("postdomain/b".to_string(), "context_cap".to_string())]
    );

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenarios 15–16: the file lane's admission order (#634) ────

fn scan_file_isolated(session: &str, path: &str, home: &Path, state: &Path) -> String {
    let output = ways_cmd(home, &home.join(".cache"), state)
        .args([
            "scan", "file",
            "--path", path,
            "--session", session,
            "--project", "/tmp/nonexistent-project",
        ])
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
    ways_cmd(home, &home.join(".cache"), state)
        .args([
            "scan", "command",
            "--command", "git commit -m x",
            "--session", session,
            "--project", project,
        ])
        .env("CLAUDE_PROJECT_DIR", project)
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

    // The scan above falls back to every project dir on a slug miss, so it
    // cannot pin the slug. `ways context --project` reads the slug's dir alone.
    let out = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["context", "--project", project, "--json"])
        .output()
        .expect("Failed to run ways context");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("ways context printed no JSON: {}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(json["tokens_used"], 40_000, "context read the project's own dir: {json}");

    clean_markers(&session);
    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenario: ways switched off for subagents (#768) ───────────

/// Run `ways hook command` for `git commit` as the main agent or, with
/// `agent`, from inside a subagent; return stdout.
#[cfg(unix)]
fn hook_command(home: &Path, state: &Path, project: &Path, session: &str, agent: Option<&str>) -> String {
    hook_bash(home, state, project, session, agent, "git commit -m x")
}

/// Run `ways hook command` for `command` as the main agent or, with `agent`,
/// from inside a subagent; return stdout.
#[cfg(unix)]
fn hook_bash(home: &Path, state: &Path, project: &Path, session: &str, agent: Option<&str>, command: &str) -> String {
    use std::io::Write;
    let mut child = ways_cmd(home, &home.join(".cache"), state)
        .args(["hook", "command"])
        .env("CLAUDE_PROJECT_DIR", project)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to run ways hook command");
    let agent_field = agent.map(|a| format!(r#","agent_id":"{a}""#)).unwrap_or_default();
    let payload = format!(
        r#"{{"session_id":"{session}"{agent_field},"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{{"command":"{command}"}}}}"#
    );
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Run `ways hook task` for a delegation that names the way's keyword.
#[cfg(unix)]
fn hook_task(home: &Path, state: &Path, project: &Path, session: &str) {
    use std::io::Write;
    let mut child = ways_cmd(home, &home.join(".cache"), state)
        .args(["hook", "task"])
        .env("CLAUDE_PROJECT_DIR", project)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to run ways hook task");
    let payload = format!(
        r#"{{"session_id":"{session}","hook_event_name":"PreToolUse","tool_name":"Task","tool_input":{{"prompt":"deploy the service"}}}}"#
    );
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
}

#[cfg(unix)]
#[test]
fn scenario_subagent_switch_keeps_ways_from_subagents_only() {
    let base = std::env::temp_dir().join(format!("ways-sim-subagents-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let state = base.join("state");
    let project = base.join("project");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    // A way that reaches the main agent and subagents alike.
    let dir = home.join(".claude/hooks/ways/subdomain/w");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("w.md"),
        "---\ndescription: test way w\npattern: \\bdeploy\\b\npattern_strict: true\ncommands: ^git\\ commit\nscope: agent, subagent\nrefire: 0.15\n---\n# Marker w\n",
    )
    .unwrap();
    let events = || std::fs::read_to_string(state.join("agent-ways/events.jsonl")).unwrap_or_default();
    let suppressed = |log: &str| log.lines().filter(|l| l.contains("\"injection_suppressed\"")).count();

    let stashes = |session: &str| {
        std::fs::read_dir(Path::new(&sessions_root()).join(session).join("subagent-stash")).map_or(0, |d| d.count())
    };

    // Switched on (the default): a subagent's command fires the way, and a
    // dispatch stashes ways for the subagent it starts.
    let s1 = format!("sim-sub1-{}", std::process::id());
    clean_markers(&s1);
    assert!(hook_command(&home, &state, &project, &s1, Some("a1")).contains("# Marker w"));
    hook_task(&home, &state, &project, &s1);
    assert_eq!(stashes(&s1), 1);

    // The session switch: the subagent gets nothing, the main agent keeps its way,
    // and the suppression is logged once per agent.
    let s2 = format!("sim-sub2-{}", std::process::id());
    clean_markers(&s2);
    let off = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["session", "subagents", "off", "--session", &s2, "--json"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(off.status.success(), "{}", String::from_utf8_lossy(&off.stderr));
    let report: serde_json::Value = serde_json::from_slice(&off.stdout).unwrap();
    assert_eq!((report["subagents"].as_str(), report["switch"].as_str()), (Some("off"), Some("session")));
    assert_eq!(hook_command(&home, &state, &project, &s2, Some("a2")), "");
    assert_eq!(hook_command(&home, &state, &project, &s2, Some("a2")), "");
    assert_eq!(suppressed(&events()), 1, "one line per agent");
    let line = events().lines().find(|l| l.contains("injection_suppressed")).unwrap().to_string();
    assert!(line.contains("\"switch\":\"session\"") && line.contains("\"agent\":\"a2\""), "{line}");
    assert!(hook_command(&home, &state, &project, &s2, None).contains("# Marker w"), "the main agent keeps its ways");
    hook_task(&home, &state, &project, &s2);
    assert_eq!(stashes(&s2), 0, "a dispatch stashes nothing");

    // Compaction clears the session's state; the switch survives it.
    let compact = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["session", "reset", "--session", &s2, "--confirm"])
        .output()
        .unwrap();
    assert!(compact.status.success());
    assert_eq!(hook_command(&home, &state, &project, &s2, Some("a2b")), "", "still off after the state is cleared");

    // Switching needs a named session: --session, or the one this process runs in.
    let guess = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["session", "subagents", "off"])
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!guess.status.success(), "no guessed session is switched");
    let own = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["session", "subagents", "on", "--json"])
        .env("CLAUDE_CODE_SESSION_ID", &s2)
        .current_dir(&project)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&own.stdout).unwrap();
    assert_eq!((report["session"].as_str(), report["subagents"].as_str()), (Some(s2.as_str()), Some("on")));
    assert!(hook_command(&home, &state, &project, &s2, Some("a2c")).contains("# Marker w"));
    assert!(events().lines().any(|l| l.contains("injection_suppressed") && l.contains("\"lane\":\"task\"")));

    // The project setting does the same for every session in the project.
    std::fs::write(project.join(".claude/ways.yaml"), "subagents: false\n").unwrap();
    let s3 = format!("sim-sub3-{}", std::process::id());
    clean_markers(&s3);
    assert_eq!(hook_command(&home, &state, &project, &s3, Some("a3")), "");
    assert!(events().lines().any(|l| l.contains("injection_suppressed") && l.contains("\"switch\":\"config\"")));
    assert!(hook_command(&home, &state, &project, &s3, None).contains("# Marker w"));

    // SessionStart prunes switches untouched for 30 days and keeps fresh ones.
    let switches = state.join("agent-ways/subagent-switch");
    std::fs::create_dir_all(&switches).unwrap();
    std::fs::write(switches.join("sim-old-switch"), "").unwrap();
    std::fs::write(switches.join("sim-new-switch"), "").unwrap();
    let forty_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600);
    std::fs::File::options().write(true).open(switches.join("sim-old-switch")).unwrap().set_modified(forty_days_ago).unwrap();
    {
        use std::io::Write;
        let mut child = ways_cmd(&home, &home.join(".cache"), &state)
            .args(["hook", "session-start"])
            .env("CLAUDE_PROJECT_DIR", &project)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(br#"{"session_id":"sim-prune","hook_event_name":"SessionStart","source":"startup"}"#).unwrap();
        assert!(child.wait().unwrap().success());
    }
    assert!(!switches.join("sim-old-switch").exists(), "a 40-day-old switch is pruned");
    assert!(switches.join("sim-new-switch").exists(), "a fresh one stays");

    let _ = std::fs::remove_dir_all(&base);
}

// ── Scenario: firing state is kept per agent (#815) ────────────

/// A home with way `agentdomain/w` (fires on `git commit`, and on a
/// delegation that says "deploy") and way
/// `agentdomain/dep`, whose check fires on `npm install` and pulls the way in
/// the first time each agent sees it. Everything reaches subagents. Returns
/// (base, home, state, project).
#[cfg(unix)]
fn per_agent_fixture(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("ways-sim-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let project = base.join("project");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    let ways = home.join(".claude/hooks/ways/agentdomain");
    std::fs::create_dir_all(ways.join("w")).unwrap();
    std::fs::write(
        ways.join("w/w.md"),
        "---\ndescription: test way w\npattern: \\bdeploy\\b\npattern_strict: true\ncommands: ^git\\ commit\nscope: agent, subagent\nrefire: 0.15\n---\n# Marker w\n",
    )
    .unwrap();
    std::fs::create_dir_all(ways.join("dep")).unwrap();
    std::fs::write(
        ways.join("dep/dep.md"),
        "---\ndescription: dependency audit\nscope: agent, subagent\nrefire: 0.15\n---\n# Marker dep\n",
    )
    .unwrap();
    std::fs::write(
        ways.join("dep/dep.check.md"),
        "---\ndescription: dependency install check\ncommands: ^npm\\ install\nscope: agent, subagent\n---\n## anchor\n\nAudit first.\n\n## check\n\n- [ ] Package is maintained\n",
    )
    .unwrap();
    let state = base.join("state");
    (base, home, state, project)
}

/// The `check_fired` rows for `session`, as (agent_id, fire_count).
#[cfg(unix)]
fn check_fires_logged(state: &Path, session: &str) -> Vec<(String, String)> {
    let log = std::fs::read_to_string(state.join("agent-ways/events.jsonl")).unwrap_or_default();
    log.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["event"] == "check_fired" && v["session"] == session)
        .map(|v| {
            (
                v["agent_id"].as_str().unwrap_or("").to_string(),
                v["fire_count"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn scenario_per_agent_refire_window() {
    let (base, home, state, project) = per_agent_fixture("agent-refire");
    let s = format!("sim-agent-refire-{}", std::process::id());
    clean_markers(&s);
    let root = Path::new(&sessions_root()).join(&s);
    let engagement = root.join("way-engagement/agentdomain__w.json");

    // Main fires the way; a second match inside its refire window is suppressed.
    assert!(hook_command(&home, &state, &project, &s, None).contains("# Marker w"));
    let main_window = std::fs::read(&engagement).expect("main's engagement at the session root");
    assert_eq!(hook_command(&home, &state, &project, &s, None), "");

    // A subagent in the same session gets the way: main's fire does not hold it back.
    assert!(
        hook_command(&home, &state, &project, &s, Some("asub1")).contains("# Marker w"),
        "the subagent was refire-suppressed by main's fire"
    );
    assert_eq!(hook_command(&home, &state, &project, &s, Some("asub1")), "", "the subagent keeps its own window");
    assert!(root.join("agents/asub1/way-engagement/agentdomain__w.json").is_file());

    // The subagent's fire leaves main's window as it was.
    assert_eq!(std::fs::read(&engagement).unwrap(), main_window, "main's refire window changed");
    assert_eq!(hook_command(&home, &state, &project, &s, None), "");

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn scenario_per_agent_check_decay() {
    let (base, home, state, project) = per_agent_fixture("agent-check");
    let s = format!("sim-agent-check-{}", std::process::id());
    clean_markers(&s);

    // Agent A fires the check twice.
    hook_bash(&home, &state, &project, &s, Some("aaa"), "npm install left-pad");
    hook_bash(&home, &state, &project, &s, Some("aaa"), "npm install right-pad");
    assert_eq!(check_fires_logged(&state, &s), vec![("aaa".into(), "1".into()), ("aaa".into(), "2".into())]);

    // Agent B's first check fire counts only its own fires: decay 1/1, not 1/3.
    hook_bash(&home, &state, &project, &s, Some("abb"), "npm install left-pad");
    assert_eq!(check_fires_logged(&state, &s).last(), Some(&("abb".into(), "1".into())));
    let fires = |agent: &str| {
        let p = Path::new(&sessions_root()).join(&s).join("agents").join(agent).join("check-fires/agentdomain/dep/.value");
        std::fs::read_to_string(p).unwrap_or_default()
    };
    assert_eq!((fires("aaa").as_str(), fires("abb").as_str()), ("2", "1"));

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn scenario_legacy_session_state_reads_as_mains() {
    let (base, home, state, project) = per_agent_fixture("agent-legacy");
    let s = format!("sim-agent-legacy-{}", std::process::id());
    clean_markers(&s);

    // A check count an older binary wrote at the session root.
    let legacy = Path::new(&sessions_root()).join(&s).join("check-fires/agentdomain/dep/.value");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "3").unwrap();

    // A subagent starts from zero; main continues from the legacy count.
    hook_bash(&home, &state, &project, &s, Some("anew"), "npm install left-pad");
    hook_bash(&home, &state, &project, &s, None, "npm install left-pad");
    assert_eq!(
        check_fires_logged(&state, &s),
        vec![("anew".into(), "1".into()), ("main".into(), "4".into())]
    );
    assert_eq!(std::fs::read_to_string(&legacy).unwrap().trim(), "4");

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}

/// Run `ways hook <event>` with a raw payload; return stdout.
#[cfg(unix)]
fn hook_raw(home: &Path, state: &Path, project: &Path, event: &str, payload: &str) -> String {
    use std::io::Write;
    let mut child = ways_cmd(home, &home.join(".cache"), state)
        .args(["hook", event])
        .env("CLAUDE_PROJECT_DIR", project)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to run ways hook");
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[cfg(unix)]
#[test]
fn scenario_per_agent_epoch() {
    let (base, home, state, project) = per_agent_fixture("agent-epoch");
    let s = format!("sim-agent-epoch-{}", std::process::id());
    clean_markers(&s);

    hook_bash(&home, &state, &project, &s, None, "ls");
    assert_epoch(&s, 1);
    for _ in 0..3 {
        hook_bash(&home, &state, &project, &s, Some("asub"), "ls");
    }
    // The subagent's tool calls advance its own epoch, not main's.
    assert_epoch(&s, 1);
    let sub = std::fs::read_to_string(Path::new(&sessions_root()).join(&s).join("agents/asub/epoch")).unwrap();
    assert_eq!(sub.trim(), "3");

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn scenario_subagent_start_records_injected_ways() {
    let (base, home, state, project) = per_agent_fixture("agent-inject");
    let s = format!("sim-agent-inject-{}", std::process::id());
    clean_markers(&s);
    let root = Path::new(&sessions_root()).join(&s);
    let start = |agent: &str| {
        hook_raw(
            &home, &state, &project, "subagent-start",
            &format!(r#"{{"session_id":"{s}","agent_id":"{agent}","agent_type":"general-purpose","hook_event_name":"SubagentStart"}}"#),
        )
    };

    // Main dispatches; the subagent starts with the way injected.
    hook_task(&home, &state, &project, &s);
    assert!(start("ainj").contains("# Marker w"));
    assert!(root.join("agents/ainj/way-engagement/agentdomain__w.json").is_file());
    assert!(root.join("ways/agentdomain/w/.marker.ainj").is_file());
    // Its first matching command does not deliver the way a second time.
    assert_eq!(hook_command(&home, &state, &project, &s, Some("ainj")), "");
    // Main's state is untouched: main still gets the way.
    assert!(hook_command(&home, &state, &project, &s, None).contains("# Marker w"));

    // A teammate's scope marker lands in its own state, not main's.
    hook_raw(
        &home, &state, &project, "task",
        &format!(r#"{{"session_id":"{s}","hook_event_name":"PreToolUse","tool_name":"Task","tool_input":{{"prompt":"deploy the service","team_name":"red"}}}}"#),
    );
    start("atm");
    assert_eq!(std::fs::read_to_string(root.join("agents/atm/teammate")).unwrap().trim(), "red");
    assert!(!root.join("teammate").exists(), "main's scope stays agent");

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn scenario_session_ways_lists_rows_per_agent() {
    let (base, home, state, project) = per_agent_fixture("agent-list");
    let s = format!("sim-agent-list-{}", std::process::id());
    clean_markers(&s);

    // Main fires w; a subagent fires w and, through its check, dep.
    hook_command(&home, &state, &project, &s, None);
    hook_command(&home, &state, &project, &s, Some("alist"));
    hook_bash(&home, &state, &project, &s, Some("alist"), "npm install left-pad");

    let out = ways_cmd(&home, &home.join(".cache"), &state)
        .args(["session", "ways", "--session", &s, "--json"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows: Vec<(String, Option<String>, u64)> = json["ways"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["id"].as_str().unwrap().to_string(),
                w["agent_id"].as_str().map(str::to_string),
                w["check_fires"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("agentdomain/w".into(), None, 0),
            ("agentdomain/w".into(), Some("alist".into()), 0),
            ("agentdomain/dep".into(), Some("alist".into()), 1),
        ],
        "main's row first and unlabelled, then the subagent's rows: {json}"
    );
    assert_eq!(json["ways_fired"], 2, "distinct ways, not rows");

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn scenario_queued_messages_are_main_agents_only() {
    let (base, home, state, project) = per_agent_fixture("agent-queued");
    let s = format!("sim-agent-queued-{}", std::process::id());
    clean_markers(&s);
    // Main's transcript holds an operator message queued mid-turn.
    let transcript = base.join("main.jsonl");
    std::fs::write(
        &transcript,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-10-03T12:00:00Z","content":"deploy the service now"}"#.to_string() + "\n",
    )
    .unwrap();
    let mark = Path::new(&sessions_root()).join(&s).join("queued-scan-mark");
    let post_tool = |agent: Option<&str>| {
        let agent_field = agent.map(|a| format!(r#","agent_id":"{a}""#)).unwrap_or_default();
        hook_raw(
            &home, &state, &project, "queued",
            &format!(
                r#"{{"session_id":"{s}"{agent_field},"transcript_path":"{}","hook_event_name":"PostToolUse","tool_name":"Bash"}}"#,
                transcript.display()
            ),
        )
    };

    // A subagent's PostToolUse names main's transcript; it must not consume the message.
    assert_eq!(post_tool(Some("aq")), "");
    assert!(!mark.exists(), "the subagent advanced main's queued-scan mark");
    // Main's next PostToolUse scans it and the way fires for main.
    assert!(post_tool(None).contains("# Marker w"));
    assert_eq!(std::fs::read_to_string(&mark).unwrap().trim(), "2026-10-03T12:00:00Z");

    clean_markers(&s);
    let _ = std::fs::remove_dir_all(&base);
}


// ── ADR-701 §2: event-log rotation runs from a real scan ───────

#[test]
fn scenario_event_log_rotates_old_lines_on_a_scan() {
    let s = Session::new("rot");
    let home = sim_root().join("home-rot");
    let _ = std::fs::remove_dir_all(&home);
    let ways_link = home.join(".claude/hooks/ways");
    std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).unwrap();
    #[cfg(windows)]
    copy_dir_all(&fixture_ways_dir(), &ways_link).unwrap();

    let state = home.join(".local/state/agent-ways");
    let log = state.join("events.jsonl");

    // A first scan writes a current line, which anchors the cutoff to real time.
    s.scan_prompt_with_home("how do I write a unit test for this module", &home);
    assert_marker_exists("testdomain/parent/child", &s.id);

    // Put an old line at the head and release today's claim, as the next day would.
    let current = std::fs::read_to_string(&log).unwrap();
    std::fs::write(&log, format!("{{\"ts\":\"2001-01-01T00:00:00Z\",\"event\":\"way_fired\",\"way\":\"ancient/way\"}}\n{current}")).unwrap();
    for e in std::fs::read_dir(&state).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("events.rotated-") {
            std::fs::remove_file(e.path()).unwrap();
        }
    }

    // A second scan fires another way: the event rotates the old line out first.
    s.scan_prompt_with_home("refactor extract method decompose this function", &home);
    assert_marker_exists("testdomain/parent/child2", &s.id);

    let got = std::fs::read_to_string(&log).unwrap();
    assert!(!got.contains("ancient/way"), "the old line is rotated out:\n{got}");
    assert!(got.contains("testdomain/parent/child2") && got.contains("testdomain/parent/child\""), "the scans' own events are kept:\n{got}");
    let claimed = std::fs::read_dir(&state)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("events.rotated-"));
    assert!(claimed, "today's rotation claim is on disk");

    let _ = std::fs::remove_dir_all(&home);
}

// ── ADR-701 §1: a project prefix toggle excludes the ways under it ─────

#[test]
fn scenario_prefix_toggle_excludes_ways_and_a_way_toggle_overrides_it() {
    let s = Session::new("prefix");
    let project = sim_root().join("project-prefix");
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    let scan = |query: &str| {
        s.cmd()
            .args(["scan", "prompt", "--query", query, "--session", &s.id, "--project"])
            .arg(&project)
            .env("CLAUDE_PROJECT_DIR", &project)
            .current_dir(&project)
            .env("PWD", &project)
            .output()
            .expect("Failed to run ways scan prompt");
    };

    std::fs::write(project.join(".claude/ways.yaml"), "ways:\n  testdomain/*: false\n").unwrap();
    scan("how do I write a unit test for this module");
    assert_marker_absent("testdomain/parent/child", &s.id);

    // An explicit toggle on the way itself overrides the prefix.
    std::fs::write(
        project.join(".claude/ways.yaml"),
        "ways:\n  testdomain/*: false\n  testdomain/parent/child: true\n",
    )
    .unwrap();
    scan("how do I write a unit test for this module");
    assert_marker_exists("testdomain/parent/child", &s.id);

    let _ = std::fs::remove_dir_all(&project);
}

// ── ADR-701 §2: history is archived, not deleted ───────────────

#[test]
fn scenario_event_history_is_archived_expired_and_read_back() {
    let s = Session::new("arch");
    let home = sim_root().join("home-arch");
    let _ = std::fs::remove_dir_all(&home);
    let ways_link = home.join(".claude/hooks/ways");
    std::fs::create_dir_all(ways_link.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture_ways_dir(), &ways_link).unwrap();
    #[cfg(windows)]
    copy_dir_all(&fixture_ways_dir(), &ways_link).unwrap();

    let state = home.join(".local/state/agent-ways");
    let log = state.join("events.jsonl");

    // A first scan writes a current line, which anchors the cutoff to real time.
    s.scan_prompt_with_home("how do I write a unit test for this module", &home);
    assert_marker_exists("testdomain/parent/child", &s.id);

    // Put an old line at the head, plant an archive far past the retention,
    // and release today's claim, as the next day would.
    let current = std::fs::read_to_string(&log).unwrap();
    std::fs::write(&log, format!("{{\"ts\":\"2001-01-01T00:00:00Z\",\"event\":\"way_fired\",\"way\":\"ancient/way\"}}\n{current}")).unwrap();
    ways_core::event_archive::append(&state, 981_000_000, b"{\"ts\":\"2001-02-01T00:00:00Z\",\"event\":\"way_fired\",\"way\":\"older/way\"}\n").unwrap();
    let hand_made = state.join("events-preserved-20261005.jsonl.gz");
    std::fs::write(&hand_made, b"not an archive name").unwrap();
    for e in std::fs::read_dir(&state).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("events.rotated-") {
            std::fs::remove_file(e.path()).unwrap();
        }
    }

    // A second scan: the old line leaves the live file for today's archive.
    s.scan_prompt_with_home("refactor extract method decompose this function", &home);
    assert_marker_exists("testdomain/parent/child2", &s.id);

    let live = std::fs::read_to_string(&log).unwrap();
    assert!(!live.contains("ancient/way"), "the old line left the live file:\n{live}");
    let archives = ways_core::event_archive::archives(&state);
    assert_eq!(archives.len(), 1, "today's archive only; the 2001 one expired: {archives:?}");
    let today = agent_fmt::when::utc_date(agent_fmt::when::now_secs());
    assert_eq!(archives[0].file_name().unwrap().to_string_lossy(), format!("events-{today}.jsonl.gz"));
    let archived = ways_core::event_archive::read_source(&archives[0]).unwrap();
    assert!(archived.contains("ancient/way") && !archived.contains("testdomain"), "{archived}");
    assert!(hand_made.exists(), "a file outside the archive pattern is left alone");

    // A reader sees the archived line as well as the live ones.
    let out = s.cmd_with_home(&home).args(["tune", "stats", "--global", "--json"]).output().expect("Failed to run ways tune stats");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("ancient/way") && text.contains("testdomain/parent/child2"), "stats reads archive and live:\n{text}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn scenario_session_dump_does_not_read_archives_older_than_the_session() {
    let home = sim_root().join("home-dump-arch");
    let _ = std::fs::remove_dir_all(&home);
    let state = home.join(".local/state/agent-ways");
    std::fs::create_dir_all(&state).unwrap();
    let fire = |ts: &str, way: &str| {
        format!("{{\"ts\":\"{ts}\",\"event\":\"way_fired\",\"way\":\"{way}\",\"session\":\"sim-dump-arch\",\"project\":\"/tmp/p-dump\",\"trigger\":\"semantic\",\"scope\":\"agent\"}}\n")
    };
    let start = "{\"ts\":\"2026-10-01T10:00:00Z\",\"event\":\"session_start\",\"session\":\"sim-dump-arch\",\"project\":\"/tmp/p-dump\"}\n";
    // An archive from before the session holds a fire for the same id: it is never read.
    ways_core::event_archive::append(&state, 1_790_000_000, fire("2026-09-01T10:00:00Z", "ghost/way").as_bytes()).unwrap();
    ways_core::event_archive::append(&state, 1_791_000_000, format!("{start}{}", fire("2026-10-01T10:01:00Z", "archived/way")).as_bytes()).unwrap();
    std::fs::write(state.join("events.jsonl"), fire("2026-10-02T10:00:00Z", "live/way")).unwrap();

    let out = ways_cmd(&home, &home.join(".cache"), &home.join(".local/state"))
        .args(["session", "dump", "--session", "sim-dump-arch", "--project", "/tmp/p-dump"])
        .output()
        .expect("Failed to run ways session dump");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("live/way") && text.contains("archived/way"), "the session's own history is read:\n{text}");
    assert!(!text.contains("ghost/way"), "an archive older than the session is not read:\n{text}");

    let _ = std::fs::remove_dir_all(&home);
}
