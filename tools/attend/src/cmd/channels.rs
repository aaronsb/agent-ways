//! `attend join` / `leave` / `channels` / `dissolve` — channel
//! membership, the chat-idiom verbs (ADR-173).
//!
//! Storage is untouched by the vocabulary: channels are still `@name/`
//! signal namespaces with membership in `_groups.yaml` (attend-groups).
//! Join, leave and the listings are `verbs::channels`; this renders them.
//! The lifecycle verbs below them (dissolve, create, describe, pin) stay
//! CLI-only.

use crate::util::get_groups;

pub(crate) fn cmd_join(name: &str, pin: bool, json: bool) {
    match crate::verbs::channels::join(name, pin) {
        Ok(j) if json => crate::cmd::print_json(&j),
        Ok(j) => {
            let suffix = if j.pinned { " (pinned)" } else { "" };
            println!("[attend] joined #{}{suffix}{}", j.name, j.note);
        }
        Err(e) => {
            if json {
                crate::cmd::print_json_error(&e, &e.message);
            }
            eprintln!("[attend] join: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn cmd_leave(name: &str, json: bool) {
    let left = crate::verbs::channels::leave(name);
    if json {
        crate::cmd::print_json(&left);
    } else if left.was_member {
        println!("[attend] left #{}", left.name);
    } else {
        println!("[attend] not in #{} — nothing to leave", left.name);
    }
}

pub(crate) fn cmd_dissolve(name: &str) {
    let name = name.trim_start_matches('#');
    // Mirror the TUI's guard (PR #395 finding 2): the base channel is
    // structural — dissolving it would rip out the reserved
    // migration dir and report false success.
    if name == "open" {
        eprintln!("[attend] dissolve: #open is the base channel — it cannot be dissolved");
        std::process::exit(1);
    }
    let r = get_groups();
    let members = r.dissolve(name);
    if members.is_empty() {
        println!("[attend] dissolved #{name} (was empty)");
    } else {
        println!(
            "[attend] dissolved #{name} ({} members released)",
            members.len()
        );
    }
}

/// Create a channel without joining it (#404): explicit lifecycle,
/// pinned by the shared crate so it survives empty.
pub(crate) fn cmd_create(name: &str, description: &str) {
    let r = get_groups();
    let desc = (!description.trim().is_empty()).then_some(description.trim());
    match r.create(name, desc) {
        Ok(()) => match desc {
            Some(d) => println!("[attend] created #{name} — {d}"),
            None => println!("[attend] created #{name}"),
        },
        Err(e) => {
            eprintln!("[attend] create: {e}");
            std::process::exit(1);
        }
    }
}

/// Set or replace a channel's description (#404). Empty text clears.
pub(crate) fn cmd_describe(name: &str, description: &str) {
    let r = get_groups();
    match r.set_description(name, description) {
        Ok(()) => {
            if description.trim().is_empty() {
                println!("[attend] cleared #{name} description");
            } else {
                println!("[attend] #{name} — {}", description.trim());
            }
        }
        Err(e) => {
            eprintln!("[attend] describe: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn cmd_pin(name: &str) {
    let name = name.trim_start_matches('#');
    get_groups().pin(name);
    println!("[attend] pinned #{name}");
}

pub(crate) fn cmd_unpin(name: &str) {
    let name = name.trim_start_matches('#');
    get_groups().unpin(name);
    println!("[attend] unpinned #{name}");
}

/// Joined channels only. Read-only, so a hook macro can call it on every fire
/// without `cmd_channels`'s cleanup rewriting shared state.
pub(crate) fn cmd_joined(json: bool) {
    let my = crate::verbs::channels::joined();
    if json {
        crate::cmd::print_json(&my);
    } else if my.is_empty() {
        println!("channels: project only");
    } else {
        let mut t = agent_fmt::Table::new(&["Channel", "Pinned"]);
        for c in &my {
            let label = format!("#{}", c.name);
            t.add(vec![&label, if c.pinned { "yes" } else { "no" }]);
        }
        t.print();
    }
}

/// All channels with membership marks — the agent-side mirror of the
/// TUI's `/channels`.
pub(crate) fn cmd_channels(json: bool) {
    let channels = crate::verbs::channels::list();
    if json {
        crate::cmd::print_json(&channels);
        return;
    }
    let mut t =
        agent_fmt::Table::new(&["Channel", "Members", "Joined", "Pinned", "Description"]);
    t.align(1, agent_fmt::Align::Right);
    // ADR-124 §I.4: base channel leads the list, mirrors the TUI's
    // leftmost rule. `(all)` captures the fact that every peer is
    // implicitly subscribed.
    t.add(vec!["#open", "(all)", "(always)", "(base)", "the commons"]);
    for c in &channels {
        let label = format!("#{}", c.name);
        t.add(vec![
            &label,
            &c.members.to_string(),
            if c.joined { "yes" } else { "" },
            if c.pinned { "yes" } else { "no" },
            c.description.as_deref().unwrap_or(""),
        ]);
    }
    t.print();
}
