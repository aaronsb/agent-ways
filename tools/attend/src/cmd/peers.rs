//! `attend peers` — list active Claude Code sessions and focus groups.

use crate::util::get_groups;

pub(crate) fn cmd_peers() {
    let r = get_groups();

    #[cfg(feature = "sensor-peers")]
    let peers = {
        let sensor = crate::sensors::PeerSensor::new();
        sensor.list_peers()
    };
    #[cfg(not(feature = "sensor-peers"))]
    let peers: Vec<(String, String, String, f64)> = Vec::new();

    let my_focus = r.my_groups();

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, 24);

    // Show self in project group
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let self_project = cwd.rsplit('/').next().unwrap_or("?");
    t.add(vec!["(project)", self_project, "working", ""]);

    // Show named groups we're focused on
    for (name, pinned) in &my_focus {
        let pin_marker = if *pinned { " (pinned)" } else { "" };
        let label = format!("{name}{pin_marker}");
        t.add(vec![&label, "(you)", "", ""]);
    }

    // Show peers
    if !peers.is_empty() {
        t.add(vec!["", "", "", ""]);
        for (peer_cwd, project, status, ctx) in &peers {
            let focus_label = if *peer_cwd == cwd {
                "(project)".to_string()
            } else {
                String::new()
            };
            t.add(vec![&focus_label, project, status, &format!("{ctx:.0}%")]);
        }
    }

    t.print();

    let focus_count = my_focus.len();
    let peer_count = peers.len();
    println!(
        "  {} agent(s), {} focus group(s)",
        peer_count + 1,
        focus_count + 1
    );
}
