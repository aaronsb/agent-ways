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

    // Scope per row, this session first: the cwd basename, extended only
    // where two live sessions would otherwise wear the same label.
    let roster = peers::roster(&p.me, &p.peers);
    let scopes = peers::scopes(&roster);

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, agent_column_cap(&roster, &scopes));

    // Self-row Focus cell: "(self)" identifies which row IS the runner of
    // `attend peer list`, without using the literal string "project" — that
    // priming caused agents to reach for `attend message send --channel project`
    // and silently land signals in an empty `@project/` room.
    t.add_owned(vec![
        "(self)".to_string(),
        render_agent_label(&p.me, &scopes[0], caps, &painter),
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
        for (peer, scope) in p.peers.iter().zip(&scopes[1..]) {
            // Same-cwd peers previously rendered "(project)" in the
            // Focus column — a string that primed agents into running
            // `attend message send --channel project`, a group nobody had
            // joined. "(here)" pairs with the self row's "(self)"
            // marker, preserves the same-cwd visual scan, and uses
            // parentheses to make clear it is not a focus group name.
            let focus_label = if peer.here { "(here)".to_string() } else { String::new() };
            t.add_owned(vec![
                focus_label,
                render_agent_label(peer, scope, caps, &painter),
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

/// The "Agent" column's width cap. 28 holds a name and a basename; a row
/// whose scope was extended to tell two sessions apart gets room for all of
/// it, since truncation would cut the part that differs.
fn agent_column_cap(roster: &[&Agent], scopes: &[String]) -> usize {
    roster
        .iter()
        .zip(scopes)
        .filter(|(a, s)| **s != agent_identity::cwd_basename(&a.cwd))
        .map(|(a, s)| a.name.chars().count() + s.chars().count() + 3)
        .fold(28, usize::max)
}

/// Compose the "Agent" column cell: the name (with instance suffix when
/// present, ADR-129) in identity color, then `scope` in parens in dim. The
/// scope is the cwd basename unless [`peers::scopes`] extended it to tell
/// two sessions apart. The agent-fmt table measures visible length correctly
/// even with ANSI codes embedded, so we can style freely without breaking
/// alignment.
///
/// The suffix is part of the name before color wrapping, so the whole
/// `Tamsin-alpha` token shares the identity color — keeps the visual scan
/// tight and matches the wire identity (a single addressable name, not a
/// colored stem with a plain tail).
fn render_agent_label(agent: &Agent, scope: &str, caps: agent_theme::ColorDepth, painter: &Painter) -> String {
    let id = Identity::for_cwd(&agent.cwd, caps);
    let nick = ansi::wrap(&agent.name, &id.palette, id.style, painter);
    let dim_scope = painter.paint(agent_theme::Role::Muted, format!("({scope})"));
    format!("{nick} {dim_scope}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(sid: &str, cwd: &str, name: &str) -> Agent {
        Agent {
            session_id: Some(sid.to_string()),
            cwd: cwd.to_string(),
            name: name.to_string(),
            nickname: name.to_string(),
            instance: None,
            status: "waiting".to_string(),
            context_pct: None,
            here: false,
        }
    }

    fn labels(agents: &[Agent]) -> Vec<String> {
        let roster: Vec<&Agent> = agents.iter().collect();
        let scopes = peers::scopes(&roster);
        let painter = Painter::plain();
        roster.iter().zip(&scopes).map(|(a, s)| render_agent_label(a, s, painter.depth(), &painter)).collect()
    }

    #[test]
    fn labels_without_a_collision_are_name_and_basename() {
        let agents = [
            agent("aaaaaaaa-1", "/home/a/praecipio/ai-workstation-ops", "Clelia"),
            agent("bbbbbbbb-2", "/home/a/over-the-hill", "Clelia"),
            agent("cccccccc-3", "/home/a/app", "McKay"),
        ];
        assert_eq!(labels(&agents), ["Clelia (ai-workstation-ops)", "Clelia (over-the-hill)", "McKay (app)"]);
        let roster: Vec<&Agent> = agents.iter().collect();
        assert_eq!(agent_column_cap(&roster, &peers::scopes(&roster)), 28);
    }

    #[test]
    fn colliding_labels_are_told_apart_and_fit_the_column() {
        let agents = [
            agent("aaaaaaaa-1111", "/x/praecipio/app", "Clelia-alpha"),
            agent("bbbbbbbb-2222", "/y/praecipio/app", "Clelia-alpha"),
            agent("cccccccc-3333", "/z/personal/app", "Clelia-alpha"),
        ];
        let got = labels(&agents);
        assert_eq!(
            got,
            [
                "Clelia-alpha (praecipio/app aaaaaaaa)",
                "Clelia-alpha (praecipio/app bbbbbbbb)",
                "Clelia-alpha (personal/app)",
            ]
        );
        let roster: Vec<&Agent> = agents.iter().collect();
        assert_eq!(agent_column_cap(&roster, &peers::scopes(&roster)), got[0].len());
    }
}
