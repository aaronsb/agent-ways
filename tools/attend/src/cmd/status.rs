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

    // ── Instances section
    if instances.is_empty() {
        t.add(vec!["instances", "(none)", ""]);
    } else {
        for (i, (pid, cmd)) in instances.iter().enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            t.add(vec![label, &format!("PID {pid}"), cmd]);
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
}
