//! `ways judge-setup` on a real terminal (ADR-708 §9): a pty, the real binary
//! and stand-ins for `claude` and `ways-agent`, so the menu it offers, what it
//! leaves unsaid, and what a choice writes are read as the operator meets
//! them. The binary is copied into the fixture so the `ways-agent` it finds
//! beside itself is the stand-in, not the workspace's build.

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `ways-agent key add` stores a key file and records a valid check of it
/// against the anthropic profile's model, as the real one does after the
/// provider accepts the key.
const AGENT: &str = r#"#!/bin/sh
case "$1 $2" in
  "key add")
    d="$XDG_CONFIG_HOME/agent-ways/keys"; mkdir -p "$d"; chmod 700 "$d"
    printf 'sk-ant-test-abcdefghijklmnop\n' > "$d/$4"; chmod 600 "$d/$4"
    s="$XDG_STATE_HOME/agent-ways/agent"; mkdir -p "$s"
    set -- $(stat -c '%Y %s' "$d/$4") "$4"
    printf '{"result":"valid","model":"claude-haiku-4-5","at":0,"source":"%s","stamp":[%s,%s]}\n' "$d/$3" "$1" "$2" > "$s/key-check-$3.json" ;;
esac
"#;

/// `claude` at the version in $STUB_VERSION, with no plugins.
const CLAUDE: &str = r#"#!/bin/sh
case "$*" in
  "--version") echo "$STUB_VERSION (Claude Code)" ;;
  *"list --json") echo '[]' ;;
  *) exit 2 ;;
esac
"#;

struct Fx {
    root: PathBuf,
}

