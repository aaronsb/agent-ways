//! `ways-audit trace <way>` — full claim trace for a single way.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use ways_core::firing::load_events;
use agent_theme::{paint, Role, Style};

const BOLD: Style = Style::new().bold();
const WARN: Style = Style::new().role(Role::Warn).bold();

pub fn run(manifest: &Value, way_id: &str, json_out: bool) -> Result<()> {
    let way_data = &manifest["ways"][way_id];
    if way_data.is_null() {
        let available: Vec<&str> = manifest["ways"]
            .as_object()
            .map(|m| m.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        bail!(
            "way '{}' not found in manifest. Available:\n  {}",
            way_id,
            available.join("\n  ")
        );
    }

    if json_out {
        let mut out = way_data.clone();
        if let Some(obj) = out.as_object_mut() {
            obj.insert("way".to_string(), json!(way_id));
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!();
    println!(
        "{}{}",
        paint(BOLD, "Claim Trace: "),
        paint(Role::Accent, way_id)
    );
    println!();
    if let Some(path) = way_data["path"].as_str() {
        println!("  File: {}", paint(Role::Muted, path));
    }
    println!();

    let prov = &way_data["provenance"];
    if prov.is_null() {
        println!("  {}", paint(WARN, "(no claim)"));
        return Ok(());
    }

    // Policies
    println!("{}", paint(BOLD, "Policy sources:"));
    if let Some(policies) = prov["policy"].as_array() {
        for p in policies {
            let ptype = p["type"].as_str().unwrap_or("unknown");
            let uri = p["uri"].as_str().unwrap_or("");
            println!("  {ptype}: {uri}");
        }
    }
    println!();

    // Controls
    println!("{}", paint(BOLD, "Controls:"));
    if let Some(controls) = prov["controls"].as_array() {
        for c in controls {
            if let Some(obj) = c.as_object() {
                let cid = obj.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                println!("  {cid}");
                if let Some(justifications) = obj.get("justifications").and_then(|v| v.as_array()) {
                    for j in justifications {
                        if let Some(s) = j.as_str() {
                            println!("    ✓ {s}");
                        }
                    }
                }
            } else if let Some(s) = c.as_str() {
                println!("  {s}");
            }
        }
    }
    println!();

    // Verified
    match prov["verified"].as_str() {
        Some(v) => println!("  Verified: {}", paint(Role::Ok, v)),
        None => println!("  Verified: {}", paint(WARN, "not set")),
    }
    println!();

    // Rationale
    if let Some(rationale) = prov["rationale"].as_str() {
        println!("{}", paint(BOLD, "Rationale:"));
        println!("  {rationale}");
    }

    // Firing history
    print_firing_history(way_id);

    Ok(())
}

fn print_firing_history(way_id: &str) {
    let events = load_events();
    let fires: Vec<&str> = events
        .iter()
        .filter(|e| {
            e["event"].as_str() == Some("way_fired") && e["way"].as_str() == Some(way_id)
        })
        .filter_map(|e| e["ts"].as_str())
        .collect();

    if !fires.is_empty() {
        let first = fires.first().map(|s| &s[..10.min(s.len())]).unwrap_or("?");
        let last = fires.last().map(|s| &s[..10.min(s.len())]).unwrap_or("?");
        println!();
        println!("Firing history: {} times ({first} → {last})", fires.len());
    }
}
