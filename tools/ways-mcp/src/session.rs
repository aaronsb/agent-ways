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

/// The session a lookup tool acts for: its id and, when Claude Code's session
/// record names one, the working directory it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    /// The session's own working directory, worktree included. `ways_status`
    /// reports the normalized origin; ways resolve against where the session
    /// actually works, so a project way in a managed worktree is found.
    pub project: Option<String>,
}

/// Who this server serves, derived by process ancestry on every call (ADR-171,
/// ADR-187 item 5). `None` when no ancestor is a Claude Code session, as under
/// a bare shell: the tools then act without a session and say so.
pub fn identity() -> Option<Identity> {
    let records = claude_sessions::read_session_records(&claude_sessions::ClaudeDir::user().sessions_dir());
    identity_in(&records, std::process::id(), attend_presence::process::parent_pid, std::env::var("CLAUDE_PROJECT_DIR").ok())
}

/// [`identity`] over the records already read: the nearest ancestor of
/// `own_pid` (up to 15 levels) that owns a record is the session. The project
/// is what a hook would use: `CLAUDE_PROJECT_DIR`, else the session's own `cwd`
/// as its record has it, worktree and all.
fn identity_in(
    records: &[claude_sessions::SessionRecord],
    own_pid: u32,
    parent_of: impl Fn(u32) -> Option<u32>,
    env_project: Option<String>,
) -> Option<Identity> {
    let mut pid = own_pid;
    for _ in 0..15 {
        if let Some(r) = records.iter().find(|r| r.pid == pid) {
            let project = env_project.filter(|p| !p.is_empty()).or_else(|| r.cwd.clone());
            return Some(Identity { id: r.session_id.clone(), project });
        }
        match parent_of(pid) {
            Some(parent) if parent != pid && parent > 0 => pid = parent,
            _ => break,
        }
    }
    None
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use claude_sessions::SessionRecord;
    use std::collections::HashMap;

    fn record(pid: u32, id: &str, cwd: Option<&str>) -> SessionRecord {
        SessionRecord { pid, session_id: id.into(), cwd: cwd.map(Into::into), path: Default::default() }
    }

    #[test]
    fn the_session_is_the_nearest_ancestor_with_a_record_and_keeps_its_raw_cwd() {
        let records = [record(10, "outer", Some("/a")), record(20, "inner", Some("/work/.claude/worktrees/w1"))];
        let parents: HashMap<u32, u32> = [(100, 90), (90, 20), (20, 10)].into();
        let who = identity_in(&records, 100, |p| parents.get(&p).copied(), None).unwrap();
        assert_eq!(who.id, "inner");
        assert_eq!(who.project.as_deref(), Some("/work/.claude/worktrees/w1"), "the worktree is where the session works, as the hook payload's cwd says");
    }

    #[test]
    fn no_ancestor_with_a_record_is_no_session() {
        let records = [record(10, "s", None)];
        assert_eq!(identity_in(&records, 100, |p| (p == 100).then_some(50), None), None);
        assert_eq!(identity_in(&[], 100, |_| None, None), None);
    }

    #[test]
    fn the_project_follows_hooks_environment_first_then_the_session_cwd() {
        let records = [record(10, "s", Some("/from/record"))];
        let up = |p: u32| (p == 100).then_some(10);
        assert_eq!(identity_in(&records, 100, up, Some("/from/env".into())).unwrap().project.as_deref(), Some("/from/env"));
        assert_eq!(identity_in(&records, 100, up, Some(String::new())).unwrap().project.as_deref(), Some("/from/record"), "an empty variable is unset");
        let bare = [record(10, "s", None)];
        assert_eq!(identity_in(&bare, 100, up, None).unwrap().project, None);
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
