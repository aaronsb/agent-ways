//! Signal wire format, paths, and I/O for `attend-chat`.
//!
//! The TUI is a first-class endpoint on the signal bus — it reads and
//! writes the same `.signal` files the CLI does. The line parser and the
//! filename are shared through agent-identity; the paths and I/O around
//! them are duplicated here.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
#[allow(dead_code)] // `id`/`cwd`/`reply_to` land when the sidebar and
                   // threading UI ship in follow-up ADR-120 PRs.
pub struct Signal {
    pub id: String,
    pub from: String,
    pub project: String,
    pub cwd: String,
    pub reply_to: Option<String>,
    pub message: String,
    pub ts: u64,
    /// Which channel this signal arrived on — derived from the inbox
    /// dir it was read from, never from the body (wire format
    /// unchanged). Drives the per-tab transcript filter (#393).
    pub channel: Channel,
}

/// Provenance of a signal, at inbox-dir granularity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Channel {
    /// `_broadcast/` — the base `#open` channel.
    Open,
    /// `@<name>/` — a named channel.
    Group(String),
    /// A cwd inbox (directed send) or a local self-echo.
    Direct,
    /// Local-only status blocks (`/peers` output) — never on the bus,
    /// rendered in every tab so command feedback can't hide.
    Info,
}

pub fn signals_base() -> PathBuf {
    attend_presence::cache::signals_dir()
}

pub fn broadcast_dir() -> PathBuf {
    signals_base().join(attend_groups::BROADCAST_DIR)
}

/// Encode a cwd path into the signal directory name the peer sensor
/// scans: `claude_sessions::attend_key`, the one name `attend` and the
/// sensor use too.
pub fn encode_cwd(path: &str) -> String {
    claude_sessions::attend_key(path)
}

/// Directory that delivers signals to the claude session rooted at
/// `cwd`. The peer sensor scans `signals_base/<encoded>/` for
/// messages addressed specifically to it.
pub fn cwd_dir(cwd: &str) -> PathBuf {
    signals_base().join(encode_cwd(cwd))
}

/// Parse a `.signal` file through the shared ADR-120 parser
/// (`agent_identity::parse_signal`), so a `re:` prefix that is not a signal
/// id reads as legacy prose here exactly as it does in the attend CLI.
/// Returns `None` for anything that doesn't look like a signal we can render.
pub fn parse_file(path: &Path) -> Option<Signal> {
    if path.extension().and_then(|s| s.to_str()) != Some("signal") {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    let parsed = agent_identity::parse_signal(raw.trim_end_matches('\n'))?;

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
        from: parsed.from.to_string(),
        project: parsed.project.to_string(),
        cwd: parsed.cwd.to_string(),
        reply_to: parsed.reply_to.map(str::to_string),
        message: parsed.message.to_string(),
        ts,
        channel: channel_for_path(path),
    })
}

/// Classify a signal file's channel by the inbox dir it sits in —
/// the same first-component logic `watcher::accept_path` admits by,
/// so everything the watcher streams gets a definite channel.
fn channel_for_path(path: &Path) -> Channel {
    let dir = path
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if dir == attend_groups::BROADCAST_DIR {
        Channel::Open
    } else if let Some(g) = dir.strip_prefix('@') {
        Channel::Group(g.to_string())
    } else {
        Channel::Direct
    }
}

/// Write a broadcast signal to `_broadcast/`. The line, the sender
/// identity and the atomic write are `agent_identity::signal`'s, shared
/// with `attend send`. Threaded replies (`re:<id>`) are produced by
/// `attend send` only; the TUI does not originate threaded sends yet.
pub fn write_broadcast(message: &str) -> io::Result<String> {
    write_signal(&broadcast_dir(), message)
}

