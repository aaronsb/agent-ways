//! `attend scene` / `attend scenes` — named focus-group presets.

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

    let mut t = agent_fmt::Table::new(&["Scene", "Focus groups"]);
    for name in &names {
        let scene = &all[*name];
        let groups_str = if scene.rooms.is_empty() {
            "(none — project only)".to_string()
        } else {
            scene.rooms.join(", ")
        };
        t.add(vec![name.as_str(), &groups_str]);
    }
    t.print();
}
