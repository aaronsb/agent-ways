//! `ways-audit gaps/stale/active` — audit-focused queries.

use anyhow::Result;
use serde_json::{json, Value};

use crate::helpers::{cutoff_date, find_stale_ways};
use ways_core::firing::{count_fires, load_events};
use agent_theme::{paint, Role, Style};

const BOLD: Style = Style::new().bold();
const WARN: Style = Style::new().role(Role::Warn).bold();

pub fn gaps(manifest: &Value, json_out: bool) -> Result<()> {
    let without = &manifest["coverage"]["without_provenance"];

    if json_out {
        println!("{}", serde_json::to_string_pretty(without)?);
        return Ok(());
    }

    let total = manifest["ways_scanned"].as_u64().unwrap_or(0);
    let count = manifest["ways_without_provenance"].as_u64().unwrap_or(0);

    println!();
    println!(
        "{} {}",
        paint(BOLD, "Ways Without a Claim"),
        paint(WARN, format!("({count} of {total})"))
    );
    println!();
    if let Some(arr) = without.as_array() {
        for way in arr {
            if let Some(s) = way.as_str() {
                println!("  {s}");
            }
        }
    }

    Ok(())
}

pub fn stale(manifest: &Value, days: u32, json_out: bool) -> Result<()> {
    let stale_ways = find_stale_ways(manifest, days);

    if json_out {
        let result: Vec<Value> = stale_ways
            .iter()
            .filter_map(|way| {
                let verified = manifest["ways"][way.as_str()]["provenance"]["verified"]
                    .as_str()?
                    .to_string();
                Some(json!({"way": way, "verified": verified}))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let cutoff = cutoff_date(days);
    println!();
    println!(
        "{} {}",
        paint(BOLD, "Stale Claims"),
        paint(Role::Muted, format!("(verified > {days} days ago, cutoff: {cutoff})"))
    );
    println!();

    if stale_ways.is_empty() {
        println!("  {}", paint(Role::Ok, "All claim dates are current."));
    } else {
        for way in &stale_ways {
            let verified = manifest["ways"][way.as_str()]["provenance"]["verified"]
                .as_str()
                .unwrap_or("?");
            println!("  {way}  (verified: {verified})");
        }
    }

    Ok(())
}

pub fn active(manifest: &Value, json_out: bool) -> Result<()> {
    let stats = load_events();
    let fire_counts = count_fires(&stats);

    let with_prov = match manifest["coverage"]["with_provenance"].as_array() {
        Some(a) => a,
        None => {
            println!("No claim data.");
            return Ok(());
        }
    };

    if json_out {
        let result: Vec<Value> = with_prov
            .iter()
            .filter_map(|v| {
                let way = v.as_str()?;
                let fires = fire_counts.get(way).copied().unwrap_or(0);
                Some(json!({"way": way, "fires": fires}))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let total_governed = manifest["ways_with_provenance"].as_u64().unwrap_or(0);
    let total_ways = manifest["ways_scanned"].as_u64().unwrap_or(0);

    println!();
    println!("{}", paint(BOLD, "Active Claims Report"));
    println!();
    println!(
        "  Ways with claims: {} of {total_ways}",
        paint(Role::Ok, total_governed)
    );
    println!();
    println!(
        "  {}",
        paint(BOLD, format!("{:<28} {:>5}  Status", "Way", "Fires"))
    );
    println!(
        "  {}",
        paint(Role::Muted, format!("{:<28} {:>5}  ------", "---", "-----"))
    );

    for v in with_prov {
        let way = match v.as_str() {
            Some(s) => s,
            None => continue,
        };
        let fires = fire_counts.get(way).copied().unwrap_or(0);
        let status = if fires > 0 {
            paint(Role::Ok, "active")
        } else {
            paint(Role::Muted, "dormant")
        };
        println!("  {:<28} {:>5}  {}", way, fires, status);
    }

    // Ways without a claim, ranked by fire count.
    println!();
    println!(
        "{} {}",
        paint(BOLD, "Ways without a claim"),
        paint(Role::Muted, "(top by fire count):")
    );
    if let Some(without) = manifest["coverage"]["without_provenance"].as_array() {
        let mut ungov_fires: Vec<(&str, u64)> = without
            .iter()
            .filter_map(|v| {
                let way = v.as_str()?;
                let fires = fire_counts.get(way).copied().unwrap_or(0);
                if fires > 0 {
                    Some((way, fires))
                } else {
                    None
                }
            })
            .collect();
        ungov_fires.sort_by_key(|e| std::cmp::Reverse(e.1));

        if ungov_fires.is_empty() {
            println!("  (no firing data for ways without a claim)");
        } else {
            for (way, fires) in ungov_fires.iter().take(5) {
                println!(
                    "  {:<28} {:>5} fires {}",
                    way, fires, paint(WARN, "(no claim)")
                );
            }
        }
    }

    Ok(())
}
