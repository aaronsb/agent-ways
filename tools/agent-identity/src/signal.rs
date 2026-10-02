//! The ADR-120 signal wire format: one line, `from|project|cwd|message`, or
//! `from|project|cwd|re:<signal-id>|message` for a threaded reply.
//!
//! Lives beside [`crate::signal_filename`], which mints the ids a reply
//! references, so every reader (the attend CLI, the attend-chat TUI, the
//! peers sensor) applies one rule for what counts as threaded, and both
//! writers (`attend send`, attend-chat) produce the same line, the same
//! sender identity and the same atomic file.

use std::io;
use std::path::Path;

/// A parsed signal line. Legacy signals have no `reply_to`; threaded replies
/// carry the original signal's id there. Borrows from the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignal<'a> {
    pub from: &'a str,
    pub project: &'a str,
    pub cwd: &'a str,
    pub reply_to: Option<&'a str>,
    pub message: &'a str,
}

/// Signal ids are filename stems in the form `<sender-id>-<timestamp>`,
/// which is always `[A-Za-z0-9_-]+`. This char class is the discriminator
/// fence that keeps legacy prose starting with "re:" from being misparsed as
/// threaded: `attend send "re: the thing we discussed|still open"` stays a
/// legacy message because `the thing we discussed` has a space.
pub fn is_valid_signal_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a single-line signal. The discriminator is a `re:<id>|` prefix on
/// the field that follows `cwd`, where `<id>` passes [`is_valid_signal_id`].
/// A malformed or ambiguous `re:` prefix degrades to the legacy reading, so
/// real prose round-trips intact. Pipes in the message are kept.
pub fn parse_signal(content: &str) -> Option<ParsedSignal<'_>> {
    let parts: Vec<&str> = content.splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let tail = parts[3];
    let (reply_to, message) = match tail.strip_prefix("re:").and_then(|rest| rest.split_once('|')) {
        Some((id, msg)) if is_valid_signal_id(id) => (Some(id), msg),
        _ => (None, tail),
    };
    Some(ParsedSignal {
        from: parts[0],
        project: parts[1],
        cwd: parts[2],
        reply_to,
        message,
    })
}

/// One wire line, newline-terminated: `from|project|cwd|message`, or
/// `from|project|cwd|re:<id>|message` when `reply_to` is given.
pub fn format_signal(from: &str, project: &str, cwd: &str, reply_to: Option<&str>, message: &str) -> String {
    match reply_to {
        Some(id) => format!("{from}|{project}|{cwd}|re:{id}|{message}\n"),
        None => format!("{from}|{project}|{cwd}|{message}\n"),
    }
}

/// The `project` field for a sender at `cwd`: its last non-empty path
/// segment, or `?` when there is none.
pub fn project_label(cwd: &str) -> String {
    cwd.rsplit('/').find(|seg| !seg.is_empty()).unwrap_or("?").to_string()
}

/// Write `content` into `dest` as `filename`, creating `dest` first. The
/// line goes to `<filename>.tmp` and is renamed into place, so a reader
/// never sees a half-written signal.
pub fn write_signal_file(dest: &Path, filename: &str, content: &str) -> io::Result<()> {
    std::fs::create_dir_all(dest)?;
    let tmp = dest.join(format!("{filename}.tmp"));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, dest.join(filename)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The login name: `USER`, else `LOGNAME`, else `unknown`.
pub fn user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// The terminal a person is typing in, from the environment: a specific
/// emulator or multiplexer first, then `TERM_PROGRAM` (lowercased), an SSH
/// session, and `TERMINAL`'s basename. Empty when nothing names one.
pub fn detect_terminal() -> String {
    for (var, name) in [
        ("KITTY_PID", "kitty"),
        ("ALACRITTY_SOCKET", "alacritty"),
        ("WEZTERM_PANE", "wezterm"),
        ("TMUX", "tmux"),
        ("STY", "screen"),
    ] {
        if std::env::var_os(var).is_some() {
            return name.to_string();
        }
    }
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        return tp.to_lowercase();
    }
    if std::env::var_os("SSH_CONNECTION").is_some() {
        return "ssh".to_string();
    }
    if let Ok(t) = std::env::var("TERMINAL") {
        return t.rsplit('/').next().unwrap_or(&t).to_string();
    }
    String::new()
}

