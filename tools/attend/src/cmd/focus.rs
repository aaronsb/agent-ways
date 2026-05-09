//! `attend focus` — manage named attention groups.

use crate::util::get_groups;

pub(crate) fn cmd_focus_on(name: &str, pin: bool) {
    let r = get_groups();
    match r.join(name, pin) {
        Ok(()) => {
            let suffix = if pin { " (pinned)" } else { "" };
            println!("[attend] focus: attending to {name}{suffix}");
        }
        Err(e) => {
            eprintln!("[attend] focus: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn cmd_focus_off(name: &str) {
    let r = get_groups();
    r.leave(name).ok();
    println!("[attend] focus: released {name}");
}

pub(crate) fn cmd_focus_clear() {
    let r = get_groups();
    for (name, _) in r.my_groups() {
        r.leave(&name).ok();
    }
    println!("[attend] focus: cleared (project only)");
}

pub(crate) fn cmd_focus_pin(name: &str) {
    let r = get_groups();
    r.pin(name);
    println!("[attend] focus: pinned {name}");
}

pub(crate) fn cmd_focus_unpin(name: &str) {
    let r = get_groups();
    r.unpin(name);
    println!("[attend] focus: unpinned {name}");
}

pub(crate) fn cmd_focus_dissolve(name: &str) {
    let r = get_groups();
    let members = r.dissolve(name);
    if members.is_empty() {
        println!("[attend] focus: dissolved {name} (was empty)");
    } else {
        println!(
            "[attend] focus: dissolved {name} ({} members released)",
            members.len()
        );
    }
}

pub(crate) fn cmd_focus_all() {
    let r = get_groups();
    r.cleanup_stale();
    let all = r.all_groups();
    let mut t = agent_fmt::Table::new(&["Focus", "Members", "Pinned"]);
    t.align(1, agent_fmt::Align::Right);
    // ADR-124 §I.4: base channel leads the list, mirrors the TUI's
    // leftmost rule. `(all)` captures the fact that every peer is
    // implicitly subscribed.
    t.add(vec!["#open", "(all)", "(base)"]);
    for (name, count, pinned) in &all {
        t.add(vec![
            name.as_str(),
            &count.to_string(),
            if *pinned { "yes" } else { "no" },
        ]);
    }
    t.print();
}

pub(crate) fn cmd_focus_list() {
    let r = get_groups();
    let my = r.my_groups();
    if my.is_empty() {
        println!("focus: project only");
    } else {
        let mut t = agent_fmt::Table::new(&["Focus", "Pinned"]);
        for (name, pinned) in &my {
            t.add(vec![name.as_str(), if *pinned { "yes" } else { "no" }]);
        }
        t.print();
    }
}
