//! Signal wire format, paths, and I/O for `attend-chat`.
//!
//! The TUI is a first-class endpoint on the signal bus — it reads and
//! writes the same `.signal` files the CLI does. Duplicating the
//! handful of lines it takes to do that is cheaper than extracting a
//! shared crate for a single new caller; if a third writer shows up
//! later we lift this into a common place.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
#[allow(dead_code)] // `id`/`cwd`/`reply_to`/`ts` land when the sidebar and
                   // threading UI ship in follow-up ADR-120 PRs.
pub struct Signal {
    pub id: String,
    pub from: String,
    pub project: String,
    pub cwd: String,
    pub reply_to: Option<String>,
    pub message: String,
    pub ts: u64,
}

pub fn signals_base() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".cache").join("attend").join("signals")
}

pub fn broadcast_dir() -> PathBuf {
    signals_base().join("_broadcast")
}

/// Encode a cwd path into the signal directory name the peer sensor
/// scans. Must stay byte-identical to `attend::util::encode_project`
/// (same `/`, `_`, `.` → `-` transform). See the mirror note on
/// `write_broadcast` — same contract, different layer.
pub fn encode_cwd(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

/// Directory that delivers signals to the claude session rooted at
/// `cwd`. The peer sensor scans `signals_base/<encoded>/` for
/// messages addressed specifically to it.
pub fn cwd_dir(cwd: &str) -> PathBuf {
    signals_base().join(encode_cwd(cwd))
}

/// Parse a `.signal` file. Supports both the legacy
/// `from|project|cwd|message` format and the threaded
/// `from|project|cwd|re:signal-id|message` extension. Returns `None`
/// for anything that doesn't look like a signal we can render.
pub fn parse_file(path: &Path) -> Option<Signal> {
    if path.extension().and_then(|s| s.to_str()) != Some("signal") {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    let line = raw.trim_end_matches('\n');
    // splitn(5, '|') so message can contain pipes.
    let parts: Vec<&str> = line.splitn(5, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let (reply_to, message) = if parts.len() == 5 && parts[3].starts_with("re:") {
        (Some(parts[3][3..].to_string()), parts[4].to_string())
    } else if parts.len() == 5 {
        // Unexpected 5-field form without re: — treat as legacy body with a pipe.
        (None, format!("{}|{}", parts[3], parts[4]))
    } else {
        (None, parts[3].to_string())
    };

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    let ts = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Some(Signal {
        id,
        from: parts[0].to_string(),
        project: parts[1].to_string(),
        cwd: parts[2].to_string(),
        reply_to,
        message,
        ts,
    })
}

/// Write a broadcast signal to `_broadcast/` using the atomic
/// tmp+rename pattern `cmd_send` uses, so readers (including our own
/// watcher) never see a half-written file.
///
/// **Wire-format mirror.** The line format here must stay byte-
/// identical to the legacy branch of `cmd_send` in
/// `tools/attend/src/cmd/send.rs`. If you change one, change both —
/// we intentionally didn't extract a shared crate while there are
/// only two writers, so drift is a per-PR review concern rather than
/// a compile error. Threaded replies (`re:<id>`) are produced by
/// `cmd_send` only; the TUI does not originate threaded sends yet.
pub fn write_broadcast(message: &str) -> io::Result<String> {
    write_signal(&broadcast_dir(), message)
}

/// Write a signal into an arbitrary destination directory. The
/// broadcast and directed (`@Nickname`) paths both ride this — same
/// wire format, same atomic tmp+rename, only the target differs.
pub fn write_signal(dest: &Path, message: &str) -> io::Result<String> {
    fs::create_dir_all(dest)?;

    let (sender_id, from, project, cwd) = sender_identity();

    // Build the filename stem (== signal id that `re:<id>` replies
    // reference). `signal_filename` normalizes the sender id — the TUI's
    // sender is `$USER@<terminal>`, whose raw `@`/`.` would fail
    // `is_valid_signal_id` and break `attend reply` auto-threading
    // (issue #368) — and makes the name collision-proof. `from` keeps the
    // raw id.
    let filename = agent_identity::signal_filename(&sender_id);
    let content = format!("{}|{}|{}|{}\n", from, project, cwd, message);

    let tmp = dest.join(format!("{}.tmp", filename));
    let final_path = dest.join(&filename);
    fs::write(&tmp, content)?;
    fs::rename(&tmp, &final_path)?;
    Ok(filename)
}

/// Compose the in-memory `Signal` for a message THIS session just sent,
/// so the sender's own transcript can echo it.
///
/// Why this exists: a broadcast rides `_broadcast/`, which the sender's
/// own watcher surfaces (`watcher::accept_path`), so it self-echoes for
/// free. A directed (`@name`) send is written to the *recipient's* cwd
/// inbox, which the sender does not watch — so without this it would
/// vanish from the sender's view even though it was delivered. This
/// builds the same identity fields the wire signal carries (`from`,
/// `project`, `cwd`) so the echoed row renders with the sender's chip,
/// identical to how their own broadcast already appears. It writes
/// nothing — echo is a display concern, not a bus event.
pub fn compose_self_echo(message: &str) -> Signal {
    let (_sender_id, from, project, cwd) = sender_identity();
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Signal {
        // Not a bus id — this row never came off disk. Marked so it is
        // never mistaken for a real signal stem should that ever matter.
        id: format!("local-echo-{ts}"),
        from,
        project,
        cwd,
        reply_to: None,
        message: message.to_string(),
        ts,
    }
}

/// Compose a local-only transcript status block (`/peers` output and
/// kin). Rendered like any message cell but never written to the bus.
/// `from` carries no wire prefix on purpose: `known_identities` skips
/// unknown prefixes, so a status block can't pollute the legend or
/// `@`-completion, and the chip falls through to the raw-value branch
/// (`attend` / `<kind>`) — visually distinct from every real sender.
pub fn compose_status_block(kind: &str, body: &str) -> Signal {
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Signal {
        id: format!("local-status-{ts}"),
        from: "attend".to_string(),
        project: kind.to_string(),
        cwd: String::new(),
        reply_to: None,
        message: body.to_string(),
        ts,
    }
}

/// Derive this session's wire identity fields once: `(sender_id, from,
/// project, cwd)`. Shared by `write_signal` (the delivered signal) and
/// `compose_self_echo` (the local echo) so the echoed row's chip is
/// always derived identically to the delivered signal's — if this logic
/// changes, both move together instead of drifting.
///
/// `project` is the last non-empty cwd segment, or `"?"` when the cwd is
/// empty (e.g. `current_dir()` failed). Note `"".rsplit('/').next()`
/// yields `Some("")`, not `None`, so a naive `.next().unwrap_or("?")`
/// would render a blank chip — `find(non-empty)` is deliberate.
fn sender_identity() -> (String, String, String, String) {
    let (sender_id, kind) = identify_sender();
    let from = format!("{}:{}", kind, sender_id);
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = cwd
        .rsplit('/')
        .find(|seg| !seg.is_empty())
        .unwrap_or("?")
        .to_string();
    (sender_id, from, project, cwd)
}

/// Focus-group membership identity for the human at the keyboard
/// (ADR-170): the sanitized username alone, no terminal suffix. One
/// entry per human regardless of terminal or cwd — the same dedupe
/// rule the chip registry applies to external senders. This is the
/// member id written into `_groups.yaml` by `/join` and the heartbeat
/// key the TUI touches while running.
pub fn human_member_id() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    agent_identity::sanitize_id_component(&user)
}

/// Identify the human at the keyboard. attend-chat is almost always
/// running outside a Claude session (it's the human's coordination
/// surface), so we skip the Claude-session detection the CLI does and
/// go straight to `$USER@<terminal>`.
fn identify_sender() -> (String, &'static str) {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let term = detect_terminal();
    let id = if term.is_empty() {
        user
    } else {
        format!("{}@{}", user, term)
    };
    (id, "external")
}

fn detect_terminal() -> String {
    if std::env::var("KITTY_PID").is_ok() {
        return "kitty".into();
    }
    if std::env::var("ALACRITTY_SOCKET").is_ok() {
        return "alacritty".into();
    }
    if std::env::var("WEZTERM_PANE").is_ok() {
        return "wezterm".into();
    }
    if std::env::var("TMUX").is_ok() {
        return "tmux".into();
    }
    if std::env::var("STY").is_ok() {
        return "screen".into();
    }
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        return tp.to_lowercase();
    }
    if std::env::var("SSH_CONNECTION").is_ok() {
        return "ssh".into();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_signal(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(format!("{}.signal", name));
        let mut f = fs::File::create(&p).unwrap();
        writeln!(f, "{}", body).unwrap();
        p
    }

    #[test]
    fn parses_legacy_format() {
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-1", "claude:abc|proj|/home/x|hello world");
        let s = parse_file(&p).unwrap();
        assert_eq!(s.from, "claude:abc");
        assert_eq!(s.project, "proj");
        assert_eq!(s.cwd, "/home/x");
        assert_eq!(s.message, "hello world");
        assert!(s.reply_to.is_none());
    }

    #[test]
    fn parses_threaded_format() {
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-2", "claude:abc|proj|/home/x|re:abc123|reply body");
        let s = parse_file(&p).unwrap();
        assert_eq!(s.reply_to.as_deref(), Some("abc123"));
        assert_eq!(s.message, "reply body");
    }

    #[test]
    fn preserves_pipes_in_legacy_body() {
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-3", "claude:abc|proj|/home/x|a | b");
        let s = parse_file(&p).unwrap();
        assert_eq!(s.message, "a | b");
        assert!(s.reply_to.is_none());
    }

    #[test]
    fn write_then_parse_roundtrip() {
        // Point $HOME at a temp dir so broadcast_dir() resolves there
        // instead of the real cache, then assert the writer's output
        // is accepted by our own parser. Guards against silent wire-
        // format drift between the TUI's send path and its watcher.
        let home = tempdir_like();
        // `set_var` is !Send on some platforms but this test is
        // single-threaded; Cargo isolates by default.
        std::env::set_var("HOME", &home);

        let filename = write_broadcast("round-trip body").unwrap();
        let path = broadcast_dir().join(&filename);
        let sig = parse_file(&path).expect("written signal must parse");
        assert_eq!(sig.message, "round-trip body");
        assert!(sig.from.starts_with("external:"));
        assert!(sig.reply_to.is_none());
    }

    #[test]
    fn compose_self_echo_carries_message_and_self_identity() {
        // The echo must render as coming from THIS session (so it shows
        // the sender's own chip) and carry the message verbatim. It is a
        // display object, never written to disk.
        let echo = compose_self_echo("hello @peer");
        assert_eq!(echo.message, "hello @peer");
        assert!(
            echo.from.starts_with("claude:") || echo.from.starts_with("external:"),
            "echo.from should be a real sender identity, got {:?}",
            echo.from
        );
        assert!(echo.reply_to.is_none());
        assert!(echo.id.starts_with("local-echo-"), "echo id marks it non-bus");
    }

    #[test]
    fn sender_id_env_precedence() {
        // Several pieces of state here are process-global (env vars),
        // so we run the whole precedence lattice inside one test and
        // reset between cases instead of relying on cargo's parallel
        // runner to serialise us.
        let original: Vec<(&str, Option<String>)> = [
            "USER",
            "LOGNAME",
            "KITTY_PID",
            "ALACRITTY_SOCKET",
            "WEZTERM_PANE",
            "TMUX",
            "STY",
            "TERM_PROGRAM",
            "SSH_CONNECTION",
            "TERMINAL",
        ]
        .iter()
        .map(|k| (*k, std::env::var(*k).ok()))
        .collect();

        let clear_all = || {
            for (k, _) in &original {
                std::env::remove_var(k);
            }
        };

        // Kitty wins over TERM_PROGRAM when both are set.
        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("KITTY_PID", "123");
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        let (id, kind) = identify_sender();
        assert_eq!(kind, "external");
        assert_eq!(id, "tester@kitty");

        // TERM_PROGRAM is the fallback when no specific-terminal env
        // is set, and it's lowercased.
        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TERM_PROGRAM", "iTerm.app");
        let (id, _) = identify_sender();
        assert_eq!(id, "tester@iterm.app");

        // TMUX beats TERM_PROGRAM (multiplexer wins over host
        // terminal emulator).
        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TMUX", "/tmp/tmux-0/default,123,0");
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        let (id, _) = identify_sender();
        assert_eq!(id, "tester@tmux");

        // SSH is the final fallback before TERMINAL / bare user.
        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22");
        let (id, _) = identify_sender();
        assert_eq!(id, "tester@ssh");

        // No terminal identifiers → bare user.
        clear_all();
        std::env::set_var("USER", "tester");
        let (id, _) = identify_sender();
        assert_eq!(id, "tester");

        // LOGNAME fills in when USER is missing.
        clear_all();
        std::env::set_var("LOGNAME", "backup_name");
        let (id, _) = identify_sender();
        assert_eq!(id, "backup_name");

        // Restore prior environment so we don't pollute sibling tests.
        clear_all();
        for (k, v) in original {
            if let Some(v) = v {
                std::env::set_var(k, v);
            }
        }
    }

    #[test]
    fn encode_cwd_matches_attend_convention() {
        // Same transform as attend::util::encode_project — `/`, `_`,
        // `.` collapse to `-`. This is the path the peer sensor
        // scans when looking for messages directed at a specific
        // cwd, so drift here breaks direct routing silently.
        assert_eq!(encode_cwd("/home/aaron/.claude"), "-home-aaron--claude");
        assert_eq!(encode_cwd("/tmp/foo_bar"), "-tmp-foo-bar");
        assert_eq!(encode_cwd(""), "");
    }

    #[test]
    fn cwd_dir_is_under_signals_base() {
        // Point $HOME at a temp dir so signals_base resolves there.
        let home = tempdir_like();
        std::env::set_var("HOME", &home);
        let d = cwd_dir("/home/aaron/proj");
        assert!(
            d.starts_with(signals_base()),
            "cwd_dir {d:?} should live under {:?}",
            signals_base()
        );
        assert_eq!(
            d.file_name().and_then(|s| s.to_str()),
            Some("-home-aaron-proj")
        );
    }

    fn tempdir_like() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "attend-chat-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
}
