//! `ways-audit gaps/stale/active` — audit-focused queries.

use anyhow::Result;
use serde_json::{json, Value};

use crate::helpers::{cutoff_date, find_stale_ways};
use ways_core::firing::{count_fires, load_events};

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
        "\x1b[1mWays Without a Claim\x1b[0m \x1b[1;33m({count} of {total})\x1b[0m"
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
        "\x1b[1mStale Claims\x1b[0m \x1b[2m(verified > {days} days ago, cutoff: {cutoff})\x1b[0m"
    );
    println!();

    if stale_ways.is_empty() {
        println!("  \x1b[0;32mAll claim dates are current.\x1b[0m");
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
    println!("\x1b[1mActive Claims Report\x1b[0m");
    println!();
    println!(
        "  Ways with claims: \x1b[0;32m{total_governed}\x1b[0m of {total_ways}"
    );
    println!();
    println!(
        "  \x1b[1m{:<28} {:>5}  Status\x1b[0m",
        "Way", "Fires"
    );
    println!(
        "  \x1b[2m{:<28} {:>5}  ------\x1b[0m",
        "---", "-----"
    );

    for v in with_prov {
        let way = match v.as_str() {
            Some(s) => s,
            None => continue,
        };
        let fires = fire_counts.get(way).copied().unwrap_or(0);
        let status = if fires > 0 {
            "\x1b[0;32mactive\x1b[0m"
        } else {
            "\x1b[2mdormant\x1b[0m"
        };
        println!("  {:<28} {:>5}  {}", way, fires, status);
    }

    // Ways without a claim, ranked by fire count.
    println!();
    println!(
        "\x1b[1mWays without a claim\x1b[0m \x1b[2m(top by fire count):\x1b[0m"
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
        ungov_fires.sort_by(|a, b| b.1.cmp(&a.1));

        if ungov_fires.is_empty() {
            println!("  (no firing data for ways without a claim)");
        } else {
            for (way, fires) in ungov_fires.iter().take(5) {
                println!(
                    "  {:<28} {:>5} fires \x1b[1;33m(no claim)\x1b[0m",
                    way, fires
                );
            }
        }
    }

    Ok(())
}
