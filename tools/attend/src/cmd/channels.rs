//! `attend channel join` / `add` / `leave` / `list` / `dissolve` — channel
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
            if let Some(me) = &j.you_are {
                println!("you are {me}");
            }
        }
        Err(e) => {
            if json {
                crate::cmd::print_json_error(&e, &e.to_string());
            }
            eprintln!("[attend] join: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn cmd_add(member: &str, channel: Option<&str>, json: bool) {
    match crate::verbs::channels::add(member, channel) {
        Ok(a) => {
            if json {
                crate::cmd::print_json(&a);
            } else {
                println!("[attend] {}", added_line(&a));
            }
            if let Some(e) = &a.not_told {
                eprintln!("[attend] add: {} is in #{}, but telling them failed: {e}", a.member, a.name);
            }
        }
        Err(e) => {
            if json {
                crate::cmd::print_json_error(&e, &e.to_string());
            }
            eprintln!("[attend] add: {e}");
            std::process::exit(1);
        }
    }
}

/// What an add did, as the actor reads it: who was added where, then what
/// the member has to read of the channel and how many older messages it was
/// not handed.
fn added_line(a: &crate::verbs::channels::Added) -> String {
    let mut line = format!("added {} to #{}", a.member, a.name);
    if a.to_read > 0 {
        let messages = if a.to_read == 1 { "message" } else { "messages" };
        line += &format!(": {} has {} earlier {messages} to read", a.member, a.to_read);
    }
    match a.held {
        0 => {}
        1 => line += "; 1 older one is held back",
        n => line += &format!("; {n} older ones are held back"),
    }
    line
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
    let members = match attend_membership::dissolve(&attend_membership::Bus::here(), "", name) {
        Ok(d) => d.members,
        Err(e) => {
            eprintln!("[attend] dissolve: {e}");
            std::process::exit(1);
        }
    };
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
    // Who is in each channel this session is in, one line each, so a member
    // sees who joined before anyone posts. Other channels show their count.
    for c in channels.iter().filter(|c| !c.member_names.is_empty()) {
        println!("  #{}: {}", c.name, c.member_names.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::added_line;

    fn added(to_read: usize, held: usize) -> crate::verbs::channels::Added {
        crate::verbs::channels::Added {
            name: "topic".into(),
            member: "Ada".into(),
            session_id: "s".into(),
            to_read,
            held,
            not_told: None,
        }
    }

    #[test]
    fn the_add_line_says_what_the_member_has_to_read() {
        assert_eq!(added_line(&added(0, 0)), "added Ada to #topic");
        assert_eq!(added_line(&added(1, 0)), "added Ada to #topic: Ada has 1 earlier message to read");
        assert_eq!(added_line(&added(50, 1)), "added Ada to #topic: Ada has 50 earlier messages to read; 1 older one is held back");
        assert_eq!(added_line(&added(50, 10)), "added Ada to #topic: Ada has 50 earlier messages to read; 10 older ones are held back");
    }
}
