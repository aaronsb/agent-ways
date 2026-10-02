//! The terminal guard on a pty that hangs up under it. The guard runs in a
//! child of this test binary whose stdin, stdout and stderr are the pty's
//! slave; the parent closes the master and every handle it has on the
//! slave, so the child's terminal and its stderr are both gone, then lets
//! the child drop its guard. A drop that prints panics on the dead stderr.

use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::term::TermGuard;

fn wait_for(p: &std::path::Path) {
    let start = Instant::now();
    while !p.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "{} never appeared", p.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "run by dropping_the_guard_on_a_hung_up_terminal_neither_prints_nor_panics"]
fn child_drops_its_guard_after_a_hangup() {
    let Some(dir) = std::env::var_os("AGENT_TUI_GUARD_CHILD").map(PathBuf::from) else { return };
    let mut guard = TermGuard::new();
    // A frame hides the cursor, which ratatui's own drop would show again.
    guard.term.draw(|_| {}).unwrap();
    std::fs::write(dir.join("ready"), "").unwrap();
    wait_for(&dir.join("go"));
    drop(guard);
    std::fs::write(dir.join("dropped"), "").unwrap();
}

#[test]
fn dropping_the_guard_on_a_hung_up_terminal_neither_prints_nor_panics() {
    let dir = std::env::temp_dir().join(format!("agent-tui-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills two descriptors we then own; the master is kept
    // from the child so closing ours hangs the terminal up.
    let (master, slave) = unsafe {
        assert_eq!(libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws), 0);
        libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC);
        (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s))
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["app::term_tests::child_drops_its_guard_after_a_hangup", "--exact", "--ignored", "--test-threads=1", "--nocapture"])
        .env("AGENT_TUI_GUARD_CHILD", &dir)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()))
        .spawn()
        .unwrap();
    // Drain the master until the guard is up, so the child never blocks on output.
    let mut reader = std::fs::File::from(master.try_clone().unwrap());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let halt = stop.clone();
    let drain = std::thread::spawn(move || {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        let mut b = [0u8; 4096];
        while !halt.load(std::sync::atomic::Ordering::SeqCst) {
            let mut pfd = libc::pollfd { fd: reader.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd we own.
            if unsafe { libc::poll(&mut pfd, 1, 20) } > 0 && matches!(reader.read(&mut b), Ok(0) | Err(_)) {
                break;
            }
        }
    });
    wait_for(&dir.join("ready"));
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    drain.join().unwrap();
    drop(master);
    drop(slave);
    std::fs::write(dir.join("go"), "").unwrap();
    let status = child.wait().unwrap();
    let dropped = dir.join("dropped").exists();
    let _ = std::fs::remove_dir_all(&dir);
    // The test harness itself then fails printing its result to the dead
    // terminal, so the mark the child writes after the drop is the verdict.
    assert!(dropped, "the guard's drop panicked on a hung-up terminal: {status:?}");
}
