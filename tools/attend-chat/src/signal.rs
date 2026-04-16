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
    let dir = broadcast_dir();
    fs::create_dir_all(&dir)?;

    let (sender_id, kind) = identify_sender();
    let from = format!("{}:{}", kind, sender_id);
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = cwd.rsplit('/').next().unwrap_or("?").to_string();
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let filename = format!("{}-{}.signal", sender_id.replace('/', "-"), ts);
    let content = format!("{}|{}|{}|{}\n", from, project, cwd, message);

    let tmp = dir.join(format!("{}.tmp", filename));
    let final_path = dir.join(&filename);
    fs::write(&tmp, content)?;
    fs::rename(&tmp, &final_path)?;
    Ok(filename)
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
