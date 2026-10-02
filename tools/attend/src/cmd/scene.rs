//! `attend scene` / `attend scenes` — named channel presets.

use crate::scenes;
use crate::util::get_groups;

pub(crate) fn cmd_scene(name: &str) {
    let r = get_groups();
    match scenes::activate(name, &r) {
        Ok(result) => println!("[attend] scene '{name}': {result}"),
        Err(e) => {
            eprintln!("[attend] scene: {e}");
            std::process::exit(1);
        }
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
