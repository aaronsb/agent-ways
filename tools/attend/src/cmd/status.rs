//! `attend status` — show running instances, signal counts, and focus.

use crate::util::{count_signals, encode_project, get_groups, signals_base};

pub(crate) fn cmd_status() {
    // Check if attend run is already active
    let output = std::process::Command::new("ps")
        .args(["--no-headers", "-eo", "pid,args"])
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
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_dir = base.join(encode_project(&cwd));
    let broadcast_dir = base.join("_broadcast");
    let own_count = count_signals(&own_dir);
    let broadcast_count = count_signals(&broadcast_dir);

    let r = get_groups();
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
    t.add(vec!["", "broadcast", &format!("{broadcast_count} pending")]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Focus section
    if my_focus.is_empty() {
        t.add(vec!["focus", "project only", ""]);
    } else {
        for (i, (name, pinned)) in my_focus.iter().enumerate() {
            let label = if i == 0 { "focus" } else { "" };
            let pin = if *pinned { " (pinned)" } else { "" };
            let info = format!("{name}{pin}");
            t.add(vec![label, &info, ""]);
        }
    }

    t.print();
}
