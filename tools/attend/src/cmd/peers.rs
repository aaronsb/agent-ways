//! `attend peers` — list active Claude Code sessions and focus groups.

use crate::util::{get_groups, own_session_id};
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
    let peers: Vec<(String, String, String, String, f64)> = Vec::new();

    let my_focus = r.my_groups();
    let registry = attend_instances::Registry::new();

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, 28);

    // Self row: derive identity from our cwd so the nickname matches
    // what peers see when they render a signal from us.
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let self_id = Identity::for_cwd(&cwd, caps);
    // Look up our own instance suffix (ADR-129). Falls back to the
    // bare nickname when the registry is unreadable or our session
    // somehow has no entry — neither should happen in practice
    // since `attend run` registers at startup.
    let self_sid = own_session_id();
    let self_instance = self_sid
        .as_deref()
        .and_then(|sid| registry.lookup(&cwd, sid));
    let self_label = render_agent_label(&self_id, self_instance.as_deref(), caps);
    // Self-row Focus cell: "(self)" identifies which row IS the runner of
    // `attend peers`, without using the literal string "project" — that
    // priming caused agents to reach for `attend send --focus project`
    // and silently land signals in an empty `@project/` room.
    t.add_owned(vec![
        "(self)".to_string(),
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
        for (peer_sid, peer_cwd, _project, status, ctx) in &peers {
            // Same-cwd peers previously rendered "(project)" in the
            // Focus column — a string that primed agents into running
            // `attend send --focus project`, a group nobody had
            // joined. "(here)" pairs with the self row's "(self)"
            // marker, preserves the same-cwd visual scan, and uses
            // parentheses to make clear it is not a focus group name.
            let focus_label = if *peer_cwd == cwd {
                "(here)".to_string()
            } else {
                String::new()
            };
            let peer_id = Identity::for_cwd(peer_cwd, caps);
            let peer_instance = registry.lookup(peer_cwd, peer_sid);
            let label = render_agent_label(&peer_id, peer_instance.as_deref(), caps);
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
    // `focus_count` is now the literal number of explicit focus groups
    // self has joined. The previous `+ 1` counted the implicit "(self)"
    // row as if it were a focus group, which inflated the count and
    // suggested a phantom "project" group existed.
    println!(
        "  {} agent(s), {} focus group(s)",
        peer_count + 1,
        focus_count
    );
}

/// Compose the "Agent" column cell: nickname (with instance suffix
/// when present, ADR-129) in identity color, cwd basename in parens
/// in dim. The agent-fmt table measures visible length correctly
/// even with ANSI codes embedded, so we can style freely without
/// breaking alignment.
///
/// The suffix is concatenated *before* color wrapping so the whole
/// `Tamsin-alpha` token shares the identity color — keeps the visual
/// scan tight and matches the wire identity (a single addressable
/// name, not a colored stem with a plain tail).
fn render_agent_label(id: &Identity, instance: Option<&str>, caps: TermCaps) -> String {
    let display = match instance {
        Some(inst) => format!("{}-{}", id.nickname, inst),
        None => id.nickname.to_string(),
    };
    let nick = ansi::wrap(&display, &id.palette, id.style, caps);
    let dim_basename = format!("\x1b[2m({})\x1b[0m", id.cwd_basename);
    format!("{nick} {dim_basename}")
}
