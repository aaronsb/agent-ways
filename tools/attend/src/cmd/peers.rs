//! `attend peers` — list active Claude Code sessions and focus groups.

use crate::util::get_groups;
use agent_identity::{ansi, Identity, TermCaps};

pub(crate) fn cmd_peers() {
    let r = get_groups();
    let caps = TermCaps::detect();

    #[cfg(feature = "sensor-peers")]
    let peers = {
        let sensor = crate::sensors::PeerSensor::new();
        sensor.list_peers()
    };
    #[cfg(not(feature = "sensor-peers"))]
    let peers: Vec<(String, String, String, f64)> = Vec::new();

    let my_focus = r.my_groups();

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, 28);

    // Self row: derive identity from our cwd so the nickname matches
    // what peers see when they render a signal from us.
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let self_id = Identity::for_cwd(&cwd, caps);
    let self_label = render_agent_label(&self_id, caps);
    t.add_owned(vec![
        "(project)".to_string(),
        self_label,
        "working".to_string(),
        String::new(),
    ]);

    // Show named groups we're focused on
    for (name, pinned) in &my_focus {
        let pin_marker = if *pinned { " (pinned)" } else { "" };
        let label = format!("{name}{pin_marker}");
        t.add(vec![&label, "(you)", "", ""]);
    }

    // Show peers
    if !peers.is_empty() {
        t.add(vec!["", "", "", ""]);
        for (peer_cwd, _project, status, ctx) in &peers {
            let focus_label = if *peer_cwd == cwd {
                "(project)".to_string()
            } else {
                String::new()
            };
            let peer_id = Identity::for_cwd(peer_cwd, caps);
            let label = render_agent_label(&peer_id, caps);
            t.add_owned(vec![
                focus_label,
                label,
                status.clone(),
                format!("{ctx:.0}%"),
            ]);
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

/// Compose the "Agent" column cell: nickname in identity color, cwd
/// basename in parens in dim. The agent-fmt table measures visible
/// length correctly even with ANSI codes embedded, so we can style
/// freely without breaking alignment.
fn render_agent_label(id: &Identity, caps: TermCaps) -> String {
    let nick = ansi::wrap(id.nickname, &id.palette, id.style, caps);
    let dim_basename = format!("\x1b[2m({})\x1b[0m", id.cwd_basename);
    format!("{nick} {dim_basename}")
}
