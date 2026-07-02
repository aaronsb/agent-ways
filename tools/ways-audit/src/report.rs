//! `ways-audit report` — claim coverage overview.

use anyhow::Result;
use serde_json::{json, Value};

use crate::helpers::{find_incomplete, find_stale_ways, obj_len};

pub fn run(manifest: &Value, json_out: bool) -> Result<()> {
    let total = manifest["ways_scanned"].as_u64().unwrap_or(0);
    let with = manifest["ways_with_provenance"].as_u64().unwrap_or(0);
    let without = manifest["ways_without_provenance"].as_u64().unwrap_or(0);
    let policies = obj_len(&manifest["coverage"]["by_policy"]);
    let controls = obj_len(&manifest["coverage"]["by_control"]);

    let stale_ways = find_stale_ways(manifest, 90);
    let incomplete = find_incomplete(manifest);

    if json_out {
        let result = json!({
            "total_ways": total,
            "with_provenance": with,
            "without_provenance": without,
            "coverage_pct": if total > 0 { with * 100 / total } else { 0 },
            "policy_sources": policies,
            "control_references": controls,
            "stale_ways": stale_ways,
            "incomplete_ways": incomplete,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!();
    println!("\x1b[1mClaim Coverage Report\x1b[0m");
    println!();

    if total > 0 {
        let pct = with * 100 / total;
        let color = if pct >= 75 {
            "\x1b[0;32m"
        } else if pct >= 40 {
            "\x1b[1;33m"
        } else {
            "\x1b[0;31m"
        };
        println!("  Ways scanned:        {:3}", total);
        println!(
            "  With claims:         {}{:3} ({}%)\x1b[0m",
            color, with, pct
        );
        println!("  Without claims:      {:3}", without);
    } else {
        println!("  Ways scanned:        {:3}", total);
    }
    println!();

    // Policy sources
    println!(
        "\x1b[1mPolicy Sources\x1b[0m \x1b[2m({policies}):\x1b[0m"
    );
    if let Some(by_policy) = manifest["coverage"]["by_policy"].as_object() {
        for (uri, ways) in by_policy {
            println!("  \x1b[0;36m{uri}\x1b[0m");
            if let Some(arr) = ways.as_array() {
                let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                println!("    \x1b[2m→ {}\x1b[0m", names.join(", "));
            }
        }
    }
    println!();

    // Control references
    println!(
        "\x1b[1mControl References\x1b[0m \x1b[2m({controls}):\x1b[0m"
    );
    if let Some(by_control) = manifest["coverage"]["by_control"].as_object() {
        for (cid, ways) in by_control {
            println!("  {cid}");
            if let Some(arr) = ways.as_array() {
                let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                println!("    \x1b[2m→ {}\x1b[0m", names.join(", "));
            }
        }
    }
    println!();

    if !stale_ways.is_empty() {
        println!(
            "\x1b[1mStale Claims\x1b[0m \x1b[1;33m(verified > 90 days ago):\x1b[0m"
        );
        for way in &stale_ways {
            if let Some(verified) = manifest["ways"][way]["provenance"]["verified"].as_str() {
                println!("  \x1b[1;33m{way}\x1b[0m \x1b[2m(verified: {verified})\x1b[0m");
            }
        }
        println!();
    }

    if !incomplete.is_empty() {
        println!(
            "\x1b[1mIncomplete Claims\x1b[0m \x1b[1;33m(missing policy, controls, or rationale):\x1b[0m"
        );
        for way in &incomplete {
            println!("  \x1b[1;33m{way}\x1b[0m");
        }
        println!();
    }

    // Ways without a claim
    println!("\x1b[1mWays without a claim:\x1b[0m");
    if let Some(arr) = manifest["coverage"]["without_provenance"].as_array() {
        for way in arr {
            if let Some(s) = way.as_str() {
                println!("  \x1b[2m{s}\x1b[0m");
            }
        }
    }
    println!();

    Ok(())
}
