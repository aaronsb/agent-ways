//! `attend status` — show running instances, signal counts, and focus.

use crate::util::{count_signals, get_groups};

pub(crate) fn cmd_status() {
    // Check if attend run is already active
    let output = std::process::Command::new("ps")
        .args(["-ww", "-eo", "pid=,args="])
        .output()
        .ok();

    let mut instances: Vec<(String, String)> = Vec::new(); // (pid, info)
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let own_pid = std::process::id();
        for line in stdout.lines() {
            let line = line.trim();
            // Only match actual attend binary, not shell wrappers that contain "attend run"
            if !line.contains("attend run") || line.contains(&own_pid.to_string()) {
                continue;
            }
            // Skip zsh/bash wrapper lines (contain shell-snapshots or eval)
            if line.contains("shell-snapshots") || line.contains("eval '") {
                continue;
            }
            // Extract PID and show clean output
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 {
                instances.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
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
            .map(|(pid, _)| {
                let ident = pid.parse().map(attend_presence::session::identity_for_pid).ok();
                match ident.filter(|i| i.session_resolved) {
                    Some(i) => (i.session_id.clone(), Some(crate::cmd::whoami::display_name(&i))),
                    None => (String::new(), None),
                }
            })
            .collect();
        for (i, ((pid, _), (sid, name))) in instances.iter().zip(&owners).enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            let runs = owners.iter().filter(|(s, _)| s == sid).count();
            let info = instance_info(name.as_deref(), sid == &own, runs);
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

/// What an `attend run` row says about its owner. `name` is the owning
/// session's display name, `None` when no Claude Code session owns the
/// process. `runs` counts the rows with the same owner. The session lock
/// (ADR-129) allows one run per session, so a second is worth flagging, and
/// so is a run whose session has gone.
fn instance_info(name: Option<&str>, mine: bool, runs: usize) -> String {
    let Some(name) = name else {
        return "no owning session; a leftover run, safe to stop".to_string();
    };
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
        assert_eq!(instance_info(Some("Chaucer-alpha"), true, 1), "Chaucer-alpha (this session)");
        assert_eq!(instance_info(Some("McKay"), false, 1), "McKay (another session)");
    }

    #[test]
    fn a_second_run_for_one_session_and_an_orphan_are_flagged() {
        assert_eq!(instance_info(Some("Chaucer"), true, 2), "Chaucer (this session, duplicate run)");
        assert!(instance_info(None, false, 1).starts_with("no owning session"));
    }
}