/// Who is sending, as `(sender_id, kind)`: `(session_id, "claude")` inside
/// a Claude session, else `(user@terminal, "external")`, or the bare user
/// when no terminal is known. The wire `from` is `kind:sender_id`.
pub fn identify_sender(session_id: Option<String>) -> (String, &'static str) {
    if let Some(sid) = session_id {
        return (sid, "claude");
    }
    let user = user_name();
    let terminal = detect_terminal();
    if terminal.is_empty() {
        (user, "external")
    } else {
        (format!("{user}@{terminal}"), "external")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_parse_round_trip() {
        let line = format_signal("claude:a", "p", "/x/p", Some("id-1"), "a | b");
        assert_eq!(line, "claude:a|p|/x/p|re:id-1|a | b\n");
        let s = parse_signal(line.trim()).unwrap();
        assert_eq!((s.reply_to, s.message), (Some("id-1"), "a | b"));
        assert_eq!(format_signal("claude:a", "p", "/x/p", None, "hi"), "claude:a|p|/x/p|hi\n");
    }

    #[test]
    fn project_label_is_the_last_non_empty_segment() {
        assert_eq!(project_label("/home/u/proj"), "proj");
        assert_eq!(project_label("/home/u/proj/"), "proj");
        assert_eq!(project_label(""), "?");
        assert_eq!(project_label("/"), "?");
    }

    #[test]
    fn a_session_id_makes_a_claude_sender() {
        assert_eq!(identify_sender(Some("sess".into())), ("sess".to_string(), "claude"));
    }

    #[test]
    fn external_sender_env_precedence() {
        // Env vars are process-global, so the whole precedence lattice
        // runs inside one test, resetting between cases.
        let keys = [
            "USER", "LOGNAME", "KITTY_PID", "ALACRITTY_SOCKET", "WEZTERM_PANE", "TMUX", "STY",
            "TERM_PROGRAM", "SSH_CONNECTION", "TERMINAL",
        ];
        let original: Vec<(&str, Option<std::ffi::OsString>)> =
            keys.iter().map(|k| (*k, std::env::var_os(k))).collect();
        let clear_all = || keys.iter().for_each(|k| std::env::remove_var(k));
        let id = || identify_sender(None);

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("KITTY_PID", "123");
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        assert_eq!(id(), ("tester@kitty".to_string(), "external"), "a specific emulator wins");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TERM_PROGRAM", "iTerm.app");
        assert_eq!(id().0, "tester@iterm.app", "TERM_PROGRAM is lowercased");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TMUX", "/tmp/tmux-0/default,123,0");
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        assert_eq!(id().0, "tester@tmux", "a multiplexer beats the host emulator");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22");
        assert_eq!(id().0, "tester@ssh");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TERMINAL", "/usr/bin/foot");
        assert_eq!(id().0, "tester@foot", "TERMINAL's basename is the last resort");

        clear_all();
        std::env::set_var("USER", "tester");
        assert_eq!(id().0, "tester");

        clear_all();
        std::env::set_var("LOGNAME", "backup_name");
        assert_eq!(id().0, "backup_name");

        clear_all();
        for (k, v) in original {
            if let Some(v) = v {
                std::env::set_var(k, v);
            }
        }
    }

    #[test]
    fn written_signal_lands_whole_under_its_name() {
        let dir = std::env::temp_dir().join(format!("agent-identity-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_signal_file(&dir.join("tray"), "x-1-0.signal", "a|b|c|d\n").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("tray").join("x-1-0.signal")).unwrap(), "a|b|c|d\n");
        assert!(!dir.join("tray").join("x-1-0.signal.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn threaded_reply_carries_its_id() {
        let s = parse_signal("claude:a|p|/x|re:sender-123-0|reply body").unwrap();
        assert_eq!(s.reply_to, Some("sender-123-0"));
        assert_eq!(s.message, "reply body");
    }

    #[test]
    fn prose_starting_re_stays_legacy() {
        let s = parse_signal("claude:a|p|/x|re: prose|x").unwrap();
        assert_eq!(s.reply_to, None);
        assert_eq!(s.message, "re: prose|x");
    }

    #[test]
    fn legacy_body_keeps_its_pipes() {
        let s = parse_signal("claude:a|p|/x|a | b").unwrap();
        assert_eq!((s.reply_to, s.message), (None, "a | b"));
        assert!(parse_signal("claude:a|p|/x").is_none());
        assert!(parse_signal("").is_none());
    }

    #[test]
    fn a_malformed_re_prefix_keeps_the_raw_tail() {
        for line in ["claude:a|p|/x|re:alone", "claude:a|p|/x|re:|body", "claude:a|p|/x|re:has spaces|body"] {
            let s = parse_signal(line).unwrap();
            assert_eq!(s.reply_to, None, "{line}");
            assert_eq!(s.message, line.splitn(4, '|').nth(3).unwrap(), "{line}");
        }
    }
}
