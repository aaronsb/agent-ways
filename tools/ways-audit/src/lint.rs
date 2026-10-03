//! `ways-audit lint` — claim quality checks.

use anyhow::Result;
use serde_json::{json, Value};
use agent_theme::{paint, Role, Style};

const BOLD: Style = Style::new().bold();
const WARN: Style = Style::new().role(Role::Warn).bold();

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
                        // A relative URI names a file in the app (where the
                        // shipped governance/ lives since ADR-142), in the
                        // ~/.claude projection, or under the directory lint
                        // runs from (a separate compliance repo's root).
                        let found = [ways_core::paths::data_root(), ways_core::paths::projection_root(), std::path::PathBuf::from(".")]
                            .iter()
                            .any(|base| base.join(uri).exists());
                        if !found {
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
    println!("{}", paint(BOLD, "Claim Lint Report"));
    println!();

    for (way, msg) in &errors {
        println!(
            "  {} [{:<28}] {}",
            paint(Role::Err, format!("{:<6}", "ERROR")), way, msg
        );
    }
    for (way, msg) in &warnings {
        println!(
            "  {} [{:<28}] {}",
            paint(WARN, format!("{:<6}", "WARN")), way, msg
        );
    }

    if error_count == 0 && warning_count == 0 {
        println!("  {}", paint(Role::Ok, "All claim checks passed."));
    } else {
        println!();
        println!(
            "  Results: {}, {}",
            paint(Role::Err, format!("{error_count} error(s)")),
            paint(WARN, format!("{warning_count} warning(s)"))
        );
        if error_count > 0 {
            println!("  {}", paint(Role::Err, "Lint FAILED — errors must be resolved."));
            std::process::exit(1);
        }
    }

    Ok(())
}
