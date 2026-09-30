//! What the server can learn about the session it runs in, derived on every
//! call (ADR-187 item 5).

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// The launch flag that loads a development channel, and the value naming this
/// server (ADR-402).
const CHANNEL_FLAG: &str = "--dangerously-load-development-channels";
const CHANNEL_NAME: &str = "server:agent-ways";

fn sessions_dir() -> PathBuf {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude"));
    base.join("sessions")
}

/// The pid of the claude process whose session record carries `session_id`:
/// the records are named `<pid>.json`.
fn claude_pid(session_id: &str) -> Option<u32> {
    let needle = format!("\"sessionId\":\"{session_id}\"");
    std::fs::read_dir(sessions_dir()).ok()?.flatten().find_map(|e| {
        let text = std::fs::read_to_string(e.path()).ok()?;
        if !text.contains(&needle) {
            return None;
        }
        e.path().file_stem()?.to_str()?.parse().ok()
    })
}

/// Whether `argv` loads this server as a development channel. The flag takes
/// a comma-separated list, and `--flag=value` is accepted as well.
pub fn loads_channel(argv: &[String]) -> bool {
    let names = argv.iter().enumerate().find_map(|(i, a)| {
        if a == CHANNEL_FLAG {
            argv.get(i + 1).cloned()
        } else {
            a.strip_prefix(&format!("{CHANNEL_FLAG}=")).map(str::to_string)
        }
    });
    names.is_some_and(|v| v.split(',').any(|n| n.trim() == CHANNEL_NAME))
}

/// The command line of `pid`, split on whitespace. `ps` works on Linux and
/// macOS alike; an argument containing spaces is split, which the flag check
/// tolerates.
fn argv_of(pid: u32) -> Option<Vec<String>> {
    let out = Command::new("ps").args(["-o", "args=", "-p", &pid.to_string()]).output().ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout).split_whitespace().map(str::to_string).collect()
    })
}

/// The session this server belongs to, and which inbound conduits it has.
pub fn describe() -> Value {
    let ident = attend_session::identity_for_pid(std::process::id());
    let pid = ident.session_resolved.then(|| claude_pid(&ident.session_id)).flatten();
    let channel = pid.and_then(argv_of).map(|argv| loads_channel(&argv));
    json!({
        "session_id": ident.session_resolved.then_some(ident.session_id),
        "origin_path": ident.origin_path,
        "claude_pid": pid,
        "channel_loaded": channel,
    })
}

#[cfg(test)]
mod tests {
    use super::loads_channel;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn detects_the_channel_flag_in_its_forms() {
        assert!(loads_channel(&argv("claude --dangerously-load-development-channels server:agent-ways")));
        assert!(loads_channel(&argv("claude --dangerously-load-development-channels=server:agent-ways")));
        assert!(loads_channel(&argv("claude --dangerously-load-development-channels server:other,server:agent-ways")));
    }

    #[test]
    fn other_channels_and_no_flag_do_not_count() {
        assert!(!loads_channel(&argv("claude")));
        assert!(!loads_channel(&argv("claude --dangerously-load-development-channels server:other")));
        assert!(!loads_channel(&argv("claude --dangerously-load-development-channels")));
        assert!(!loads_channel(&argv("claude server:agent-ways")));
    }
}
