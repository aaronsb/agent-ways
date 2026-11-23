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
//! destructor and leaves its entries, and its servers run on.
//!
//! [`sweep`] runs when a sensor host starts. An entry whose owner still
//! runs belongs to a live host or command, such as another session's host
//! in the project, and is kept. Otherwise the entry's group is ended, TERM
//! then KILL, when it is still the server's: the leader runs with the
//! recorded start time, or the leader is gone and the group lives on,
//! which no other process can claim, since a pid is never reused while a
//! group of that id has members. A leader that runs with another start
//! time is a reused pid, and nothing is sent. The entry is then removed.
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
    leader_start: Option<String>,
    owner: u32,
    owner_start: Option<String>,
}

/// Where the entries live.
pub fn dir() -> PathBuf {
    ways_core::paths::state_root().join("attend").join("mcp-servers")
}

/// The entries of the project at `project` under `base`.
fn project_dir(base: &Path, project: &Path) -> PathBuf {
    let h = agent_identity::identity::fnv1a_64(project.to_string_lossy().as_bytes());
    base.join(format!("{h:016x}"))
}

/// This process's start time, read once.
fn own_start() -> Option<String> {
    static START: OnceLock<Option<String>> = OnceLock::new();
    START.get_or_init(|| attend_presence::process::start_time(std::process::id())).clone()
}

/// Note the server whose group is `group`, started by this process for the
/// project at `project`, and return its entry's path; none when it cannot
/// be written, which leaves the server unswept after a kill and nothing
/// else.
pub fn note(base: &Path, project: &Path, group: u32) -> Option<PathBuf> {
    let dir = project_dir(base, project);
    std::fs::create_dir_all(&dir).ok()?;
    let entry = Entry { group, leader_start: attend_presence::process::start_time(group), owner: std::process::id(), owner_start: own_start() };
    let path = dir.join(format!("{group}.json"));
    std::fs::write(&path, serde_json::to_vec(&entry).ok()?).ok()?;
    Some(path)
}

/// Remove an entry once its server ended.
pub fn forget(path: &Path) {
    std::fs::remove_file(path).ok();
}

/// End the servers of the project at `project` whose owner is gone, and
/// remove their entries. Returns how many groups were ended.
pub fn sweep(base: &Path, project: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(project_dir(base, project)) else { return 0 };
    let mut ended = 0;
    for path in rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
        let Some(e) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Entry>(&b).ok()) else {
            forget(&path);
            continue;
        };
        let owner_lives = e.owner_start.is_some() && attend_presence::process::start_time(e.owner) == e.owner_start;
        if owner_lives {
            continue;
        }
        let leader = attend_presence::process::start_time(e.group);
        let ours = match leader {
            Some(now) => Some(now) == e.leader_start,
            None => group_lives(e.group),
        };
        if ours && end_group(e.group) {
            ended += 1;
        }
        forget(&path);
    }
    ended
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
