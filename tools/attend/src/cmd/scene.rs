//! `attend scene` / `attend scenes` — named channel presets.

use crate::scenes;
use crate::util::get_groups;

pub(crate) fn cmd_scene(name: &str) {
    let r = get_groups();
    match scenes::activate(name, &r) {
        Ok(result) => {
            settle_scene_enrollment(name, &r);
            println!("[attend] scene '{name}': {result}");
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
fn settle_scene_enrollment(name: &str, groups: &crate::groups::Groups) {
    if !groups.my_groups().is_empty() {
        crate::util::enroll_by_join();
        return;
    }
    crate::util::settle_join_enrollment(groups);
    let ident = attend_presence::session::identity();
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
