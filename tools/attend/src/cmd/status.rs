//! `attend status` — show running instances, signal counts, and focus.

use crate::util::{count_signals, get_groups};

pub(crate) fn cmd_status() {
    // Check if attend run is already active
    let output = std::process::Command::new("ps")
        .args(["-ww", "-eo", "pid=,args="])
        .output()
        .ok();

    let mut instances: Vec<u32> = Vec::new();
    if let Some(out) = output {
        let own_pid = std::process::id();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Some((pid, args)) = line.trim().split_once(char::is_whitespace) else { continue };
            if let Ok(pid) = pid.parse::<u32>() {
                if pid != own_pid && is_attend_run(args) {
                    instances.push(pid);
                }
            }
        }
    }

    // Gather all data before building a single unified table
    let cwd = crate::util::own_origin_cwd();
    let r = get_groups();
    let dirs = r.receive_dirs(&cwd);
    let pending = |room: attend_groups::Room| -> usize {
        dirs.iter().filter(|d| d.room == room).map(|d| count_signals(&d.path)).sum()
    };
    let own_count = pending(attend_groups::Room::Project);
    let broadcast_count = pending(attend_groups::Room::Open);
    let my_focus = r.my_groups();

    // Single table: Section | Detail | Info
    let mut t = agent_fmt::Table::new(&["", "Detail", "Info"]);
    t.align(0, agent_fmt::Align::Left);

    // ── Instances section. `ps` lists every `attend run` on the machine,
    // one per session that runs attend, so each row names the session it
    // serves. Two rows are normal when two sessions run attend.
    if instances.is_empty() {
        t.add(vec!["instances", "(none)", ""]);
    } else {
        let own = attend_presence::session::identity().session_id;
        let owners: Vec<(String, Option<String>)> = instances
            .iter()
            .map(|&pid| {
                let i = attend_presence::session::identity_for_pid(pid);
                if !i.session_resolved {
                    return (String::new(), None);
                }
                // A name derives from the origin; with none readable, the
                // session id stands in rather than a guessed name.
                let name = if i.origin_path.is_empty() {
                    format!("session {}", &i.session_id[..i.session_id.len().min(8)])
                } else {
                    crate::cmd::whoami::display_name(&i)
                };
                (i.session_id, Some(name))
            })
            .collect();
        for (i, (&pid, (sid, name))) in instances.iter().zip(&owners).enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            let runs = owners.iter().filter(|(s, _)| s == sid).count();
            let info = match name {
                Some(name) => instance_info(name, sid == &own, runs),
                None => unowned_info(pid, reparented(pid)),
            };
            t.add(vec![label, &format!("PID {pid}"), &info]);
        }
    }

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Signals section
    t.add(vec!["signals", "project", &format!("{own_count} pending")]);
    t.add(vec!["", "#open", &format!("{broadcast_count} pending")]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Keepwarm section (ADR-182)
    t.add(vec!["keepwarm", &crate::cmd::keepwarm::status_line(), ""]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Focus section
    if my_focus.is_empty() {
        t.add(vec!["channels", "project only", ""]);
    } else {
        for (i, (name, pinned)) in my_focus.iter().enumerate() {
            let label = if i == 0 { "channels" } else { "" };
            let pin = if *pinned { " (pinned)" } else { "" };
            let info = format!("{name}{pin}");
            t.add(vec![label, &info, ""]);
        }
    }

    t.print();
    // Printed whole, outside the table, which would cut a long path.
    println!("cache: {}", attend_presence::cache::dir().display());
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

/// What a row says when no Claude Code session record names the run. That
/// alone proves nothing (a record can be missing or unreadable), so the
/// run is called leftover only when it was reparented to init or a
/// subreaper.
fn unowned_info(pid: u32, reparented: bool) -> String {
    let base = format!("no Claude Code session found for pid {pid}");
    if reparented {
        format!("{base}; a leftover run, safe to stop")
    } else {
        base
    }
}

/// What an `attend run` row says about its owner. `name` is the owning
/// session's display name. `runs` counts the rows with the same owner. The
/// session lock (ADR-129) allows one run per session, so a second is worth
/// flagging.
fn instance_info(name: &str, mine: bool, runs: usize) -> String {
    let whose = if mine { "this session" } else { "another session" };
    if runs > 1 {
        format!("{name} ({whose}, duplicate run)")
    } else {
        format!("{name} ({whose})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_run_names_the_session_it_serves() {
        assert_eq!(instance_info("Chaucer-alpha", true, 1), "Chaucer-alpha (this session)");
        assert_eq!(instance_info("McKay", false, 1), "McKay (another session)");
    }

    #[test]
    fn a_second_run_for_one_session_is_flagged() {
        assert_eq!(instance_info("Chaucer", true, 2), "Chaucer (this session, duplicate run)");
    }

    #[test]
    fn a_run_without_a_session_is_leftover_only_when_reparented() {
        assert_eq!(unowned_info(42, false), "no Claude Code session found for pid 42");
        assert_eq!(unowned_info(42, true), "no Claude Code session found for pid 42; a leftover run, safe to stop");
    }

    #[test]
    fn only_the_attend_program_running_run_is_an_instance() {
        assert!(is_attend_run("attend run"));
        assert!(is_attend_run("/home/u/.local/bin/attend run --catchup"));
        assert!(!is_attend_run("/bin/zsh -c eval 'attend run'"));
        assert!(!is_attend_run("nvim notes-about-attend run"));
        assert!(!is_attend_run("attend-chat run"));
        assert!(!is_attend_run("attend status"));
        assert!(!is_attend_run(""));
    }

    /// A run whose parent is alive and is no subreaper is not leftover.
    #[test]
    fn a_run_with_a_live_parent_is_not_reparented() {
        assert!(!reparented(std::process::id()));
    }
}
