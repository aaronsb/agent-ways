//! What the server can learn about the session it runs in, derived on every
//! call (ADR-187 item 5).

use serde_json::{json, Value};

/// The launch flag that loads development channels, and the value naming this
/// server (ADR-402). The flag takes one or more values, space-separated.
const CHANNEL_FLAG: &str = "--dangerously-load-development-channels";
const CHANNEL_NAME: &str = "server:agent-ways";

/// Whether `argv` names this server under the development-channels flag. This
/// reads the launch command only: organization policy or the protocol revision
/// can still keep the channel from loading.
pub fn has_channel_flag(argv: &[String]) -> bool {
    let mut values: Vec<&str> = Vec::new();
    let mut in_flag = false;
    for a in argv {
        if a == CHANNEL_FLAG {
            in_flag = true;
        } else if let Some(v) = a.strip_prefix(&format!("{CHANNEL_FLAG}=")) {
            values.push(v);
            in_flag = false;
        } else if a.starts_with('-') {
            in_flag = false;
        } else if in_flag {
            values.push(a);
        }
    }
    values.iter().flat_map(|v| v.split(',')).any(|n| n.trim() == CHANNEL_NAME)
}

/// The session this server belongs to, and whether it was launched with this
/// server as a development channel.
pub fn describe() -> Value {
    match attend_presence::session::find_own_session(std::process::id()) {
        Some((sid, pid)) => json!({
            "origin_path": attend_presence::session::origin_path(&sid),
            "session_id": sid,
            "claude_pid": pid,
            "channel_flag": attend_presence::process::argv(pid).map(|argv| has_channel_flag(&argv)),
        }),
        None => json!({ "session_id": null, "claude_pid": null, "channel_flag": null, "origin_path": null }),
    }
}

#[cfg(test)]
mod tests {
    use super::has_channel_flag;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn detects_the_flag_in_its_forms() {
        assert!(has_channel_flag(&argv("claude --dangerously-load-development-channels server:agent-ways")));
        assert!(has_channel_flag(&argv("claude --dangerously-load-development-channels=server:agent-ways")));
        assert!(has_channel_flag(&argv(
            "claude --dangerously-load-development-channels server:webhook server:agent-ways --model opus"
        )));
        assert!(has_channel_flag(&argv("claude --dangerously-load-development-channels server:webhook,server:agent-ways")));
    }

    #[test]
    fn other_servers_and_later_arguments_do_not_count() {
        assert!(!has_channel_flag(&argv("claude")));
        assert!(!has_channel_flag(&argv("claude --dangerously-load-development-channels server:webhook")));
        assert!(!has_channel_flag(&argv("claude --dangerously-load-development-channels")));
        assert!(!has_channel_flag(&argv("claude server:agent-ways")));
        // A value after another option belongs to that option.
        assert!(!has_channel_flag(&argv(
            "claude --dangerously-load-development-channels server:webhook --resume server:agent-ways"
        )));
    }
}
