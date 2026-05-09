//! `attend sensors` — list every sensor known to this build.
//!
//! Reports built-in (compiled-in crate) sensors and any script sensors
//! defined in config, with their state (active / off / not compiled /
//! missing) and source (`crate@version` or script path). Pure metadata
//! — no polling, no side effects.

use crate::config::Config;
use crate::sensors::{enumerate_sensors, Focus, SensorEntry, SensorState};
use std::time::Duration;

pub(crate) fn cmd_sensors(args: &[String]) {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    if let Some(unknown) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!("attend sensors: unknown flag '{unknown}' — try --help");
        std::process::exit(1);
    }

    let focus = Focus::default_focus();
    let cfg = Config::load(&focus.working_dir);
    let entries = enumerate_sensors(&cfg, &focus);

    let mut t = agent_fmt::Table::new(&["Sensor", "Kind", "State", "Interval", "Description", "Source"]);
    t.align(0, agent_fmt::Align::Left);
    // Disable auto-fit: agent-fmt's default expands col 0 to soak up
    // leftover terminal width, which pushes the rest of the columns
    // far to the right when col 0 is short content (sensor names).
    t.no_auto_fit();

    for e in &entries {
        t.add_owned(vec![
            e.name.clone(),
            e.kind.label().to_string(),
            e.state.label().to_string(),
            format_interval(e),
            e.description.clone(),
            e.source.clone(),
        ]);
    }

    t.print();
    print_legend(&entries);
}

fn format_interval(e: &SensorEntry) -> String {
    format!("{} / {}", fmt_secs(e.interval), fmt_secs(e.min_interval))
}

fn fmt_secs(d: Duration) -> String {
    format!("{}s", d.as_secs())
}

fn print_help() {
    println!("attend sensors — list every sensor known to this build");
    println!();
    println!("usage: attend sensors");
    println!();
    println!("Reports built-in (compiled-in crate) sensors and any script sensors");
    println!("defined in config, with their state and source.");
    println!();
    println!("State values:");
    println!("  active        compiled in (or script file present) and enabled in config");
    println!("  off           compiled in (or script file present) but disabled in config");
    println!("  not compiled  built-in whose feature flag was excluded at build time");
    println!("  missing       script: path does not resolve to a file");
    println!();
    println!("To toggle a sensor's enabled state, edit the relevant `enabled:` field");
    println!("in your attend.yaml — see `attend config path` for locations.");
}

/// Print short hints under the table for any non-Active states present.
/// Skipped when every sensor is Active to keep the common case quiet.
fn print_legend(entries: &[SensorEntry]) {
    let has_off = entries.iter().any(|e| matches!(e.state, SensorState::Off));
    let has_not_compiled = entries.iter().any(|e| matches!(e.state, SensorState::NotCompiled));
    let has_missing = entries.iter().any(|e| matches!(e.state, SensorState::Missing));

    if !(has_off || has_not_compiled || has_missing) {
        return;
    }
    println!();
    if has_off {
        println!("  off          disabled in config (set `enabled: true` to activate)");
    }
    if has_not_compiled {
        println!("  not compiled feature flag excluded at build time");
    }
    if has_missing {
        println!("  missing      script: path does not resolve to a file");
    }
    println!("  see `attend sensors --help` for the full state vocabulary");
}
