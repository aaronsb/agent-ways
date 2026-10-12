//! `write_to_child` under the default SIGPIPE, as the suite's binaries run.
//! This binary holds one test, so setting the default here reaches nothing
//! else.

#![cfg(unix)]

use std::process::{Command, Stdio};

#[test]
fn a_child_that_exits_unread_gives_an_error_and_the_signal_is_taken() {
    agent_fmt::exit_quietly_on_closed_pipe();
    let mut child = Command::new("sh").args(["-c", "exit 0"]).stdin(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    child.wait().unwrap();
    // More than a pipe buffer, on this thread: a bare write_all ends the
    // test binary by the signal.
    let err = agent_fmt::write_to_child(&mut stdin, &vec![b'x'; 1 << 20]).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    // Nothing is left pending: a later write in the same thread with the
    // signal unblocked would otherwise deliver it now.
    let err = agent_fmt::write_to_child(&mut stdin, b"again").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
}