/// Write a signal into an arbitrary destination directory. The
/// broadcast and directed (`@Nickname`) paths both ride this — same
/// wire format, same atomic tmp+rename, only the target differs.
pub fn write_signal(dest: &Path, message: &str) -> io::Result<String> {
    let (sender_id, from, project, cwd) = sender_identity();

    // Build the filename stem (== signal id that `re:<id>` replies
    // reference). `signal_filename` normalizes the sender id — the TUI's
    // sender is `$USER@<terminal>`, whose raw `@`/`.` would fail
    // `is_valid_signal_id` and break `attend reply` auto-threading
    // (issue #368) — and makes the name collision-proof. `from` keeps the
    // raw id.
    let filename = agent_identity::signal_filename(&sender_id);
    let content = agent_identity::signal::format_signal(&from, &project, &cwd, None, message);
    agent_identity::signal::write_signal_file(dest, &filename, &content)?;
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
        // Echoes exist only for directed sends the watcher can't
        // round-trip, so Direct is definitionally right.
        channel: Channel::Direct,
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
        channel: Channel::Info,
    }
}

/// Derive this session's wire identity fields once: `(sender_id, from,
/// project, cwd)`. Shared by `write_signal` (the delivered signal) and
/// `compose_self_echo` (the local echo) so the echoed row's chip is
/// always derived identically to the delivered signal's — if this logic
/// changes, both move together instead of drifting.
///
/// The sender is `agent_identity::signal::identify_sender` with no
/// session: attend-chat is the human's coordination surface, so it is
/// `$USER@<terminal>`. `project` is `agent_identity::signal::project_label`.
fn sender_identity() -> (String, String, String, String) {
    let (sender_id, kind) = agent_identity::signal::identify_sender(None);
    let from = format!("{}:{}", kind, sender_id);
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = agent_identity::signal::project_label(&cwd);
    (sender_id, from, project, cwd)
}

/// Focus-group membership identity for the human at the keyboard
/// (ADR-170): the sanitized username alone, no terminal suffix. One
/// entry per human regardless of terminal or cwd — the same dedupe
/// rule the chip registry applies to external senders. This is the
/// member id written into `_groups.yaml` by `/join` and the heartbeat
/// key the TUI touches while running.
pub fn human_member_id() -> String {
    agent_identity::sanitize_id_component(&agent_identity::signal::user_name())
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
    fn prose_starting_re_with_a_pipe_stays_legacy() {
        // `re: prose` is not a signal id (it has a space), so the attend CLI
        // reads this as a legacy message; the TUI must read it the same way.
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-4", "claude:abc|proj|/home/x|re: prose|x");
        let s = parse_file(&p).unwrap();
        assert!(s.reply_to.is_none());
        assert_eq!(s.message, "re: prose|x");
    }

    #[test]
    fn parse_classifies_channel_by_inbox_dir() {
        // Provenance drives the per-tab filter (#393): broadcast →
        // Open, `@g/` → Group, anything else (cwd inbox) → Direct.
        let d = tempdir_like();
        let bdir = d.join("_broadcast");
        let gdir = d.join("@deploy");
        let cdir = d.join("-home-me-proj");
        for dir in [&bdir, &gdir, &cdir] {
            fs::create_dir_all(dir).unwrap();
        }
        let body = "claude:abc|proj|/home/x|hi";
        assert_eq!(parse_file(&tmp_signal(&bdir, "s", body)).unwrap().channel, Channel::Open);
        assert_eq!(
            parse_file(&tmp_signal(&gdir, "s", body)).unwrap().channel,
            Channel::Group("deploy".into())
        );
        assert_eq!(parse_file(&tmp_signal(&cdir, "s", body)).unwrap().channel, Channel::Direct);
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
        // The attend cache follows XDG_CACHE_HOME first: point it into the
        // temp home too, or a runner's own setting sends the write to the
        // real cache.
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));

        let filename = write_broadcast("round-trip body").unwrap();
        let path = broadcast_dir().join(&filename);
        assert!(path.starts_with(&home), "the write stays in the temp home: {}", path.display());
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
    fn encode_cwd_is_the_attend_key() {
        // The name attend::util::encode_project and the peer sensor use
        // for a cwd's tray; drift here breaks direct routing silently.
        assert_eq!(encode_cwd("/srv/my proj"), "-srv-my-proj-bte5w6");
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
            Some("-home-aaron-proj-wjpr18")
        );
    }

    fn tempdir_like() -> std::path::PathBuf {
        crate::test_dir::unique("signal")
    }
}
