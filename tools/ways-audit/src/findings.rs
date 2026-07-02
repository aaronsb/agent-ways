//! `ways-audit assemble` and `ways-audit findings` — the finding dataset (ADR-201).
//!
//! `assemble` builds the classifier-ready dataset from the current claims and the
//! firing log; `findings` reads the ledger back. Neither writes a determination —
//! that boundary is enforced here by simply having no code that does.

use anyhow::Result;
use serde_json::Value;
use std::io::Write;

use ways_core::finding::{assemble as assemble_findings, Finding};
use ways_core::{firing, paths, util};

/// Assemble finding rows from the claim manifest + firing events, print them, and
/// optionally append them to the ledger. `way` filters to a single way id.
pub fn assemble(manifest: &Value, way: Option<&str>, write: bool, json_out: bool) -> Result<()> {
    let events = firing::load_events();
    let mut findings = assemble_findings(manifest, &events, &util::now_utc());
    if let Some(w) = way {
        findings.retain(|f| f.way == w);
    }

    if write {
        append_to_ledger(&findings)?;
    }

    if json_out {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }

    render_table(&findings, "Assembled Findings");
    let outcome = findings.iter().filter(|f| f.criterion.is_some()).count();
    println!();
    println!(
        "  \x1b[2m{} finding(s): {} outcome-tier (assessable), {} process-tier. \
         Determination is unset — a classifier labels it (ADR-201).\x1b[0m",
        findings.len(),
        outcome,
        findings.len() - outcome,
    );
    if write {
        println!("  \x1b[2mAppended to {}\x1b[0m", paths::findings_ledger().display());
    }
    Ok(())
}

/// Read the finding ledger back and display it.
pub fn list(json_out: bool) -> Result<()> {
    let path = paths::findings_ledger();
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let findings: Vec<Finding> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    if json_out {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }

    if findings.is_empty() {
        println!();
        println!("  \x1b[2mNo findings assembled yet. Run `ways-audit assemble --write`.\x1b[0m");
        return Ok(());
    }
    render_table(&findings, "Finding Ledger");
    let labeled = findings.iter().filter(|f| f.determination.is_some()).count();
    println!();
    println!(
        "  \x1b[2m{} finding(s), {} labeled by a classifier, {} awaiting classification.\x1b[0m",
        findings.len(),
        labeled,
        findings.len() - labeled,
    );
    Ok(())
}

fn render_table(findings: &[Finding], title: &str) {
    println!();
    println!("\x1b[1m{title}\x1b[0m");
    println!();
    println!(
        "  \x1b[1m{:<26} {:<26} {:<8} {:>5}  DETERMINATION\x1b[0m",
        "WAY", "CONTROL", "TIER", "FIRES"
    );
    println!(
        "  \x1b[2m{:<26} {:<26} {:<8} {:>5}  -------------\x1b[0m",
        "---", "-------", "----", "-----"
    );
    for f in findings {
        let control = if f.control.len() > 26 { &f.control[..26] } else { &f.control };
        let tier = match f.tier {
            ways_core::finding::Tier::Process => "process",
            ways_core::finding::Tier::Outcome => "outcome",
        };
        let det = match &f.determination {
            Some(d) => d.as_str(),
            None => "\x1b[2m(unset)\x1b[0m",
        };
        println!(
            "  {:<26} {:<26} {:<8} {:>5}  {}",
            f.way, control, tier, f.evidence.total, det
        );
    }
}

fn append_to_ledger(findings: &[Finding]) -> Result<()> {
    let path = paths::findings_ledger();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    for f in findings {
        writeln!(file, "{}", serde_json::to_string(f)?)?;
    }
    Ok(())
}
