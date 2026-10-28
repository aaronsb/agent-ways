//! `ways-audit report` — claim coverage overview.

use anyhow::Result;
use serde_json::{json, Value};

use crate::helpers::{find_incomplete, find_stale_ways, obj_len};
use agent_theme::{paint, Role, Style};

const BOLD: Style = Style::new().bold();
const WARN: Style = Style::new().role(Role::Warn).bold();

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
            "coverage_pct": (with * 100).checked_div(total).unwrap_or(0),
            "policy_sources": policies,
            "control_references": controls,
            "stale_ways": stale_ways,
            "incomplete_ways": incomplete,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let mut out = String::new();
    agent_fmt::say!(out);
    agent_fmt::say!(out, "{}", paint(BOLD, "Claim Coverage Report"));
    agent_fmt::say!(out);

    if let Some(pct) = (with * 100).checked_div(total) {
        let color = if pct >= 75 {
            Style::new().role(Role::Ok)
        } else if pct >= 40 {
            WARN
        } else {
            Style::new().role(Role::Err)
        };
        agent_fmt::say!(out, "  Ways scanned:        {:3}", total);
        agent_fmt::say!(out, 
            "  With claims:         {}",
            paint(color, format!("{:3} ({}%)", with, pct))
        );
        agent_fmt::say!(out, "  Without claims:      {:3}", without);
    } else {
        agent_fmt::say!(out, "  Ways scanned:        {:3}", total);
    }
    agent_fmt::say!(out);

    // Policy sources
    agent_fmt::say!(out, 
        "{} {}",
        paint(BOLD, "Policy Sources"),
        paint(Role::Muted, format!("({policies}):"))
    );
    if let Some(by_policy) = manifest["coverage"]["by_policy"].as_object() {
        for (uri, ways) in by_policy {
            agent_fmt::say!(out, "  {}", paint(Role::Accent, uri));
            if let Some(arr) = ways.as_array() {
                let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                agent_fmt::say!(out, "    {}", paint(Role::Muted, format!("→ {}", names.join(", "))));
            }
        }
    }
    agent_fmt::say!(out);

    // Control references
    agent_fmt::say!(out, 
        "{} {}",
        paint(BOLD, "Control References"),
        paint(Role::Muted, format!("({controls}):"))
    );
    if let Some(by_control) = manifest["coverage"]["by_control"].as_object() {
        for (cid, ways) in by_control {
            agent_fmt::say!(out, "  {cid}");
            if let Some(arr) = ways.as_array() {
                let names: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                agent_fmt::say!(out, "    {}", paint(Role::Muted, format!("→ {}", names.join(", "))));
            }
        }
    }
    agent_fmt::say!(out);

    if !stale_ways.is_empty() {
        agent_fmt::say!(out, 
            "{} {}",
            paint(BOLD, "Stale Claims"),
            paint(WARN, "(verified > 90 days ago):")
        );
        for way in &stale_ways {
            if let Some(verified) = manifest["ways"][way]["provenance"]["verified"].as_str() {
                agent_fmt::say!(out, "  {} {}", paint(WARN, way), paint(Role::Muted, format!("(verified: {verified})")));
            }
        }
        agent_fmt::say!(out);
    }

    if !incomplete.is_empty() {
        agent_fmt::say!(out, 
            "{} {}",
            paint(BOLD, "Incomplete Claims"),
            paint(WARN, "(missing policy, controls, or rationale):")
        );
        for way in &incomplete {
            agent_fmt::say!(out, "  {}", paint(WARN, way));
        }
        agent_fmt::say!(out);
    }

    // Ways without a claim
    agent_fmt::say!(out, "{}", paint(BOLD, "Ways without a claim:"));
    if let Some(arr) = manifest["coverage"]["without_provenance"].as_array() {
        for way in arr {
            if let Some(s) = way.as_str() {
                agent_fmt::say!(out, "  {}", paint(Role::Muted, s));
            }
        }
    }
    agent_fmt::say!(out);
    agent_fmt::print_block(&out);

    Ok(())
}
