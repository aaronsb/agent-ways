//! `status`: running sensor loops, pending signal counts, keepwarm and
//! channel state.

use crate::util::{count_signals, current_session, groups_for, Session};
use crate::verbs::peers::JoinedChannel;
use serde::Serialize;
use std::path::PathBuf;

/// An `attend run` process on this machine.
#[derive(Debug, Clone, Serialize)]
pub struct Instance {
    pub pid: String,
    pub command: String,
}

/// Signal files waiting in this session's trays. These are counts of what
/// is on disk, not of what this session has yet to see.
#[derive(Debug, Clone, Serialize)]
pub struct Pending {
    pub project: usize,
    pub open: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub instances: Vec<Instance>,
    pub signals: Pending,
    /// Keepwarm's one-line state (ADR-182).
    pub keepwarm: String,
    pub channels: Vec<JoinedChannel>,
    pub cache: PathBuf,
}

pub fn status() -> Status {
    let me = current_session();
    let groups = groups_for(&me);
    Status {
        instances: running_instances(),
        signals: pending(&me, &groups),
        keepwarm: crate::cmd::keepwarm::status_line(&me),
        channels: groups.my_groups().into_iter().map(|(name, pinned)| JoinedChannel { name, pinned }).collect(),
        cache: attend_presence::cache::dir(),
    }
}

/// Every `attend run` in the process table, but this process and the shell
/// wrappers that carry the words.
fn running_instances() -> Vec<Instance> {
    let Ok(out) = std::process::Command::new("ps").args(["-ww", "-eo", "pid=,args="]).output() else {
        return Vec::new();
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let own_pid = std::process::id().to_string();
    stdout
        .lines()
        .map(str::trim)
        // Only match actual attend binary, not shell wrappers that contain "attend run"
        .filter(|line| line.contains("attend run") && !line.contains(&own_pid))
        // Skip zsh/bash wrapper lines (contain shell-snapshots or eval)
        .filter(|line| !line.contains("shell-snapshots") && !line.contains("eval '"))
        .filter_map(|line| {
            let (pid, args) = line.split_once(char::is_whitespace)?;
            Some(Instance { pid: pid.trim().to_string(), command: args.trim().to_string() })
        })
        .collect()
}

fn pending(me: &Session, groups: &crate::groups::Groups) -> Pending {
    let dirs = groups.receive_dirs(&me.origin_path);
    let count = |room: attend_groups::Room| -> usize {
        dirs.iter().filter(|d| d.room == room).map(|d| count_signals(&d.path)).sum()
    };
    Pending { project: count(attend_groups::Room::Project), open: count(attend_groups::Room::Open) }
}
