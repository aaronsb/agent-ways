//! `ways-audit lint` — claim quality checks.

use anyhow::Result;
use serde_json::{json, Value};

pub fn run(manifest: &Value, json_out: bool) -> Result<()> {
    let ways = match manifest["ways"].as_object() {
        Some(m) => m,
        None => {
            println!("No ways data.");
            return Ok(());
        }
    };

    let mut errors: Vec<(String, String)> = Vec::new();
    let mut warnings: Vec<(String, String)> = Vec::new();
    let date_re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();

    for (way_id, data) in ways {
        let prov = &data["provenance"];
        if prov.is_null() {
            continue;
        }

        // Check: controls exist
        let ctrl_count = prov["controls"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        if ctrl_count == 0 {
            errors.push((
                way_id.clone(),
                "claim declared but no controls listed".to_string(),
            ));
        }

        // Check: structured controls have justifications
        if let Some(controls) = prov["controls"].as_array() {
            for c in controls {
                if let Some(obj) = c.as_object() {
                    let cid = obj.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let j_count = obj
                        .get("justifications")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    if j_count == 0 {
                        warnings.push((
                            way_id.clone(),
                            format!("control has no justifications: {cid}"),
                        ));
                    }
                }
            }

            // Check: legacy string controls
            let legacy_count = controls.iter().filter(|c| c.is_string()).count();
            if legacy_count > 0 {
                warnings.push((
                    way_id.clone(),
                    format!("{legacy_count} control(s) in legacy format (no justifications)"),
                ));
            }
        }

        // Check: policy URIs reference real files
        if let Some(policies) = prov["policy"].as_array() {
            for p in policies {
                if let Some(uri) = p["uri"].as_str() {
                    if !uri.starts_with("github://") && !uri.starts_with("http") {
                        let home = std::env::var("HOME").unwrap_or_default();
                        let full = format!("{home}/.claude/{uri}");
                        if !std::path::Path::new(&full).exists() {
                            errors.push((
                                way_id.clone(),
                                format!("policy URI not found: {uri}"),
                            ));
                        }
                    }
                }
            }
        }

        // Check: verified date
        match prov["verified"].as_str() {
            None => {
                warnings.push((way_id.clone(), "no verified date".to_string()));
            }
            Some(v) => {
                if !date_re.is_match(v) {
                    errors.push((
                        way_id.clone(),
                        format!("invalid verified date: {v}"),
                    ));
                }
            }
        }

        // Check: rationale
        if prov["rationale"].as_str().is_none() {
            warnings.push((way_id.clone(), "no rationale".to_string()));
        }
    }

    errors.sort();
    warnings.sort();

    let error_count = errors.len();
    let warning_count = warnings.len();

    if json_out {
        let result = json!({
            "errors": error_count,
            "warnings": warning_count,
            "passed": error_count == 0,
            "error_details": errors.iter().map(|(w, m)| json!({"way": w, "message": m})).collect::<Vec<_>>(),
            "warning_details": warnings.iter().map(|(w, m)| json!({"way": w, "message": m})).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
        if error_count > 0 {
            std::process::exit(1);
        }
        return Ok(());
    }

    println!();
    println!("\x1b[1mClaim Lint Report\x1b[0m");
    println!();

    for (way, msg) in &errors {
        println!(
            "  \x1b[0;31m{:<6}\x1b[0m [{:<28}] {}",
            "ERROR", way, msg
        );
    }
    for (way, msg) in &warnings {
        println!(
            "  \x1b[1;33m{:<6}\x1b[0m [{:<28}] {}",
            "WARN", way, msg
        );
    }

    if error_count == 0 && warning_count == 0 {
        println!("  \x1b[0;32mAll claim checks passed.\x1b[0m");
    } else {
        println!();
        println!(
            "  Results: \x1b[0;31m{error_count} error(s)\x1b[0m, \x1b[1;33m{warning_count} warning(s)\x1b[0m"
        );
        if error_count > 0 {
            println!("  \x1b[0;31mLint FAILED — errors must be resolved.\x1b[0m");
            std::process::exit(1);
        }
    }

    Ok(())
}
