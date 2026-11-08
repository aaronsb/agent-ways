// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use std::io::{self, Write};

/// Emit a batch of sensor disclosures to stdout for Monitor delivery.
///
/// Semantic filtering: only medium/high priority events emit to stdout
/// (triggering a Monitor wake-up). Low-priority events are logged to
/// stderr for diagnostics. If a batch mixes priorities, the low events
/// collapse into a single footnote line so Claude knows they happened
/// without having to process each one.
/// Returns true if any events were emitted to stdout (Monitor will wake).
pub fn emit_batch(disclosures: &[(String, String, Vec<String>)]) -> bool {
    let mut actionable: Vec<(&str, &str, &Vec<String>)> = Vec::new();
    let mut quiet_sensors: Vec<&str> = Vec::new();
    let mut quiet_event_count: usize = 0;

    for (sensor, priority, events) in disclosures {
        if priority == "high" || priority == "medium" {
            actionable.push((sensor, priority, events));
        } else {
            quiet_sensors.push(sensor);
            quiet_event_count += events.len();
            // Log quiet events to stderr only (no Monitor wake-up)
            for event in events {
                log(&format!("quiet ({sensor}): {event}"));
            }
        }
    }

    // All quiet — nothing to stdout, no Monitor wake-up
    if actionable.is_empty() {
        log(&format!(
            "batch suppressed: {} quiet event(s) from {}",
            quiet_event_count,
            quiet_sensors.join(", "),
        ));
        return false;
    }

    // Emit actionable events to stdout (Monitor picks these up)
    for (sensor, priority, events) in &actionable {
        for event in *events {
            println!("[attend sensor={sensor} priority={priority}] {event}");
        }
    }

    // If there were also quiet events, add a collapsed footnote
    if !quiet_sensors.is_empty() {
        println!(
            "[attend] also: {} quiet event(s) from {}",
            quiet_event_count,
            quiet_sensors.join(", "),
        );
    }

    io::stdout().flush().ok();
    true
}

/// Log diagnostic info to stderr (does not become a Monitor notification).
pub fn log(message: &str) {
    eprintln!("[attend] {message}");
}
