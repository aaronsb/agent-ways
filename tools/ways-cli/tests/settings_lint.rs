//! End-to-end tests for `ways settings lint` (ADR-147).
//!
//! Runs the real binary against committed fixture stores and asserts findings +
//! exit code. The unit tests in `cmd::settings::*` cover the checks in
//! isolation; these lock the CLI wiring: argument parsing, the human/JSON
//! reporters, and the non-zero exit on errors.

use std::path::PathBuf;
use std::process::Command;

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

fn store(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/settings-store")
        .join(name)
}

#[test]
fn clean_store_exits_zero() {
    let out = Command::new(ways_bin())
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
    let out = Command::new(ways_bin())
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
    let out = Command::new(ways_bin())
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
    let out = Command::new(ways_bin())
        .args(["settings", "lint"])
        .arg(store("does-not-exist"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "a missing store dir must fail");
}
