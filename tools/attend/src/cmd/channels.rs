//! `attend join` / `leave` / `channels` / `dissolve` — channel
//! membership, the chat-idiom primary verbs (ADR-173). `attend focus …`
//! survives as a deprecated alias dispatching onto these handlers
//! (CLI-is-contract, ADR-124): same behavior, one extra stderr note.
//!
//! Storage is untouched by the vocabulary: channels are still `@name/`
//! signal namespaces with membership in `_groups.yaml` (attend-groups).

use crate::util::get_groups;

pub(crate) fn cmd_join(name: &str, pin: bool) {
    let name = name.trim_start_matches('#');
    let r = get_groups();
    match r.join(name, pin) {
        Ok(()) => {
            let suffix = if pin { " (pinned)" } else { "" };
            println!("[attend] joined #{name}{suffix}");
        }
        Err(e) => {
            eprintln!("[attend] join: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn cmd_leave(name: &str) {
    let name = name.trim_start_matches('#');
    let r = get_groups();
    if !r.my_groups().iter().any(|(n, _)| n == name) {
        println!("[attend] not in #{name} — nothing to leave");
        return;
    }
    r.leave(name).ok();
    println!("[attend] left #{name}");
}

pub(crate) fn cmd_leave_all() {
    let r = get_groups();
    for (name, _) in r.my_groups() {
        r.leave(&name).ok();
    }
    println!("[attend] left all channels (project only)");
}

pub(crate) fn cmd_pin(name: &str) {
    let name = name.trim_start_matches('#');
    let r = get_groups();
    r.pin(name);
    println!("[attend] pinned #{name}");
}

pub(crate) fn cmd_unpin(name: &str) {
    let name = name.trim_start_matches('#');
    let r = get_groups();
    r.unpin(name);
    println!("[attend] unpinned #{name}");
}

pub(crate) fn cmd_dissolve(name: &str) {
    let name = name.trim_start_matches('#');
    // Mirror the TUI's guard (PR #395 finding 2): the base channel is
    // structural — dissolving it would rip out the reserved `@open/`
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

/// All channels with membership marks — the agent-side mirror of the
/// TUI's `/channels`. Supersedes the old joined/all split (`focus
/// list` / `focus all`), which stays reachable via the deprecated
/// alias.
pub(crate) fn cmd_channels() {
    let r = get_groups();
    r.cleanup_stale();
    let joined: std::collections::HashSet<String> =
        r.my_groups().into_iter().map(|(n, _)| n).collect();
    let all = r.all_groups();
    let descriptions = attend_groups::load_groups(&crate::util::signals_base());
    let mut t =
        agent_fmt::Table::new(&["Channel", "Members", "Joined", "Pinned", "Description"]);
    t.align(1, agent_fmt::Align::Right);
    // ADR-124 §I.4: base channel leads the list, mirrors the TUI's
    // leftmost rule. `(all)` captures the fact that every peer is
    // implicitly subscribed.
    t.add(vec!["#open", "(all)", "(always)", "(base)", "the commons"]);
    for (name, count, pinned) in &all {
        let label = format!("#{name}");
        let desc = descriptions
            .get(name)
            .and_then(|e| e.description.clone())
            .unwrap_or_default();
        t.add(vec![
            &label,
            &count.to_string(),
            if joined.contains(name) { "yes" } else { "" },
            if *pinned { "yes" } else { "no" },
            &desc,
        ]);
    }
    t.print();
}

/// Joined channels only — the old `focus list` view, kept for the
/// deprecated alias path.
pub(crate) fn cmd_joined() {
    let r = get_groups();
    let my = r.my_groups();
    if my.is_empty() {
        println!("channels: project only");
    } else {
        let mut t = agent_fmt::Table::new(&["Channel", "Pinned"]);
        for (name, pinned) in &my {
            let label = format!("#{name}");
            t.add(vec![&label, if *pinned { "yes" } else { "no" }]);
        }
        t.print();
    }
}
