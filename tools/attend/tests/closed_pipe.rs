//! A closed stdout ends attend quietly, as it ends any Unix filter:
//! `attend keepwarm status | head -5` must not panic once `head` exits.
//! The read end of the pipe is closed before attend starts, so its first
//! write meets EPIPE every time.

#![cfg(unix)]

mod common;
use common::Fixture;
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;

/// SIGPIPE is 13 on every Unix attend builds for.
const SIGPIPE: i32 = 13;

fn into_closed_pipe(f: &Fixture, args: &[&str]) {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = f.command().args(args).stdout(writer).stderr(Stdio::piped()).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "attend {args:?} panicked on a closed pipe: {stderr}");
    assert!(!stderr.contains("Broken pipe"), "attend {args:?} reported the closed pipe: {stderr}");
    assert_ne!(out.status.code(), Some(101), "attend {args:?} exited with a panic's status");
    // The default disposition ends the process by the signal itself.
    assert_eq!(out.status.signal(), Some(SIGPIPE), "attend {args:?}: {:?}: {stderr}", out.status);
}

#[test]
fn the_banner_into_a_closed_pipe_ends_quietly() {
    let f = Fixture::new("closed-pipe-banner");
    into_closed_pipe(&f, &[]);
}

/// The reported case was `keepwarm status`, which needs a `ways` to read
/// the transcript; `keepwarm off` prints through the same `println!`.
#[cfg(feature = "sensor-keepwarm")]
#[test]
fn a_keepwarm_verb_into_a_closed_pipe_ends_quietly() {
    let f = Fixture::new("closed-pipe-keepwarm");
    into_closed_pipe(&f, &["keepwarm", "off"]);
}
