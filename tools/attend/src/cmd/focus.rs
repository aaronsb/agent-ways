//! `attend focus` — manage named attention groups.

use crate::util::get_groups;

pub(crate) fn cmd_focus_new(args: &[String]) {
    let r = get_groups();

    match args.first().map(|s| s.as_str()) {
        Some("on") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus on <name> [--pin]");
                    std::process::exit(1);
                }
            };
            let pin = args.iter().any(|a| a == "--pin");
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
        Some("off") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus off <name>");
                    std::process::exit(1);
                }
            };
            r.leave(name).ok();
            println!("[attend] focus: released {name}");
        }
        Some("clear") => {
            for (name, _) in r.my_groups() {
                r.leave(&name).ok();
            }
            println!("[attend] focus: cleared (project only)");
        }
        Some("pin") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus pin <name>");
                    std::process::exit(1);
                }
            };
            r.pin(name);
            println!("[attend] focus: pinned {name}");
        }
        Some("unpin") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus unpin <name>");
                    std::process::exit(1);
                }
            };
            r.unpin(name);
            println!("[attend] focus: unpinned {name}");
        }
        Some("dissolve") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus dissolve <name>");
                    std::process::exit(1);
                }
            };
            let members = r.dissolve(name);
            if members.is_empty() {
                println!("[attend] focus: dissolved {name} (was empty)");
            } else {
                println!("[attend] focus: dissolved {name} ({} members released)", members.len());
            }
        }
        Some("all") => {
            r.cleanup_stale();
            let all = r.all_groups();
            if all.is_empty() {
                println!("no active focus groups");
                return;
            }
            let mut t = agent_fmt::Table::new(&["Focus", "Members", "Pinned"]);
            t.align(1, agent_fmt::Align::Right);
            for (name, count, pinned) in &all {
                t.add(vec![
                    name.as_str(),
                    &count.to_string(),
                    if *pinned { "yes" } else { "no" },
                ]);
            }
            t.print();
        }
        Some("list") | None => {
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
        Some(unknown) => {
            eprintln!("attend focus: unknown subcommand '{unknown}' — try on, off, list, all, clear, pin, unpin, dissolve");
            std::process::exit(1);
        }
    }
}
