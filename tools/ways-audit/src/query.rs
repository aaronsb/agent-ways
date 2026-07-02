//! `ways-audit control` and `ways-audit policy` — pattern queries.

use anyhow::{bail, Result};
use serde_json::{json, Value};

pub fn control(manifest: &Value, pattern: &str, json_out: bool) -> Result<()> {
    let by_control = match manifest["coverage"]["by_control"].as_object() {
        Some(m) => m,
        None => bail!("No control data in manifest"),
    };

    let pattern_lower = pattern.to_lowercase();
    let matches: Vec<(&String, &Value)> = by_control
        .iter()
        .filter(|(k, _)| k.to_lowercase().contains(&pattern_lower))
        .collect();

    if matches.is_empty() {
        let available: Vec<&String> = by_control.keys().collect();
        bail!(
            "No controls matching '{}'. Available:\n  {}",
            pattern,
            available.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
        );
    }

    if json_out {
        let result: Vec<Value> = matches
            .iter()
            .map(|(k, v)| json!({"control": k, "data": v}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!(
        "\x1b[1mControls matching\x1b[0m '\x1b[0;36m{pattern}\x1b[0m':"
    );
    println!();
    for (cid, ways) in &matches {
        println!("  {cid}");
        if let Some(arr) = ways.as_array() {
            let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            println!("    implementing: {}", names.join(", "));
        }
        println!();
    }

    Ok(())
}

pub fn policy(manifest: &Value, pattern: &str, json_out: bool) -> Result<()> {
    let by_policy = match manifest["coverage"]["by_policy"].as_object() {
        Some(m) => m,
        None => bail!("No policy data in manifest"),
    };

    let pattern_lower = pattern.to_lowercase();
    let matches: Vec<(&String, &Value)> = by_policy
        .iter()
        .filter(|(k, _)| k.to_lowercase().contains(&pattern_lower))
        .collect();

    if matches.is_empty() {
        let available: Vec<&String> = by_policy.keys().collect();
        bail!(
            "No policies matching '{}'. Available:\n  {}",
            pattern,
            available.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
        );
    }

    if json_out {
        let result: Vec<Value> = matches
            .iter()
            .map(|(k, v)| json!({"policy": k, "data": v}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!(
        "\x1b[1mPolicies matching\x1b[0m '\x1b[0;36m{pattern}\x1b[0m':"
    );
    println!();
    for (uri, ways) in &matches {
        println!("  {uri}");
        if let Some(arr) = ways.as_array() {
            let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            println!("    implementing ways: {}", names.join(", "));
        }
        println!();
    }

    Ok(())
}
