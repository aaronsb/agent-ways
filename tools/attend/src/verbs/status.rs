//! `status`: every running `attend run` with the session it serves,
//! pending signal counts, keepwarm and channel state.

use crate::util::{count_signals, current_session, groups_for, Session};
use crate::verbs::peers::JoinedChannel;
use serde::Serialize;
use std::path::PathBuf;

/// An `attend run` process on this machine. `ps` lists every one, one per
/// session that runs attend, so each names the session it serves.
#[derive(Debug, Clone, Serialize)]
pub struct Instance {
    pub pid: u32,
    /// The session this run serves, from its Claude Code session record;
    /// `None` when no record names it.
    pub session_id: Option<String>,
    /// That session's display name. With no origin readable, the session
    /// id's first eight characters stand in rather than a guessed name.
    pub name: Option<String>,
    /// The run serves the session asking.
    pub this_session: bool,
    /// Another run serves the same session. The session lock (ADR-129)
    /// allows one, so a second is worth flagging.
    pub duplicate: bool,
    /// No session record names the run and it was reparented to init or a
    /// subreaper: a leftover run, safe to stop. A missing record alone
    /// proves nothing, so an unowned run with a live parent stays false.
    pub reparented: bool,
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
    /// What waits for this session: `N unread`, or `N waiting · cold` while
    /// its prompt cache has lapsed under an addressed message.
    pub unread: String,
    /// Keepwarm's one-line state (ADR-182).
    pub keepwarm: String,
    pub channels: Vec<JoinedChannel>,
    pub cache: PathBuf,
}

pub fn status() -> Status {
    let me = current_session();
    let groups = groups_for(&me);
    Status {
        instances: running_instances(&me),
        signals: pending(&me, &groups),
        unread: crate::verbs::unread::headline(&crate::verbs::unread::unread()),
        keepwarm: keepwarm_line(&me),
        channels: groups.my_groups().into_iter().map(|(name, pinned)| JoinedChannel { name, pinned }).collect(),
        cache: attend_presence::cache::dir(),
    }
}

/// Every `attend run` in the process table but this process, each with the
/// session it serves.
fn running_instances(me: &Session) -> Vec<Instance> {
    let Ok(out) = std::process::Command::new("ps").args(["-ww", "-eo", "pid=,args="]).output() else {
        return Vec::new();
    };
    let own_pid = std::process::id();
    let pids: Vec<u32> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, args) = line.trim().split_once(char::is_whitespace)?;
            let pid = pid.parse::<u32>().ok()?;
            (pid != own_pid && is_attend_run(args)).then_some(pid)
        })
        .collect();
    let owners: Vec<Option<(String, String)>> = pids
        .iter()
        .map(|&pid| {
            let i = attend_presence::session::session_identity_for_pid(pid)?;
            // A name derives from the origin; with none readable, the
            // session id stands in rather than a guessed name.
            let name = if i.origin_path.is_empty() {
                format!("session {}", &i.session_id[..i.session_id.len().min(8)])
            } else {
                crate::util::display_name(&i)
            };
            Some((i.session_id, name))
        })
        .collect();
    pids.iter()
        .zip(&owners)
        .map(|(&pid, owner)| match owner {
            Some((sid, name)) => Instance {
                pid,
                session_id: Some(sid.clone()),
                name: Some(name.clone()),
                this_session: sid == &me.session_id,
                duplicate: owners.iter().flatten().filter(|(s, _)| s == sid).count() > 1,
                reparented: false,
            },
            None => Instance {
                pid,
                session_id: None,
                name: None,
                this_session: false,
                duplicate: false,
                reparented: reparented(pid),
            },
        })
        .collect()
}

/// Whether `args`, a `ps` command line, is an `attend run` process: the
/// program's basename is `attend` and its first argument is `run`. A shell
/// wrapper or an editor whose line merely mentions "attend run" is not.
fn is_attend_run(args: &str) -> bool {
    let mut words = args.split_whitespace();
    let program = words.next().unwrap_or("");
    std::path::Path::new(program).file_name().is_some_and(|n| n == "attend") && words.next() == Some("run")
}

/// Whether `pid` was orphaned: its parent is init or a subreaper (the
/// `systemd --user` manager), as happens when the session that started it
/// is gone.
fn reparented(pid: u32) -> bool {
    use attend_presence::process::{argv, parent_pid};
    match parent_pid(pid) {
        Some(1) => true,
        Some(ppid) => argv(ppid).and_then(|a| a.into_iter().next()).is_some_and(|p| {
            std::path::Path::new(&p).file_name().is_some_and(|n| n == "systemd" || n == "init")
        }),
        None => false,
    }
}

/// Keepwarm's state for session `me` in one line: armed with its time left,
/// stopped with the sensor's reason, or not armed (ADR-182). "Not armed",
/// never "off": `off` is a value of the attend mod's `mods.attend.keepwarm`
/// setting, which says whether the mod runs the check at all.
#[cfg(not(feature = "sensor-keepwarm"))]
fn keepwarm_line(_me: &Session) -> String {
    "not built".to_string()
}

#[cfg(feature = "sensor-keepwarm")]
fn keepwarm_line(me: &Session) -> String {
    use sensor_keepwarm::{fmt_duration, read_arm, read_ledger};
    if !me.session_resolved {
        return "no session".to_string();
    }
    let now = sensor_trait::epoch_secs();
    let dir = crate::util::keepwarm_dir();
    match read_arm(&dir, &me.session_id) {
        Some(a) if a.deadline > now => format!("armed, {} left", fmt_duration(a.deadline - now)),
        _ => match read_ledger(&dir, &me.session_id).and_then(|l| l.stopped) {
            Some(why) => format!("stopped, {why}"),
            None => "not armed".to_string(),
        },
    }
}

fn pending(me: &Session, groups: &crate::groups::Groups) -> Pending {
    let dirs = groups.receive_dirs(&me.origin_path);
    let count = |room: attend_groups::Room| -> usize {
        dirs.iter().filter(|d| d.room == room).map(|d| count_signals(&d.path)).sum()
    };
    Pending { project: count(attend_groups::Room::Project), open: count(attend_groups::Room::Open) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_attend_program_running_run_is_an_instance() {
        assert!(is_attend_run("attend run"));
        assert!(is_attend_run("/home/u/.local/bin/attend run --catchup"));
        assert!(!is_attend_run("/bin/zsh -c eval 'attend run'"));
        assert!(!is_attend_run("nvim notes-about-attend run"));
        assert!(!is_attend_run("attend-chat run"));
        assert!(!is_attend_run("attend peer status"));
        assert!(!is_attend_run(""));
    }

    /// A run whose parent is alive and is no subreaper is not leftover.
    #[test]
    fn a_run_with_a_live_parent_is_not_reparented() {
        assert!(!reparented(std::process::id()));
    }
}
