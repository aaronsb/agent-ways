//! A live replay following its session on a real terminal (#780): a pty,
//! the real binary and its real loop, so the ticks are the terminal loop's
//! own. `ways session live` opens following; events appended to the log
//! are read once its stat, on the backoff, sees the write, and the new
//! frame draws without a key pressed.

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ID: &str = "cccccccc-0000-4000-8000-000000000003";

fn openpty(cols: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills two descriptors we then own.
    let r = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) };
    assert_eq!(r, 0, "openpty: {}", std::io::Error::last_os_error());
    // SAFETY: setting a flag on a descriptor we own.
    unsafe { libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: both are fresh descriptors from openpty.
    unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

fn line(ts: &str, event: &str, way: &str, proj: &str) -> String {
    format!("{{\"ts\":\"{ts}\",\"event\":\"{event}\",\"session\":\"{ID}\",\"way\":\"{way}\",\"trigger\":\"keyword\",\"project\":\"{proj}\"}}\n")
}

/// The terminal output with its control sequences taken out: what was
/// drawn, in the order it was drawn.
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if it.next_if_eq(&'[').is_some() {
            for c in it.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

#[test]
fn a_live_replay_draws_appended_events_without_a_key() {
    let root = std::env::temp_dir().join(format!("ways-follow-pty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    for d in [".config", "proj/.git", ".local/state/agent-ways", ".cache", ".local/share"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    let proj = home.join("proj").display().to_string();
    let log: PathBuf = home.join(".local/state/agent-ways/events.jsonl");
    std::fs::write(&log, line("2026-07-02T10:00:00Z", "session_start", "", &proj) + &line("2026-07-02T10:00:01Z", "way_fired", "softwaredev/code/testing", &proj)).unwrap();
    // The log sat quiet an hour: the follow still starts at its floor, so
    // the session's next event is not left to the backoff's ceiling.
    let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options().write(true).open(&log).unwrap().set_modified(hour_ago).unwrap();

    let (master, slave) = openpty(100, 30);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    cmd.args(["session", "live", "--session", ID])
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
    let mut file = std::fs::File::from(master.try_clone().unwrap());
    let sink = out.clone();
    let reader = std::thread::spawn(move || {
        let mut b = [0u8; 4096];
        loop {
            let mut pfd = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd we own.
            let r = unsafe { libc::poll(&mut pfd, 1, 100) };
            if r < 0 {
                break;
            }
            if r == 0 {
                if Arc::strong_count(&sink) == 1 {
                    break;
                }
                continue;
            }
            match file.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => sink.lock().unwrap().extend_from_slice(&b[..n]),
            }
        }
    });
    let text = |o: &Arc<Mutex<Vec<u8>>>| String::from_utf8_lossy(&o.lock().unwrap()).into_owned();
    let wait_for = |needle: &str, secs: u64| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(secs) {
            if text(&out).contains(needle) {
                return start.elapsed();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("`{needle}` never appeared:\n{}", text(&out));
    };

    wait_for("LIVE", 10);
    assert!(!text(&out).contains("zz/appended"));
    // The session writes on: a way fires a minute later, a frame of its own.
    let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    f.write_all(line("2026-07-02T10:01:00Z", "way_fired", "zz/appended", &proj).as_bytes()).unwrap();
    drop(f);
    // Seen at the follow's next due stat: the floor, doubled at most twice.
    let took = wait_for("zz/appended", 20);
    // The cursor rides the newest way: its row is drawn selected.
    assert!(plain(&text(&out)).contains("▌zz/appended"), "the follow put the cursor on the new way: {}", text(&out));
    eprintln!("the appended frame drew {took:?} after the write");

    let mut m = std::fs::File::from(master.try_clone().unwrap());
    m.write_all(b"q").unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "q did not end the screen");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status:?}");
    drop((master, slave, m));
    drop(out);
    let _ = reader.join();
    let _ = std::fs::remove_dir_all(&root);
}
