//! `attend channel scene` / `attend channel scenes` — named channel presets.

use crate::scenes;
use crate::util::{current_session, groups_for, Session};

pub(crate) fn cmd_scene(name: &str) {
    let me = current_session();
    let r = groups_for(&me);
    let all = scenes::load_scenes();
    // A scene's channels are joined as `attend channel join` joins one: their old
    // messages are marked seen first, once the scene is known to activate.
    let held = match scenes::checked(&all, name) {
        Ok(scene) => scene.channels.iter().map(|c| crate::util::baseline_joined_room(&me, &r, c)).sum(),
        Err(_) => 0,
    };
    match scenes::activate(name, &r) {
        Ok(result) => {
            settle_scene_enrollment(&me, name, &r);
            println!("[attend] scene '{name}': {result}{}", crate::util::backlog_note(held));
            // A scene that leaves the session in a channel enrolls it, so
            // it names the session as `attend channel join` does.
            if !r.my_groups().is_empty() {
                if let Some(line) = crate::util::you_are_line(&me) {
                    println!("{line}");
                }
            }
        }
        Err(e) => {
            eprintln!("[attend] scene: {e}");
            std::process::exit(1);
        }
    }
}

/// A scene that leaves the session in channels enrolls it as a join. One
/// that leaves it in none withdraws the join, and `private` is the explicit
/// opt-out: it also ends `attend run`'s enrollment, at once when no run
/// holds the session, or once the live one has gone (#720).
fn settle_scene_enrollment(ident: &Session, name: &str, groups: &crate::groups::Groups) {
    if !groups.my_groups().is_empty() {
        crate::util::enroll_by_join(ident);
        return;
    }
    crate::util::settle_join_enrollment(ident, groups);
    if name == "private" && ident.resolved() {
        attend_presence::enrollment::opt_out(&ident.session_id).ok();
    }
}

pub(crate) fn cmd_scenes() {
    let all = scenes::load_scenes();
    let mut names: Vec<&String> = all.keys().collect();
    names.sort();

    // transition: removed by #717 (ADR-506)
    let legacy: Vec<String> = names
        .iter()
        .filter_map(|n| all[*n].legacy_rooms_error(n))
        .collect();
    if !legacy.is_empty() {
        for e in legacy {
            eprintln!("[attend] scenes: {e}");
        }
        std::process::exit(1);
    }

    let mut t = agent_fmt::Table::new(&["Scene", "Channels"]);
    for name in &names {
        let scene = &all[*name];
        let groups_str = if scene.channels.is_empty() {
            "(none — project only)".to_string()
        } else {
            scene.channels.join(", ")
        };
        t.add(vec![name.as_str(), &groups_str]);
    }
    t.print();
}
