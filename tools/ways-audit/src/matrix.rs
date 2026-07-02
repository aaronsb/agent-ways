//! `ways-audit matrix` — control-to-way traceability matrix.

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

    // Collect rows: (way, control, justification)
    let mut rows: Vec<(String, String, String)> = Vec::new();

    for (way_id, data) in ways {
        let prov = &data["provenance"];
        if prov.is_null() {
            continue;
        }
        if let Some(controls) = prov["controls"].as_array() {
            for c in controls {
                if let Some(obj) = c.as_object() {
                    let cid = obj
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string();
                    if let Some(justifications) =
                        obj.get("justifications").and_then(|v| v.as_array())
                    {
                        if justifications.is_empty() {
                            rows.push((way_id.clone(), cid, "(no justification)".to_string()));
                        } else {
                            for j in justifications {
                                rows.push((
                                    way_id.clone(),
                                    cid.clone(),
                                    j.as_str().unwrap_or("").to_string(),
                                ));
                            }
                        }
                    } else {
                        rows.push((way_id.clone(), cid, "(no justification)".to_string()));
                    }
                } else if let Some(s) = c.as_str() {
                    rows.push((
                        way_id.clone(),
                        s.to_string(),
                        "(legacy — no justification)".to_string(),
                    ));
                }
            }
        }
    }

    rows.sort();

    if json_out {
        let result: Vec<Value> = rows
            .iter()
            .map(|(w, c, j)| json!({"way": w, "control": c, "justification": j}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!();
    println!("\x1b[1mClaim Traceability Matrix\x1b[0m");
    println!();
    println!(
        "  \x1b[1m{:<28} {:<50} JUSTIFICATION\x1b[0m",
        "WAY", "CONTROL"
    );
    println!(
        "  \x1b[2m{:<28} {:<50} -------------\x1b[0m",
        "---", "-------"
    );

    for (way, ctrl, just) in &rows {
        println!("  {:<28} {:<50} {}", way, &ctrl[..ctrl.len().min(50)], just);
    }

    let total_c = rows.len();
    let total_j = rows.iter().filter(|(_, _, j)| !j.starts_with('(')).count();
    println!();
    println!(
        "  \x1b[2mTotal: {total_c} control claims, {total_j} justifications\x1b[0m"
    );

    Ok(())
}
