//! The post-tool scan: reactive firing through each way's `postcheck.sh`
//! (ADR-123 Decision 5), on PostToolUse and PostToolUseFailure.
//!
//! A way requests firing when its postcheck, fed the hook payload on stdin,
//! exits 0. The requests then pass the same gate predictive firing uses
//! (`show::way_scored`), so a reactive way does not fire during its refire
//! window, and they share one context budget.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cmd::show::{self, ContextBudget};
use crate::session;

/// A way's postcheck, by way id.
#[derive(Debug, PartialEq, Eq)]
pub struct Postcheck {
    pub way_id: String,
    pub script: PathBuf,
    /// Whether it may run. A project-local postcheck is project code, so it
    /// runs only for a trusted project, as a project-local macro does; an
    /// untrusted one still shadows the same id in later roots.
    pub runs: bool,
}

/// A ways root and whether its postchecks may run.
pub struct Root {
    pub dir: PathBuf,
    pub runs: bool,
}

/// The ways roots in precedence order (ADR-143): project, user, core. The
/// project root's postchecks run only when the project is listed in
/// `paths::trusted_project_macros()`, the gate project macros pass.
pub fn roots(project_dir: &str) -> Vec<Root> {
    vec![
        Root { dir: Path::new(project_dir).join(".claude/ways"), runs: show::is_project_trusted(project_dir) },
        Root { dir: crate::paths::user_ways_root(), runs: true },
        Root { dir: crate::paths::projected_ways_root(), runs: true },
    ]
}

/// Every executable `postcheck.sh` under `roots`, one per way id: a way in an
/// earlier root shadows the same id in a later one, as it does for the way
/// itself. Sorted by way id, so the fire order does not follow walk order.
pub fn find(roots: &[Root]) -> Vec<Postcheck> {
    let mut found: std::collections::BTreeMap<String, (PathBuf, bool)> = Default::default();
    for Root { dir: root, runs } in roots.iter().filter(|r| r.dir.is_dir()) {
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
            found.entry(id).or_insert((script, *runs));
        }
    }
    found.into_iter().map(|(way_id, (script, runs))| Postcheck { way_id, script, runs }).collect()
}

/// How long the postchecks of one hook call may run, together. A postcheck
/// still running then is killed and counts as no request; those that
/// finished keep their answers.
pub const DEADLINE: Duration = Duration::from_secs(3);

/// Run every postcheck that may run, at once, with the payload on stdin; the
/// ids of those that exit 0 within [`DEADLINE`], in the order given. A
/// postcheck sees `CLAUDE_SESSION_ID` and `WAYS_SESSIONS_ROOT`; its output is
/// discarded.
pub fn requests(checks: &[Postcheck], payload: &str, session_id: &str) -> Vec<String> {
    requests_within(checks, payload, session_id, DEADLINE)
}

