//! `attend permissions audit` — cross-check sensor `requires:` against
//! the grants declared in `~/.claude/settings.json`.

use crate::config;
use crate::sensors::Focus;

pub(crate) fn cmd_permissions_audit() {
    use agent_fmt::permissions;

    let focus = Focus::default_focus();
    let cfg = config::Config::load(&focus.working_dir);

    // Load settings.json grants
    let home = std::env::var("HOME").unwrap_or_default();
    let settings_path = std::path::PathBuf::from(&home).join(".claude/settings.json");
    let grants = permissions::load_settings_permissions(&settings_path);

    if grants.is_empty() {
        eprintln!("Warning: no permissions found in {}", settings_path.display());
    }

    // Collect (sensor_name, requires) pairs from config
    let mut requirements: Vec<(String, Vec<String>)> = Vec::new();
    let mut names: Vec<&String> = cfg.sensors.keys().collect();
    names.sort();
    for name in names {
        let sensor = &cfg.sensors[name];
        if !sensor.requires.is_empty() {
            let prefix = if sensor.script.is_some() { "+" } else { "" };
            requirements.push((
                format!("{prefix}{name}"),
                sensor.requires.clone(),
            ));
        }
    }

    let results = permissions::audit(&requirements, &grants);
    permissions::display_audit("Attend Permissions Audit", "Sensor", &results, false);
}
