// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The servers a process started and has not ended, so the next sensor
//! host for the project can end those a killed process left behind.
//!
//! Each running server has one entry, a JSON file under
//! `$XDG_STATE_HOME/agent-ways/attend/mcp-servers/<project hash>/`: its
//! process group, the start time of its leader, and the pid and start time
//! of the process that started it, its owner. The owner removes the entry
//! when it ends the server. A process killed with SIGKILL runs no
//! destructor and leaves its entries, and its servers run on. A server
//! whose start times cannot be read gets no entry, since nothing could
//! later tell it from another process.
//!
//! [`sweep`] runs when a sensor host starts. It reads each process as
//! alive (it runs with the recorded start time), gone (no process has the
//! pid, or one with another start time, which is a reused pid), or unknown
//! (a process has the pid and its start time cannot be read). An entry
//! whose owner is alive belongs to a live host or command, such as another
//! session's host in the project, and an entry with anything unknown is
//! left as it is; nothing is sent to either. When the owner is gone and
//! the leader is alive, the group is the server's, and it is ended, TERM
//! then KILL. When the leader is gone, the entry is dropped and nothing is
//! sent: the group may be empty and its id free, and another program, such
//! as the second half of a user's pipeline, may lead a group of that id by
//! now.
//!
//! References:
//! - ADR-403 §4: a server outlives no host, whichever way the host ends.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// One server's entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Entry {
    group: u32,
    leader_start: Option<u64>,
    owner: u32,
    owner_start: Option<u64>,
}

/// What became of a process an entry names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    /// It runs with the recorded start time.
    Alive,
    /// No process has the pid, or one with another start time does.
    Gone,
    /// A process has the pid, and its start time, or the recorded one,
    /// is unknown.
    Unknown,
}

/// Where the entries live.
pub fn dir() -> PathBuf {
    ways_core::paths::state_root().join("attend").join("mcp-servers")
}

/// The entries of the project at `project` under `base`.
pub(super) fn project_dir(base: &Path, project: &Path) -> PathBuf {
    let h = agent_identity::identity::fnv1a_64(project.to_string_lossy().as_bytes());
    base.join(format!("{h:016x}"))
}

/// This process's start time, read once.
fn own_start() -> Option<u64> {
    static START: OnceLock<Option<u64>> = OnceLock::new();
    *START.get_or_init(|| attend_presence::process::start_stamp(std::process::id()))
}

/// Note the server whose group is `group`, started by this process for the
/// project at `project`, and return its entry's path. None when it cannot
/// be written, or a start time cannot be read; the server then goes
/// unswept after a kill, and nothing else.
pub fn note(base: &Path, project: &Path, group: u32) -> Option<PathBuf> {
    let entry = Entry { group, leader_start: Some(attend_presence::process::start_stamp(group)?), owner: std::process::id(), owner_start: Some(own_start()?) };
    let dir = project_dir(base, project);
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{group}.json"));
    std::fs::write(&path, serde_json::to_vec(&entry).ok()?).ok()?;
    Some(path)
}

/// Remove an entry once its server ended.
pub fn forget(path: &Path) {
    std::fs::remove_file(path).ok();
}

/// End the servers of the project at `project` whose owner is gone, and
/// drop the entries that no longer name a running server. Returns how
/// many groups were ended.
pub fn sweep(base: &Path, project: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(project_dir(base, project)) else { return 0 };
    let mut ended = 0;
    for path in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
        let Some(e) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Entry>(&b).ok()) else {
            forget(&path);
            continue;
        };
        let read = |pid: u32, at: Option<u64>| seen(pid, at, attend_presence::process::start_stamp(pid));
        match (read(e.owner, e.owner_start), read(e.group, e.leader_start)) {
            (Seen::Gone, Seen::Alive) => {
                if end_group(e.group) {
                    ended += 1;
                }
                forget(&path);
            }
            (Seen::Gone, Seen::Gone) => forget(&path),
            _ => {}
        }
    }
    ended
}

/// What became of the process `pid`, recorded as started at `at`, whose
/// start time reads `now`.
fn seen(pid: u32, at: Option<u64>, now: Option<u64>) -> Seen {
    match (at, now) {
        (None, _) => Seen::Unknown,
        (Some(a), Some(n)) if a == n => Seen::Alive,
        (Some(_), Some(_)) => Seen::Gone,
        (_, None) if !exists(pid) => Seen::Gone,
        _ => Seen::Unknown,
    }
}

/// Whether a process has the pid `pid`: kill(2) with signal 0 fails with
/// ESRCH only when none does.
fn exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: kill(2) with signal 0 sends nothing and takes no pointers.
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        attend_presence::process::start_stamp(pid).is_some()
    }
}

/// Whether a process of the group `group` still runs.
fn group_lives(group: u32) -> bool {
    #[cfg(unix)]
    // SAFETY: kill(2) with signal 0 sends nothing and takes no pointers.
    return unsafe { libc::kill(-(group as libc::pid_t), 0) } == 0;
    #[cfg(not(unix))]
    false
}

/// TERM the group `group`, wait a moment, then KILL what is left. Whether
/// a signal reached it.
fn end_group(group: u32) -> bool {
    #[cfg(unix)]
    {
        let g = -(group as libc::pid_t);
        // SAFETY: kill(2) takes no pointers.
        if unsafe { libc::kill(g, libc::SIGTERM) } != 0 {
            return false;
        }
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until && group_lives(group) {
            std::thread::sleep(Duration::from_millis(20));
        }
        // SAFETY: as above.
        unsafe { libc::kill(g, libc::SIGKILL) };
        true
    }
    #[cfg(not(unix))]
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A process read with an unreadable start time is unknown while it
    /// runs and gone once no process has its pid; a recorded start that
    /// was never read is unknown.
    #[test]
    fn an_unreadable_start_time_is_unknown_unless_the_pid_is_free() {
        let me = std::process::id();
        assert_eq!(seen(me, Some(7), Some(7)), Seen::Alive);
        assert_eq!(seen(me, Some(7), Some(8)), Seen::Gone, "another start time is a reused pid");
        assert_eq!(seen(me, Some(7), None), Seen::Unknown, "a running process whose start cannot be read");
        assert_eq!(seen(me, None, Some(7)), Seen::Unknown, "a start that was never recorded");
        let mut done = std::process::Command::new("true").spawn().unwrap();
        let free = done.id();
        done.wait().unwrap();
        assert_eq!(seen(free, Some(7), None), Seen::Gone);
    }
}
