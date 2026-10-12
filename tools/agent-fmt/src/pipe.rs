//! A closed stdout ends the process quietly.
//!
//! Rust ignores SIGPIPE, so a write to a closed pipe returns EPIPE, and
//! `println!` panics on it: `attend keepwarm status | head -5` printed
//! "failed printing to stdout: Broken pipe" once `head` exited. The default
//! disposition ends the process by the signal, as every other Unix filter
//! does.

/// Restores the default SIGPIPE disposition. Call it first in `main`, before
/// any thread starts.
///
/// A process that writes to sockets or child pipes it must outlive, such as
/// a server answering clients that may hang up, keeps Rust's default and
/// handles EPIPE as an error instead.
pub fn exit_quietly_on_closed_pipe() {
    #[cfg(unix)]
    // SAFETY: setting a signal disposition before any thread starts.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}
