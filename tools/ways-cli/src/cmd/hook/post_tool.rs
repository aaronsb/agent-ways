//! The post-tool scan: reactive firing through each way's `postcheck.sh`
//! (ADR-123 Decision 5), on PostToolUse and PostToolUseFailure.
//!
//! A way requests firing when its postcheck, fed the hook payload on stdin,
//! exits 0. The requests then pass the same gate predictive firing uses
//! (`show::way_scored`), so a reactive way does not fire during its refire
//! window, and they share one context budget.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cmd::show::{self, ContextBudget};
use crate::session;

/// A way's postcheck, by way id.
#[derive(Debug, PartialEq, Eq)]
pub struct Postcheck {
    pub way_id: String,
    pub script: PathBuf,
}

/// The ways roots in precedence order (ADR-143): project, user, core.
pub fn roots(project_dir: &str) -> Vec<PathBuf> {
    vec![
        Path::new(project_dir).join(".claude/ways"),
        crate::paths::user_ways_root(),
        crate::paths::projected_ways_root(),
    ]
}

/// Every executable `postcheck.sh` under `roots`, one per way id: a way in an
/// earlier root shadows the same id in a later one, as it does for the way
/// itself. Sorted by way id, so the fire order does not follow walk order.
pub fn find(roots: &[PathBuf]) -> Vec<Postcheck> {
    let mut found: std::collections::BTreeMap<String, PathBuf> = Default::default();
    for root in roots.iter().filter(|r| r.is_dir()) {
        let mut here: Vec<(String, PathBuf)> = ways_core::scanner::files(root)
            .filter(|p| p.file_name().is_some_and(|n| n == "postcheck.sh"))
            .filter(|p| show::is_executable(p))
            .filter_map(|p| {
                let way_dir = p.parent()?.strip_prefix(root).ok()?;
                Some((crate::util::path_to_id(way_dir), p))
            })
            .filter(|(id, _)| !id.is_empty())
            .collect();
        here.sort();
        for (id, script) in here {
            found.entry(id).or_insert(script);
        }
    }
    found.into_iter().map(|(way_id, script)| Postcheck { way_id, script }).collect()
}

/// Run every postcheck at once with the payload on stdin; the ids of those
/// that exit 0, in the order given. A postcheck sees `CLAUDE_SESSION_ID` and
/// `WAYS_SESSIONS_ROOT`; its output is discarded.
pub fn requests(checks: &[Postcheck], payload: &str, session_id: &str) -> Vec<String> {
    let root = session::sessions_root();
    let fired: Vec<bool> = std::thread::scope(|s| {
        let handles: Vec<_> = checks
            .iter()
            .map(|c| s.spawn(|| run_one(&c.script, payload, session_id, &root)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or(false)).collect()
    });
    checks.iter().zip(fired).filter(|(_, f)| *f).map(|(c, _)| c.way_id.clone()).collect()
}

fn run_one(script: &Path, payload: &str, session_id: &str, root: &str) -> bool {
    // Unix runs the script by its shebang; Windows has no shebang, so bash.
    #[cfg(unix)]
    let mut cmd = Command::new(script);
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = Command::new("bash");
        c.arg(script);
        c
    };
    let child = cmd
        .env("CLAUDE_SESSION_ID", session_id)
        .env("WAYS_SESSIONS_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        // A postcheck that exits without reading closes the pipe: not an error.
        let _ = stdin.write_all(payload.as_bytes());
    }
    child.wait().is_ok_and(|s| s.success())
}

/// The post-tool scan for one hook call: the context it injects, empty when
/// nothing fired.
pub fn scan(payload: &str, session_id: &str, project_dir: &str) -> anyhow::Result<String> {
    let checks = find(&roots(project_dir));
    let mut context = String::new();
    let mut budget = ContextBudget::hook();
    for id in requests(&checks, payload, session_id) {
        let out = show::way_scored(&id, session_id, "postcheck", None, None, None, Some(&mut budget))?;
        if !out.is_empty() {
            context.push_str(&out);
            context.push_str("\n\n");
            budget.charge("\n\n");
        }
    }
    Ok(context.trim_end().to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(root: &Path, way: &str, body: &str, mode: u32) -> PathBuf {
        let dir = root.join(way);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("postcheck.sh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn find_takes_one_postcheck_per_way_by_precedence() {
        let base = std::env::temp_dir().join(format!("ways-postcheck-find-{}", std::process::id()));
        let (project, core) = (base.join("project"), base.join("core"));
        let shadow = script(&project, "dom/shared", "exit 0", 0o755);
        script(&core, "dom/shared", "exit 0", 0o755);
        let only_core = script(&core, "dom/b/deep", "exit 0", 0o755);
        script(&core, "dom/plain", "exit 0", 0o644); // not executable
        let got = find(&[project, base.join("absent"), core]);
        assert_eq!(
            got,
            vec![
                Postcheck { way_id: "dom/b/deep".into(), script: only_core },
                Postcheck { way_id: "dom/shared".into(), script: shadow },
            ]
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn requests_are_the_postchecks_that_exit_zero_on_the_payload() {
        let base = std::env::temp_dir().join(format!("ways-postcheck-run-{}", std::process::id()));
        let log = base.join("seen");
        let checks = vec![
            Postcheck { way_id: "a".into(), script: script(&base, "a", "grep -q Edit", 0o755) },
            Postcheck { way_id: "b".into(), script: script(&base, "b", "exit 1", 0o755) },
            // Reads no stdin and records what it was given.
            Postcheck {
                way_id: "c".into(),
                script: script(&base, "c", &format!("echo \"$CLAUDE_SESSION_ID $WAYS_SESSIONS_ROOT\" > {}", log.display()), 0o755),
            },
        ];
        let got = requests(&checks, r#"{"tool_name":"Edit"}"#, "sess-x");
        assert_eq!(got, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            format!("sess-x {}", session::sessions_root())
        );
        std::fs::remove_dir_all(&base).ok();
    }
}
