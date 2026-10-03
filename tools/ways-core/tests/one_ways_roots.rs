//! One definition of the ways roots (#797), the engine side.
//!
//! With the projection pointed at a tree other than the app copy, the
//! introspection corpus (which resolves a fired way to its criteria) reads
//! the projected tree. One test in this binary, so setting the environment
//! races nothing.

#![cfg(unix)]

use std::path::Path;

fn way(root: &Path, id: &str) {
    let leaf = id.rsplit('/').next().unwrap();
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{leaf}.md")), format!("---\ndescription: the {leaf} way\npattern: {leaf}\n---\n# {leaf}\n")).unwrap();
}

#[test]
fn introspection_reads_the_projected_ways() {
    let base = std::env::temp_dir().join(format!("ways-core-one-roots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    let app = home.join(".local/share/agent-ways/hooks/ways");
    let dev = home.join("dev/hooks/ways");
    way(&app, "stale/oldway");
    way(&dev, "dev/newway");
    std::fs::create_dir_all(home.join(".claude/hooks")).unwrap();
    std::os::unix::fs::symlink(&dev, home.join(".claude/hooks/ways")).unwrap();

    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    std::env::set_var("XDG_CONFIG_HOME", home.join("config"));

    let project = base.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    for map in [
        ways_core::introspection::default_criteria_map(),
        ways_core::introspection::project_criteria_map(project.to_str().unwrap()),
    ] {
        assert!(map.contains_key("dev/newway"), "projected way missing: {:?}", map.keys().collect::<Vec<_>>());
        assert!(!map.contains_key("stale/oldway"), "app copy leaked: {:?}", map.keys().collect::<Vec<_>>());
    }
    let _ = std::fs::remove_dir_all(&base);
}