fn requests_within(checks: &[Postcheck], payload: &str, session_id: &str, deadline: Duration) -> Vec<String> {
    let root = session::sessions_root();
    let payload: Arc<str> = Arc::from(payload);
    let started = Instant::now();
    let mut running: Vec<(usize, Child)> = Vec::new();
    for (i, c) in checks.iter().enumerate().filter(|(_, c)| c.runs) {
        let Some(mut child) = spawn(&c.script, session_id, &root) else { continue };
        if let Some(mut stdin) = child.stdin.take() {
            // Written from its own thread: a payload larger than the pipe
            // buffer would otherwise block on a postcheck that never reads.
            // A postcheck that exits or is killed closes the pipe, and the
            // write ends with an error, which is not one.
            let payload = Arc::clone(&payload);
            std::thread::spawn(move || {
                let _ = stdin.write_all(payload.as_bytes());
            });
        }
        running.push((i, child));
    }
    let mut fired = vec![false; checks.len()];
    while !running.is_empty() {
        running.retain_mut(|(i, child)| match child.try_wait() {
            Ok(Some(status)) => {
                fired[*i] = status.success();
                false
            }
            Ok(None) => true,
            Err(_) => false,
        });
        if running.is_empty() {
            break;
        }
        if started.elapsed() >= deadline {
            for (_, child) in &mut running {
                kill(child);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    checks.iter().zip(fired).filter(|(_, f)| *f).map(|(c, _)| c.way_id.clone()).collect()
}

fn spawn(script: &Path, session_id: &str, root: &str) -> Option<Child> {
    // Unix runs the script by its shebang, in its own process group so a
    // deadline kill reaches what it started; Windows has no shebang, so bash.
    #[cfg(unix)]
    let mut cmd = {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new(script);
        c.process_group(0);
        c
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = Command::new("bash");
        c.arg(script);
        c
    };
    cmd.env("CLAUDE_SESSION_ID", session_id)
        .env("WAYS_SESSIONS_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Kill a postcheck past the deadline, its process group first on Unix, and
/// reap it.
fn kill(child: &mut Child) {
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

/// The post-tool scan for one hook call: the context it injects, empty when
/// nothing fired. A way that fails on the fire path (a malformed file) is
/// skipped; the other requests still fire.
pub fn scan(payload: &str, session_id: &str, project_dir: &str) -> String {
    let checks = find(&roots(project_dir));
    let mut context = String::new();
    let mut budget = ContextBudget::hook();
    for id in requests(&checks, payload, session_id) {
        let Ok(out) = show::way_scored(&id, session_id, "postcheck", None, None, None, Some(&mut budget)) else {
            continue;
        };
        if !out.is_empty() {
            context.push_str(&out);
            context.push_str("\n\n");
            budget.charge("\n\n");
        }
    }
    context.trim_end().to_string()
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
        let root = |dir: PathBuf, runs| Root { dir, runs };
        let got = find(&[root(project, false), root(base.join("absent"), true), root(core, true)]);
        assert_eq!(
            got,
            vec![
                Postcheck { way_id: "dom/b/deep".into(), script: only_core, runs: true },
                // An untrusted project's postcheck shadows, and does not run.
                Postcheck { way_id: "dom/shared".into(), script: shadow, runs: false },
            ]
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn requests_are_the_postchecks_that_exit_zero_on_the_payload() {
        let base = std::env::temp_dir().join(format!("ways-postcheck-run-{}", std::process::id()));
        let log = base.join("seen");
        let checks = vec![
            Postcheck { way_id: "a".into(), script: script(&base, "a", "grep -q Edit", 0o755), runs: true },
            Postcheck { way_id: "b".into(), script: script(&base, "b", "exit 1", 0o755), runs: true },
            Postcheck { way_id: "d".into(), script: script(&base, "d", &format!("touch {}", base.join("d-ran").display()), 0o755), runs: false },
            // Reads no stdin and records what it was given.
            Postcheck {
                way_id: "c".into(),
                script: script(&base, "c", &format!("echo \"$CLAUDE_SESSION_ID $WAYS_SESSIONS_ROOT\" > {}", log.display()), 0o755),
                runs: true,
            },
        ];
        let got = requests(&checks, r#"{"tool_name":"Edit"}"#, "sess-x");
        assert_eq!(got, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            format!("sess-x {}", session::sessions_root())
        );
        assert!(!base.join("d-ran").exists(), "a postcheck that may not run is never spawned");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_hung_postcheck_is_killed_at_the_deadline_and_the_rest_answer() {
        let base = std::env::temp_dir().join(format!("ways-postcheck-hang-{}", std::process::id()));
        let checks = vec![
            Postcheck { way_id: "fast".into(), script: script(&base, "fast", "exit 0", 0o755), runs: true },
            Postcheck { way_id: "hung".into(), script: script(&base, "hung", "sleep 8; exit 0", 0o755), runs: true },
            // Never reads stdin; a 1 MB payload must not block the writer.
            Postcheck { way_id: "deaf".into(), script: script(&base, "deaf", "sleep 0.2; exit 0", 0o755), runs: true },
        ];
        let big = "x".repeat(1 << 20);
        let t = Instant::now();
        let got = requests_within(&checks, &big, "s", Duration::from_millis(800));
        assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
        assert_eq!(got, vec!["fast".to_string(), "deaf".to_string()]);
        std::fs::remove_dir_all(&base).ok();
    }
}