impl Fx {
    /// A home whose agent.yaml names `claude-code`, with no mod and no key.
    /// `judge` ships the ways-judge mod in the app; `agent` puts the
    /// stand-in `ways-agent` beside `ways`.
    fn new(name: &str, judge: bool, agent: bool) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-judge-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        for d in [".config/agent-ways", ".local/state", ".cache", ".claude", "proj"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::fs::write(home.join(".config/agent-ways/agent.yaml"), "engine: claude-code\n").unwrap();
        let app = home.join(".local/share/agent-ways/plugins");
        write(&app.join(".claude-plugin/marketplace.json"), "{}");
        if judge {
            write(&app.join("ways-judge/.claude-plugin/plugin.json"), r#"{"version":"1.42.1"}"#);
        }
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_ways"), root.join("bin/ways")).unwrap();
        if agent {
            exe(&root.join("bin/ways-agent"), AGENT);
        }
        exe(&root.join("stub/claude"), CLAUDE);
        Fx { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// The user config, where the ways-judge mod's wanted state lives.
    fn user_config(&self) -> PathBuf {
        self.home().join(".config/agent-ways/config.yaml")
    }

    /// The mod wanted, as `gate.engine: claude-code` wants it (ADR-512 item 4).
    fn want_judge(&self) {
        let text = std::fs::read_to_string(self.user_config()).unwrap_or_default();
        write(&self.user_config(), &format!("{text}mods:\n  ways-judge:\n    install: true\n"));
    }

    fn judge_wanted(&self) -> bool {
        std::fs::read_to_string(self.user_config()).unwrap_or_default().contains("ways-judge:\n    install: true")
    }

    fn agent_yaml(&self) -> String {
        std::fs::read_to_string(self.home().join(".config/agent-ways/agent.yaml")).unwrap()
    }

    /// Runs `ways judge-setup` on a pty with Claude Code at `version`,
    /// typing `answer` once `prompt` shows (no typing when `prompt` is None),
    /// and returns what the terminal showed.
    fn setup(&self, version: &str, prompt: Option<&str>, answer: &str) -> String {
        let home = self.home();
        let (master, slave) = openpty();
        let mut cmd = Command::new(self.root.join("bin/ways"));
        cmd.arg("judge-setup")
            .current_dir(home.join("proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TERM", "xterm-256color")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("WAYS_CLAUDE_BIN", self.root.join("stub/claude"))
            .env("STUB_VERSION", version)
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
        // A binary written a moment ago can still be open for writing in a
        // child another test thread forked and has not yet exec'd: the
        // kernel refuses to run it (ETXTBSY) until that child execs.
        let mut tries = 0;
        let mut child = loop {
            match cmd.spawn() {
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < 50 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                r => break r.unwrap(),
            }
        };
        drop(slave);
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
        if let Some(prompt) = prompt {
            let start = Instant::now();
            while !text(&out).contains(prompt) {
                assert!(start.elapsed() < Duration::from_secs(10), "`{prompt}` never appeared:\n{}", text(&out));
                std::thread::sleep(Duration::from_millis(50));
            }
            std::fs::File::from(master.try_clone().unwrap()).write_all(answer.as_bytes()).unwrap();
        }
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "judge-setup did not exit:\n{}", text(&out));
            std::thread::sleep(Duration::from_millis(50));
        };
        // Let the reader drain what the child wrote before it exited.
        std::thread::sleep(Duration::from_millis(200));
        let shown = text(&out);
        drop(out);
        let _ = reader.join();
        assert!(status.success(), "{shown}");
        shown.replace("\r\n", "\n")
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn openpty() -> (OwnedFd, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize { ws_row: 30, ws_col: 100, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills two descriptors we then own.
    let r = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) };
    assert_eq!(r, 0, "openpty: {}", std::io::Error::last_os_error());
    // SAFETY: setting a flag on a descriptor we own.
    unsafe { libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: both are fresh descriptors from openpty.
    unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn exe(p: &Path, text: &str) {
    write(p, text);
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Review of #1142, findings 2 and 5: a Claude Code too old for the mod's
/// API gets three choices and is told why; picking Anthropic, once its key
/// is stored and checked, names the anthropic engine, and then the mod is no
/// longer wanted (ADR-512 item 4).
#[test]
fn an_old_claude_code_gets_three_choices_and_a_key_choice_names_its_engine() {
    let fx = Fx::new("old-host", true, true);
    fx.want_judge();
    let shown = fx.setup("2.1.296", Some("Choice [1-3"), "1\n");
    assert!(!fx.judge_wanted(), "a key engine chosen: the mod is no longer wanted");
    assert!(std::fs::read_to_string(fx.user_config()).unwrap().contains("ways-judge:\n    install: false"));
    // The line wraps at 80 columns.
    for line in shown.lines() {
        assert!(line.chars().count() <= 80, "{line:?}");
    }
    let flat = shown.replace('\n', " ");
    assert!(flat.contains("Claude Code internal is not offered: Claude Code 2.1.296 is older than 2.1.304"), "{shown}");
    assert!(shown.contains("[1] An Anthropic API key") && !shown.contains("[4]"), "{shown}");
    assert!(fx.agent_yaml().contains("engine: anthropic"), "{}", fx.agent_yaml());
    assert!(shown.contains("Relevance judge on (Anthropic)."), "{shown}");
}

/// Re-review of #1142: choosing no judge records gate.mode off, clears the
/// claude-code engine and stops wanting the mod.
#[test]
fn choosing_no_judge_turns_the_gate_off_and_stops_wanting_the_mod() {
    let fx = Fx::new("no-judge", true, true);
    fx.want_judge();
    let shown = fx.setup("2.1.296", Some("Choice [1-3"), "3\n");
    assert!(fx.agent_yaml().contains("mode: off"), "{}", fx.agent_yaml());
    assert!(!fx.agent_yaml().contains("engine: claude-code"), "{}", fx.agent_yaml());
    assert!(!fx.judge_wanted());
    assert!(shown.replace('\n', " ").contains("Relevance judge off by choice"), "{shown}");
}

/// Review of #1142, round 4: a key engine chosen while a target is disabled
/// stops wanting the mod in the user config, so enabling that target again
/// does not install it.
#[test]
fn a_key_engine_stops_wanting_the_mod_for_a_disabled_target_too() {
    let fx = Fx::new("disabled-target", true, true);
    let home = fx.home();
    std::fs::create_dir_all(home.join(".claude-b")).unwrap();
    write(
        &fx.user_config(),
        &format!(
            "targets:\n  - path: {}\n    enabled: false\n  - path: {}\n",
            home.join(".claude").display(),
            home.join(".claude-b").display()
        ),
    );
    fx.want_judge();
    fx.setup("2.1.296", Some("Choice [1-3"), "1\n");
    assert!(fx.agent_yaml().contains("engine: anthropic"), "{}", fx.agent_yaml());
    assert!(!fx.judge_wanted(), "one setting covers every target, a disabled one included");
}

/// Finding 2: a Claude Code whose mods can complete and spawn is offered the
/// mod first, of four.
#[test]
fn a_new_enough_claude_code_is_offered_the_mod_first() {
    let fx = Fx::new("new-host", true, true);
    let shown = fx.setup("2.1.309", Some("Choice [1-4"), "s\n");
    assert!(shown.contains("[1] Claude Code internal"), "{shown}");
    assert!(!shown.contains("not offered"), "{shown}");
    assert!(shown.contains("Skipped."), "{shown}");
    assert_eq!(fx.agent_yaml(), "engine: claude-code\n", "a skip writes nothing");
}

/// Finding 3: an app that does not ship the mod says nothing about it, and
/// with no ways-agent to add a key the only entry left, "No judge", is no
/// menu at all.
#[test]
fn an_app_without_the_mod_says_nothing_of_it_and_a_lone_no_judge_is_no_menu() {
    let fx = Fx::new("unshipped", false, false);
    let shown = fx.setup("2.1.309", None, "");
    assert!(shown.contains("Ways is degraded"), "{shown}");
    assert!(!shown.contains("not offered"), "{shown}");
    assert!(!shown.contains("Choose the relevance judge"), "{shown}");
}
