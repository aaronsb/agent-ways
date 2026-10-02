//! The settings screens on a real terminal: a pty, the real binary, and a
//! termination signal while an apply waits on a slow command. The terminal
//! must come back as the shell left it (main screen, cursor shown, mouse
//! reporting off, line mode and echo on), the command must be ended, and
//! the process must exit with 128 plus the signal.

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A pty of `cols` by `rows`: (master, slave).
fn openpty(cols: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills two descriptors we then own.
    let r = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) };
    assert_eq!(r, 0, "openpty: {}", std::io::Error::last_os_error());
    // SAFETY: both are fresh descriptors from openpty.
    unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

/// Wait until `buf` holds `needle`, or fail after `secs` naming what it holds.
fn wait_for(buf: &Arc<Mutex<Vec<u8>>>, needle: &str, secs: u64) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if String::from_utf8_lossy(&buf.lock().unwrap()).contains(needle) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("`{needle}` never appeared:\n{}", String::from_utf8_lossy(&buf.lock().unwrap()));
}

fn esc(seq: &str) -> String {
    format!("{}{seq}", '\u{1b}')
}

#[test]
fn a_signal_during_an_apply_restores_the_terminal_and_ends_the_command() {
    let root = std::env::temp_dir().join(format!("ways-settings-pty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    for d in [".config", "proj", ".claude", ".local/share", ".local/state", ".cache"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    // A stand-in for the binary the queued command runs: it notes its pid,
    // then waits far longer than the test does.
    let runner = root.join("runner.sh");
    let pidfile = root.join("runner.pid");
    std::fs::write(&runner, format!("#!/bin/sh\necho $$ > {}\nexec sleep 60\n", pidfile.display())).unwrap();
    std::fs::set_permissions(&runner, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let (master, slave) = openpty(100, 30);
    let bin: PathBuf = env!("CARGO_BIN_EXE_ways").into();
    let mut cmd = Command::new(bin);
    cmd.args(["settings", "install"])
        .current_dir(home.join("proj"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("CLAUDE_PROJECT_DIR", home.join("proj"))
        .env("WAYS_SETTINGS_RUNNER", &runner)
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    // SAFETY: setsid and TIOCSCTTY are async-signal-safe; the pty becomes
    // the child's controlling terminal, as a shell's would be.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();

    let out = Arc::new(Mutex::new(Vec::new()));
    let mut reader = std::fs::File::from(master.try_clone().unwrap());
    let sink = out.clone();
    std::thread::spawn(move || {
        let mut b = [0u8; 4096];
        while let Ok(n) = reader.read(&mut b) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&b[..n]);
        }
    });
    let mut keys = std::fs::File::from(master.try_clone().unwrap());
    let mut press = |k: &str| {
        keys.write_all(k.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
    };

    wait_for(&out, "targets", 10);
    // The targets row's menu, plan, a directory, then review and apply.
    for k in ["a", "j", "j", "\r", "/tmp/x", "\r", "w", "a"] {
        press(k);
    }
    wait_for(&out, "applying", 10);
    let start = Instant::now();
    while !pidfile.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let runner_pid: i32 = std::fs::read_to_string(&pidfile).expect("the command started").trim().parse().unwrap();

    // SAFETY: a signal to our own child.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("the screens did not end on SIGTERM:\n{}", String::from_utf8_lossy(&out.lock().unwrap()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(status.code(), Some(128 + libc::SIGTERM), "exit is 128 plus the signal");

    let text = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
    let after = &text[text.rfind(&esc("[?1049h")).expect("the screens took the alternate screen")..];
    for (seq, what) in [("[?1049l", "the main screen"), ("[?25h", "the cursor"), ("[?1000l", "mouse reporting off"), ("[?1003l", "motion reporting off")] {
        assert!(after.contains(&esc(seq)), "{what} is not restored after the signal: {after:?}");
    }
    // SAFETY: termios is plain data the call fills.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut t) }, 0);
    assert!(t.c_lflag & libc::ICANON != 0 && t.c_lflag & libc::ECHO != 0 && t.c_lflag & libc::ISIG != 0, "the pty is left in raw mode");
    // SAFETY: signal 0 only asks whether the process exists.
    assert_ne!(unsafe { libc::kill(runner_pid, 0) }, 0, "the command in flight was ended, not left running");
    let _ = std::fs::remove_dir_all(&root);
}
