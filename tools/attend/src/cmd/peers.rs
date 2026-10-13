//! `attend peer list` — list active Claude Code sessions and their channels.
//! The listing is `verbs::peers`; this renders it.

use crate::verbs::peers::{self, Agent};
use agent_identity::{ansi, Identity};
use agent_theme::Painter;

pub(crate) fn cmd_peers(json: bool) {
    let p = peers::peers();
    if json {
        crate::cmd::print_json(&p);
        return;
    }
    let painter = agent_theme::painter();
    let caps = painter.depth();

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, 28);

    // Self-row Focus cell: "(self)" identifies which row IS the runner of
    // `attend peer list`, without using the literal string "project" — that
    // priming caused agents to reach for `attend message send --channel project`
    // and silently land signals in an empty `@project/` room.
    t.add_owned(vec![
        "(self)".to_string(),
        render_agent_label(&p.me, caps, &painter),
        p.me.status.clone(),
        String::new(),
    ]);

    // Show named groups we're focused on
    for c in &p.channels {
        let pin_marker = if c.pinned { " (pinned)" } else { "" };
        let label = format!("{}{pin_marker}", c.name);
        t.add(vec![&label, "(you)", "", ""]);
    }

    // Show peers
    if !p.peers.is_empty() {
        t.add(vec!["", "", "", ""]);
        for peer in &p.peers {
            // Same-cwd peers previously rendered "(project)" in the
            // Focus column — a string that primed agents into running
            // `attend message send --channel project`, a group nobody had
            // joined. "(here)" pairs with the self row's "(self)"
            // marker, preserves the same-cwd visual scan, and uses
            // parentheses to make clear it is not a focus group name.
            let focus_label = if peer.here { "(here)".to_string() } else { String::new() };
            t.add_owned(vec![
                focus_label,
                render_agent_label(peer, caps, &painter),
                peer.status.clone(),
                format!("{:.0}%", peer.context_pct.unwrap_or(0.0)),
            ]);
        }
    }

    t.print();

    // Who is in each of this session's channels, so a member sees who
    // joined before anyone posts.
    for c in &p.channels {
        println!("  #{}: {}", c.name, c.members.join(", "));
    }

    // The channel count is the explicit channels this session joined; the
    // "(self)" row is not one.
    println!(
        "  {} agent(s), {} channel(s)",
        p.peers.len() + 1,
        p.channels.len()
    );
}

/// Compose the "Agent" column cell: the name (with instance suffix when
/// present, ADR-129) in identity color, cwd basename in parens in dim. The
/// agent-fmt table measures visible length correctly even with ANSI codes
/// embedded, so we can style freely without breaking alignment.
///
/// The suffix is part of the name before color wrapping, so the whole
/// `Tamsin-alpha` token shares the identity color — keeps the visual scan
/// tight and matches the wire identity (a single addressable name, not a
/// colored stem with a plain tail).
fn render_agent_label(agent: &Agent, caps: agent_theme::ColorDepth, painter: &Painter) -> String {
    let id = Identity::for_cwd(&agent.cwd, caps);
    let nick = ansi::wrap(&agent.name, &id.palette, id.style, painter);
    let dim_basename = painter.paint(agent_theme::Role::Muted, format!("({})", id.cwd_basename));
    format!("{nick} {dim_basename}")
}
