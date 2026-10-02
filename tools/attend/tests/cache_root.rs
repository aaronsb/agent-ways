//! `attend whoami` and `attend status` name the cache root they use.
//!
//! Two processes with different `XDG_CACHE_HOME` read different trees and
//! see neither each other nor an error, so the root has to be visible.

use std::process::Command;

fn stdout_of(args: &[&str], cache: &std::path::Path, home: &std::path::Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_attend"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", cache)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env_remove("CLAUDE_SESSION_ID")
        .output()
        .expect("run attend");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn whoami_and_status_print_the_cache_root() {
    let home = std::env::temp_dir().join(format!("attend-cache-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // Long, as a TMPDIR or a deep home makes it: the row must not be cut.
    let cache = home.join(format!("elsewhere-cache-{}", "deep".repeat(30)));
    let root = cache.join("attend").to_string_lossy().into_owned();

    let whoami = stdout_of(&["whoami"], &cache, &home);
    assert!(whoami.contains(&root), "whoami does not name {root}: {whoami}");
    let status = stdout_of(&["status"], &cache, &home);
    assert!(status.contains(&root), "status does not name {root}: {status}");
    std::fs::remove_dir_all(&home).ok();
}
