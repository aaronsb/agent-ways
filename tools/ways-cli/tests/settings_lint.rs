//! End-to-end tests for `ways settings lint` (ADR-147).
//!
//! Runs the real binary against committed fixture stores and asserts findings +
//! exit code. The unit tests in `cmd::settings::*` cover the checks in
//! isolation; these lock the CLI wiring: argument parsing, the human/JSON
//! reporters, and the non-zero exit on errors.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn tmp_store() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ways-it-settings-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ways_bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // test binary name
    path.pop(); // deps/
    path.push(if cfg!(windows) { "ways.exe" } else { "ways" });
    if !path.exists() {
        path = PathBuf::from(env!("CARGO_BIN_EXE_ways"));
    }
    path
}

/// The repo's shipped schema copy — the test env's data_root has no `share/`,
/// so we point the binary at it explicitly via the runtime override.
fn repo_schema() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../share/claude-code-settings.schema.json")
}

/// `ways` with the settings schema resolved to the repo copy.
fn ways() -> Command {
    let mut c = Command::new(ways_bin());
    c.env("WAYS_SETTINGS_SCHEMA_FILE", repo_schema());
    c
}

fn store(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/settings-store")
        .join(name)
}

#[test]
fn clean_store_exits_zero() {
    let out = ways()
        .args(["settings", "lint"])
        .arg(store("clean"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "expected exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("clean"), "expected a clean report; got: {stdout}");
}

#[test]
fn dirty_store_reports_every_check_and_exits_one() {
    let out = ways()
        .args(["settings", "lint"])
        .arg(store("dirty"))
        .output()
        .unwrap();
    // Errors present -> non-zero exit.
    assert_eq!(out.status.code(), Some(1), "expected exit 1 on errors");
    let s = String::from_utf8_lossy(&out.stdout);
    // 2 errors (bad-type, managed-only) + 3 warnings (overridable, unknown, dup).
    assert!(s.contains("2 error(s), 3 warning(s)"), "summary line; got:\n{s}");
    // scope-legal error
    assert!(s.contains("managed-only"), "managed-only scope error; got:\n{s}");
    // schema type error
    assert!(s.contains("expects number"), "schema type error; got:\n{s}");
    // schema unknown-key warning (never an error)
    assert!(s.contains("frobnicate"), "unknown-key warning; got:\n{s}");
    // managed-overridable warning
    assert!(s.contains("replaced by managed scope"), "overridable warning; got:\n{s}");
    // duplicate-scalar warning
    assert!(s.contains("last wins by filename order"), "duplicate warning; got:\n{s}");
}

#[test]
fn json_flag_emits_a_findings_array() {
    let out = ways()
        .args(["settings", "lint", "--json"])
        .arg(store("dirty"))
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.trim_start().starts_with('['), "expected a JSON array; got:\n{s}");
    assert!(s.contains("\"severity\""), "expected finding fields; got:\n{s}");
    assert!(s.contains("\"check\""), "expected finding fields; got:\n{s}");
}

#[test]
fn missing_store_dir_is_an_error() {
    let out = ways()
        .args(["settings", "lint"])
        .arg(store("does-not-exist"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "a missing store dir must fail");
}

#[test]
fn scaffold_then_lint_round_trips_clean() {
    let dir = tmp_store();
    // Scaffold a normal key from the schema.
    let new = ways()
        .args(["settings", "new", "cleanupPeriodDays", "--dir"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(new.status.success(), "scaffold failed: {}", String::from_utf8_lossy(&new.stderr));
    assert!(dir.join("10-cleanupPeriodDays.md").exists());

    // The generated fragment must lint clean end-to-end.
    let lint = ways()
        .args(["settings", "lint"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "scaffolded store must lint clean; stdout: {}",
        String::from_utf8_lossy(&lint.stdout)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scaffold_unknown_key_fails() {
    let dir = tmp_store();
    let out = ways()
        .args(["settings", "new", "totallyMadeUpKey", "--dir"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success(), "scaffolding an unknown key must fail");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn schema_source_prints_a_url() {
    let out = ways()
        .args(["settings", "schema", "--source"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.trim_start().starts_with("http"), "expected a URL; got: {s}");
    assert!(s.contains("schemastore"), "expected the default source; got: {s}");
}

#[test]
fn lint_degrades_when_schema_absent() {
    // Point at a nonexistent schema: schema-valid is skipped with a notice, but
    // the clean store still passes (warnings don't fail the exit code).
    let out = Command::new(ways_bin())
        .env("WAYS_SETTINGS_SCHEMA_FILE", "/nonexistent/cc-schema.json")
        .args(["settings", "lint"])
        .arg(store("clean"))
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("skipped"), "expected the schema-skipped notice; got:\n{s}");
    assert!(out.status.success(), "a warning-only run exits 0");
}

#[test]
fn schema_refresh_writes_the_user_copy() {
    // Hermetic: source is a file:// URL (no network), config home is a temp dir.
    let cfg = tmp_store();
    let src = repo_schema().canonicalize().unwrap();
    let out = Command::new(ways_bin())
        .env("XDG_CONFIG_HOME", &cfg)
        .env("WAYS_SETTINGS_SCHEMA_URL", format!("file://{}", src.display()))
        .args(["settings", "schema", "--refresh"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "refresh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written = cfg.join("agent-ways/claude-code-settings.schema.json");
    assert!(written.exists(), "refresh must write the durable user copy");
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn scaffold_fails_when_schema_absent() {
    let dir = tmp_store();
    let out = Command::new(ways_bin())
        .env("WAYS_SETTINGS_SCHEMA_FILE", "/nonexistent/cc-schema.json")
        .args(["settings", "new", "model", "--dir"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success(), "scaffolding without a schema must fail");
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(s.contains("not available") || s.contains("refresh"), "helpful error; got:\n{s}");
    let _ = std::fs::remove_dir_all(&dir);
}
