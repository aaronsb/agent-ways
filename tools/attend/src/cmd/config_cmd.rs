//! `attend config show` renderer. Named `config_cmd` to avoid shadowing
//! the top-level `config` module.

use crate::config;

pub(crate) fn display_config(cfg: &config::Config) {
    // Governor section
    let mut t = agent_fmt::Table::new(&["", "Setting", "Value"]);
    t.align(0, agent_fmt::Align::Left);

    t.add(vec![
        "governor",
        "base_cooldown",
        &format!("{}s", cfg.governor.base_cooldown.as_secs()),
    ]);
    t.add(vec![
        "",
        "max_per_window",
        &cfg.governor.max_per_window.to_string(),
    ]);
    t.add(vec![
        "",
        "rate_window",
        &format!("{}s", cfg.governor.rate_window.as_secs()),
    ]);

    t.add(vec!["", "", ""]);

    // Engagement section (ADR-119 action potential, unified in ADR-123)
    t.add(vec![
        "engagement",
        "burst_threshold",
        &cfg.engagement.burst_threshold.to_string(),
    ]);
    t.add(vec![
        "",
        "step_multiplier",
        &format!("{:.2}", cfg.engagement.step_multiplier),
    ]);
    t.add(vec![
        "",
        "absolute_refractory",
        &format!("{}s", cfg.engagement.absolute_refractory.as_secs()),
    ]);
    t.add(vec![
        "",
        "decay_per_minute",
        &format!("{:.4}", cfg.engagement.decay_per_minute),
    ]);
    t.add(vec![
        "",
        "peer_activity_window",
        &format!("{}s", cfg.engagement.peer_activity_window.as_secs()),
    ]);

    t.add(vec!["", "", ""]);

    // Sensors — sorted by name
    let mut names: Vec<&String> = cfg.sensors.keys().collect();
    names.sort();

    for (i, name) in names.iter().enumerate() {
        let sc = &cfg.sensors[*name];
        let sensor_type = if sc.script.is_some() { "script" } else { "crate" };
        let enabled = if sc.enabled { "" } else { " (disabled)" };
        let label = format!("{name}{enabled}");

        let section = if i == 0 { "sensors" } else { "" };
        t.add(vec![
            section,
            &label,
            &format!(
                "[{sensor_type}] interval={}s min={}s threshold={} decay={}",
                sc.interval.as_secs(),
                sc.min_interval.as_secs(),
                sc.threshold,
                sc.decay_threshold,
            ),
        ]);

        if let Some(ref script) = sc.script {
            t.add(vec!["", "", &format!("script: {script}")]);
        }
        if !sc.requires.is_empty() {
            t.add(vec!["", "", &format!("requires: [{}]", sc.requires.join(", "))]);
        }
    }

    t.print();
}
