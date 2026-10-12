//! Closed pipes: a closed stdout ends the process quietly, and a child that
//! closes its stdin does not.
//!
//! Rust ignores SIGPIPE, so a write to a closed pipe returns EPIPE, and
//! `println!` panics on it: `attend keepwarm status | head -5` printed
//! "failed printing to stdout: Broken pipe" once `head` exited. The default
//! disposition ends the process by the signal, as every other Unix filter
//! does.
//!
//! Under the default, a write to a child's stdin pipe raises the same signal
//! when the child exits without reading, and the signal ends this process.
//! [`write_to_child`] makes that write return EPIPE instead. Sockets are not
//! the hazard: std sends on them with MSG_NOSIGNAL, or sets SO_NOSIGPIPE.

use std::io::{self, Write};

/// Restores the default SIGPIPE disposition. Call it first in `main`, before
/// any thread starts. Every write to a child's stdin then goes through
/// [`write_to_child`].
pub fn exit_quietly_on_closed_pipe() {
    #[cfg(unix)]
    // SAFETY: setting a signal disposition before any thread starts.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Writes all of `bytes` to a child's stdin. A child that exited without
/// reading gives an EPIPE error here, whatever the SIGPIPE disposition.
///
/// SIGPIPE is blocked in the calling thread for the write. The kernel sends
/// it to the writing thread, so it stays pending there, and it is taken off
/// with `sigwait` before the thread's mask is put back.
#[cfg(unix)]
pub fn write_to_child<W: Write + ?Sized>(pipe: &mut W, bytes: &[u8]) -> io::Result<()> {
    // SAFETY: sigset calls on zeroed sets this function owns; the thread's
    // mask is restored to what it was on every path.
    unsafe {
        let mut sigpipe: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut sigpipe);
        libc::sigaddset(&mut sigpipe, libc::SIGPIPE);
        let mut before: libc::sigset_t = std::mem::zeroed();
        libc::pthread_sigmask(libc::SIG_BLOCK, &sigpipe, &mut before);
        let result = pipe.write_all(bytes);
        // A thread that already blocked SIGPIPE keeps what is pending.
        if libc::sigismember(&before, libc::SIGPIPE) == 0 {
            let mut pending: libc::sigset_t = std::mem::zeroed();
            libc::sigpending(&mut pending);
            if libc::sigismember(&pending, libc::SIGPIPE) == 1 {
                let mut taken = 0;
                libc::sigwait(&sigpipe, &mut taken);
            }
            libc::pthread_sigmask(libc::SIG_SETMASK, &before, std::ptr::null_mut());
        }
        result
    }
}

/// Windows has no SIGPIPE: a closed pipe is already an error.
#[cfg(not(unix))]
pub fn write_to_child<W: Write + ?Sized>(pipe: &mut W, bytes: &[u8]) -> io::Result<()> {
    pipe.write_all(bytes)
}
