// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Every forced exit runs the hook the application set
//! (`agent_tui::on_forced_exit`) while the run loop is wedged: the watch
//! thread's limit before the loop takes a signal up, its limit after, and
//! a second SIGTERM. Each case runs in a child of this test binary whose
//! main thread spins, as crossterm's read does on a terminal that hung up,
//! and the parent reads the file the hook wrote.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn keep() {
    if let Some(path) = std::env::var_os("AGENT_TUI_FORCED_EXIT_MARK") {
        let _ = std::fs::write(path, "kept");
    }
}

#[test]
#[ignore = "run by every_forced_exit_runs_the_hook"]
fn child_wedged_after_a_signal() {
    let Ok(case) = std::env::var("AGENT_TUI_FORCED_EXIT_CASE") else { return };
    let signals = agent_tui::Signals::install().expect("signals");
    assert!(agent_tui::on_forced_exit(keep));
    // SAFETY: a signal to this process, whose handlers are installed.
    let raise = |sig| unsafe { libc::kill(libc::getpid(), sig) };
    match case.as_str() {
        "unseen" => raise(libc::SIGHUP),
        "seen" => {
            raise(libc::SIGTERM);
            signals.take_up();
            0
        }
        "second" => {
            raise(libc::SIGTERM);
            raise(libc::SIGTERM)
        }
        _ => unreachable!(),
    };
    // The wedge: the loop never reads the signal, nor ends.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        std::hint::spin_loop();
    }
}

fn run(case: &str) -> (Option<i32>, bool, Duration) {
    let mark = std::env::temp_dir().join(format!("agent-tui-forced-exit-{case}-{}", std::process::id()));
    let _ = std::fs::remove_file(&mark);
    let start = Instant::now();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child_wedged_after_a_signal", "--exact", "--ignored", "--test-threads=1"])
        .env("AGENT_TUI_FORCED_EXIT_CASE", case)
        .env("AGENT_TUI_FORCED_EXIT_MARK", &mark)
        .output()
        .unwrap();
    let took = start.elapsed();
    let kept = std::fs::read_to_string(&mark).is_ok_and(|s| s == "kept");
    let _ = std::fs::remove_file(PathBuf::from(&mark));
    (out.status.code(), kept, took)
}

#[test]
fn every_forced_exit_runs_the_hook() {
    let (code, kept, took) = run("unseen");
    assert_eq!(code, Some(128 + libc::SIGHUP), "the UNSEEN limit exits with the signal");
    assert!(kept, "the UNSEEN limit ran the hook");
    assert!(took < Duration::from_secs(10), "{took:?}");

    let (code, kept, took) = run("seen");
    assert_eq!(code, Some(128 + libc::SIGTERM), "the SEEN limit exits with the signal");
    assert!(kept, "the SEEN limit ran the hook");
    assert!(took >= Duration::from_secs(3), "the SEEN limit waits its three seconds: {took:?}");

    let (code, kept, took) = run("second");
    assert_eq!(code, Some(128 + libc::SIGTERM), "a second SIGTERM forces the exit");
    assert!(kept, "the second SIGTERM ran the hook");
    assert!(took < Duration::from_secs(3), "a second SIGTERM does not wait for a limit: {took:?}");
}
