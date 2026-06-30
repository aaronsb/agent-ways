//! Tune way firing-dynamics curves from observed cadence (ADR-123 Phase E).
//!
//! Surveys `~/.claude/stats/events.jsonl`, groups `way_fired` events by
//! (way, session), computes token-position deltas between consecutive fires,
//! and suggests a calibrated `half_life` per way grounded in real usage.
//!
//! Dry-run by default. `--apply` rewrites `curve:` blocks in each way's
//! frontmatter via line surgery — no full YAML reformat.
//!
//! Scope note: v1 only suggests `Curve::Exponential` shapes. The migrated
//! corpus is nearly homogeneous on Exponential, and deriving ActionPotential
//! parameters from cadence alone is under-specified without additional
//! burst-shape analysis. Future revisions can layer in ActionPotential for
//! ways whose delta distribution shows clear bi-modality.

use anyhow::{Context, Result};
use sensor_trait::Curve;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_fmt::{Align, Table};

use crate::frontmatter;
use crate::session::resolve_way_file;

/// Rule-of-thumb: suggest a half_life equal to the median observed token
/// delta between fires, rounded to the nearest 500 tokens for readability.
const ROUND_TO: u64 = 500;

/// Skip suggestions whose ratio to the current half_life falls inside this
/// band. Small tweaks generate more churn than signal.
const TOLERANCE: f64 = 0.20;

pub fn run(
    apply: bool,
    min_fires: usize,
    project_filter: Option<String>,
    way_filter: Option<String>,
) -> Result<()> {
    let events_path = crate::paths::events_log();
    if !events_path.is_file() {
        println!("no events log found at {}", events_path.display());
        println!("ways tune-curves needs real firing data — run ways for a few sessions first.");
        return Ok(());
    }

    let fires = load_fires(&events_path, project_filter.as_deref(), way_filter.as_deref())
        .with_context(|| format!("reading {}", events_path.display()))?;

    if fires.is_empty() {
        println!("no way_fired events with token_position found in the selected window.");
        println!("token_position was added to the event log in commit 8b20782 — older fires");
        println!("are not usable for cadence tuning. Let a few sessions accumulate, then re-run.");
        return Ok(());
    }

    let cadences = compute_cadences(&fires, min_fires);

    if cadences.is_empty() {
        println!("no ways have ≥{min_fires} same-session delta samples yet.");
        println!("run more sessions, or lower the floor with --min-fires N (not recommended below 3).");
        return Ok(());
    }

    let mut suggestions = Vec::with_capacity(cadences.len());
    for c in &cadences {
        let current = load_current_curve(&c.way_id);
        suggestions.push(build_suggestion(c, current));
    }

    render_table(&suggestions);

    let changed: Vec<&Suggestion> = suggestions.iter().filter(|s| s.changed).collect();
    let within_tol: usize = suggestions.len() - changed.len();

    println!();
    println!(
        "  {} ways analyzed · {} suggested changes · {} within ±{:.0}% of current",
        suggestions.len(),
        changed.len(),
        within_tol,
        TOLERANCE * 100.0,
    );

    if changed.is_empty() {
        println!("  no curve updates needed.");
        return Ok(());
    }

    if apply {
        let mut wrote = 0;
        for s in &changed {
            match apply_one(s) {
                Ok(path) => {
                    println!("  wrote {}", path.display());
                    wrote += 1;
                }
                Err(e) => eprintln!("  ! {}: {}", s.way_id, e),
            }
        }
        println!();
        println!("  applied {wrote}/{} suggested changes.", changed.len());
    } else {
        println!("  (pass --apply to rewrite the suggested curve blocks)");
    }

    Ok(())
}

/// One `way_fired` event with its token position.
#[derive(Debug, Clone)]
struct Fire {
    way: String,
    session: String,
    token_position: u64,
}

/// Pooled statistics for a single way across all sessions in the window.
#[derive(Debug, Clone)]
struct WayCadence {
    way_id: String,
    /// Total delta samples (consecutive same-session fires) across all
    /// sessions. `sample_count >= min_fires` is the inclusion floor.
    sample_count: usize,
    median: u64,
    p90: u64,
}

#[derive(Debug, Clone)]
struct Suggestion {
    way_id: String,
    cadence: WayCadence,
    current: Option<Curve>,
    suggested: Curve,
    change_pct: Option<f64>,
    changed: bool,
}

fn load_fires(
    path: &Path,
    project_filter: Option<&str>,
    way_filter: Option<&str>,
) -> Result<Vec<Fire>> {
    let content = std::fs::read_to_string(path)?;
    let mut fires = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Both way_fired (first in session) and way_redisclosed (every
        // subsequent re-fire) represent "way was shown to the model at this
        // tick." Deltas between consecutive events in the same session are
        // the re-fire cadence we're tuning against.
        match row.get("event").and_then(|v| v.as_str()) {
            Some("way_fired") | Some("way_redisclosed") => {}
            _ => continue,
        }

        let token_position = match row.get("token_position").and_then(|v| v.as_str()) {
            Some(s) => s.parse::<u64>().unwrap_or(0),
            None => continue,
        };
        if token_position == 0 {
            continue;
        }

        let way = match row.get("way").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        if let Some(pat) = way_filter {
            if !way.contains(pat) {
                continue;
            }
        }

        if let Some(pat) = project_filter {
            match row.get("project").and_then(|v| v.as_str()) {
                Some(p) if p.contains(pat) => {}
                _ => continue,
            }
        }

        let session = row
            .get("session")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if session.is_empty() {
            continue;
        }

        fires.push(Fire { way, session, token_position });
    }

    Ok(fires)
}

/// Compute per-way delta statistics. Deltas are only meaningful within a
/// single session (each session has its own monotonic token axis), so
/// fires from different sessions are never paired.
fn compute_cadences(fires: &[Fire], min_fires: usize) -> Vec<WayCadence> {
    // (way, session) → sorted positions
    let mut grouped: BTreeMap<(String, String), Vec<u64>> = BTreeMap::new();
    for f in fires {
        grouped
            .entry((f.way.clone(), f.session.clone()))
            .or_default()
            .push(f.token_position);
    }

    // way → pooled deltas across sessions
    let mut per_way: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for ((way, _session), mut positions) in grouped {
        positions.sort_unstable();
        positions.dedup();
        if positions.len() < 2 {
            continue;
        }
        let deltas = per_way.entry(way).or_default();
        for w in positions.windows(2) {
            let d = w[1] - w[0];
            if d > 0 {
                deltas.push(d);
            }
        }
    }

    let mut out = Vec::new();
    for (way_id, mut deltas) in per_way {
        if deltas.len() < min_fires {
            continue;
        }
        deltas.sort_unstable();
        let median = percentile(&deltas, 50);
        let p90 = percentile(&deltas, 90);
        out.push(WayCadence {
            way_id,
            sample_count: deltas.len(),
            median,
            p90,
        });
    }

    // Sort by biggest impact first: largest relative change would go at top,
    // but we don't know current curves at this layer. Sort by sample_count
    // descending — higher confidence first.
    out.sort_by(|a, b| b.sample_count.cmp(&a.sample_count));
    out
}

fn percentile(sorted: &[u64], p: u32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    // Floor-indexed nearest-rank: idx = floor((p/100) × (n-1)). For small
    // samples this picks the lower of the two bracketing values, matching
    // the common "lower median" convention.
    let idx = ((p as u64) * (sorted.len() as u64 - 1) / 100) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn round_to(value: u64, unit: u64) -> u64 {
    if unit == 0 {
        return value;
    }
    ((value + unit / 2) / unit) * unit
}

fn build_suggestion(c: &WayCadence, current: Option<Curve>) -> Suggestion {
    let suggested_half_life = round_to(c.median, ROUND_TO).max(ROUND_TO);
    let suggested = Curve::Exponential { half_life: suggested_half_life };

    let (change_pct, changed) = match &current {
        Some(Curve::Exponential { half_life }) if *half_life > 0 => {
            let pct = (suggested_half_life as f64 - *half_life as f64) / (*half_life as f64);
            let within = pct.abs() < TOLERANCE;
            (Some(pct), !within)
        }
        // Non-Exponential current curves: always flag as changed so the
        // operator sees the mismatch, but the suggestion is still Exponential.
        Some(_) => (None, true),
        // No current curve parsed (unlikely post-ADR-123 migration but
        // possible for project-local ways or malformed files): suggest.
        None => (None, true),
    };

    Suggestion {
        way_id: c.way_id.clone(),
        cadence: c.clone(),
        current,
        suggested,
        change_pct,
        changed,
    }
}

fn load_current_curve(way_id: &str) -> Option<Curve> {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    let (path, _) = resolve_way_file(way_id, &project_dir)?;
    frontmatter::parse(&path).ok().and_then(|f| f.curve)
}

fn render_table(suggestions: &[Suggestion]) {
    let mut t = Table::new(&["Way", "Fires", "Median", "p90", "Current", "Suggested", "Δ"]);
    t.max_width(0, 36);
    t.align(1, Align::Right);
    t.align(2, Align::Right);
    t.align(3, Align::Right);
    t.align(6, Align::Right);

    for s in suggestions {
        let fires = s.cadence.sample_count.to_string();
        let median = format_tokens(s.cadence.median);
        let p90 = format_tokens(s.cadence.p90);
        let current = describe_curve(s.current.as_ref());
        let suggested = describe_curve(Some(&s.suggested));
        let delta = match s.change_pct {
            Some(pct) if s.changed => format!("{:+.0}%", pct * 100.0),
            Some(_) => "-".to_string(),
            None if s.changed => "new".to_string(),
            None => "-".to_string(),
        };

        t.add_owned(vec![
            s.way_id.clone(),
            fires,
            median,
            p90,
            current,
            suggested,
            delta,
        ]);
    }

    println!();
    t.print();
}

fn format_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn describe_curve(c: Option<&Curve>) -> String {
    match c {
        Some(Curve::Exponential { half_life }) => format!("Exp h={}", format_tokens(*half_life)),
        Some(Curve::ActionPotential { .. }) => "ActionPotential".to_string(),
        Some(Curve::ProgressiveStaircase { .. }) => "Staircase".to_string(),
        Some(Curve::Flat { suppression }) => format!("Flat s={}", format_tokens(*suppression)),
        None => "-".to_string(),
    }
}

fn apply_one(s: &Suggestion) -> Result<PathBuf> {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    let (path, _) = resolve_way_file(&s.way_id, &project_dir)
        .ok_or_else(|| anyhow::anyhow!("could not locate way file"))?;

    let content = std::fs::read_to_string(&path)?;
    let new_block = render_curve_yaml(&s.suggested);
    let updated = replace_curve_block(&content, &new_block)
        .ok_or_else(|| anyhow::anyhow!("no curve: block found in frontmatter"))?;
    write_atomic(&path, &updated)?;
    Ok(path)
}

/// Write-to-temp-and-rename. Prevents half-written files if the process
/// is killed mid-write during a multi-file `--apply` run. `rename` within
/// the same directory is atomic on POSIX filesystems.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("path has no filename"))?;
    let tmp = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    std::fs::write(&tmp, content).with_context(|| format!("writing temp file {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming temp file to {}", path.display()))?;
    Ok(())
}

fn render_curve_yaml(c: &Curve) -> Vec<String> {
    // tune_curves only produces Exponential suggestions. Other variants are
    // unreachable from the suggestion path; if that changes, add them back
    // alongside the build_suggestion change that motivates them.
    match c {
        Curve::Exponential { half_life } => vec![
            "curve:".to_string(),
            "  type: Exponential".to_string(),
            format!("  half_life: {half_life}"),
        ],
        _ => unreachable!("tune_curves only suggests Curve::Exponential"),
    }
}

/// Line-surgery replacement of the frontmatter `curve:` block. Preserves
/// all surrounding YAML verbatim. Returns `None` if no `curve:` block is
/// found inside the frontmatter bounds.
fn replace_curve_block(content: &str, new_block: &[String]) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.first() != Some(&"---") {
        return None;
    }
    let close_idx = lines.iter().skip(1).position(|l| *l == "---")? + 1;

    // Find `curve:` at the top of the frontmatter (not inside a nested
    // mapping — same "no indentation" discipline as remove_top_level_field).
    let mut start = None;
    for (i, line) in lines.iter().enumerate().take(close_idx).skip(1) {
        if line.starts_with("curve:") {
            start = Some(i);
            break;
        }
    }
    let start = start?;

    // Consume indented continuation.
    let mut end = start + 1;
    while end < close_idx {
        let line = lines[end];
        if line.starts_with(' ') || line.starts_with('\t') || line.is_empty() {
            end += 1;
        } else {
            break;
        }
    }
    while end > start + 1 && lines[end - 1].is_empty() {
        end -= 1;
    }

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + new_block.len());
    out.extend(lines[..start].iter().map(|s| s.to_string()));
    out.extend(new_block.iter().cloned());
    out.extend(lines[end..].iter().map(|s| s.to_string()));

    let mut s = out.join("\n");
    if content.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_picks_sensible_index() {
        // 10 sorted values. Floor-indexed nearest-rank:
        //   p50 → floor(50 × 9 / 100) = 4 → sorted[4] = 50
        //   p90 → floor(90 × 9 / 100) = 8 → sorted[8] = 90
        let data = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentile(&data, 50), 50);
        assert_eq!(percentile(&data, 90), 90);
        assert_eq!(percentile(&data, 0), 10);
    }

    #[test]
    fn round_to_nearest_500() {
        assert_eq!(round_to(17_842, 500), 18_000);
        assert_eq!(round_to(17_250, 500), 17_500);
        assert_eq!(round_to(100, 500), 0);
    }

    #[test]
    fn compute_cadences_groups_by_session() {
        let fires = vec![
            Fire { way: "a".into(), session: "s1".into(), token_position: 1000 },
            Fire { way: "a".into(), session: "s1".into(), token_position: 21000 },
            Fire { way: "a".into(), session: "s2".into(), token_position: 500 },
            Fire { way: "a".into(), session: "s2".into(), token_position: 40500 },
            // different way, not enough samples
            Fire { way: "b".into(), session: "s1".into(), token_position: 2000 },
        ];
        let cad = compute_cadences(&fires, 2);
        assert_eq!(cad.len(), 1);
        assert_eq!(cad[0].way_id, "a");
        assert_eq!(cad[0].sample_count, 2);
        // Deltas: 20_000 (s1), 40_000 (s2). Median of two = p50 → index 0 → 20_000.
        assert_eq!(cad[0].median, 20_000);
    }

    #[test]
    fn replace_curve_block_round_trips() {
        let src = "---\ndescription: test way\ncurve:\n  type: Exponential\n  half_life: 30000\n---\nbody\n";
        let new_block = vec![
            "curve:".to_string(),
            "  type: Exponential".to_string(),
            "  half_life: 18000".to_string(),
        ];
        let out = replace_curve_block(src, &new_block).expect("curve block found");
        assert!(out.contains("half_life: 18000"));
        assert!(!out.contains("half_life: 30000"));
        assert!(out.contains("description: test way"));
        assert!(out.ends_with("body\n"));
    }

    #[test]
    fn replace_curve_block_missing_returns_none() {
        let src = "---\ndescription: no curve here\n---\nbody\n";
        let new_block = vec!["curve:".to_string(), "  type: Exponential".to_string(), "  half_life: 1000".to_string()];
        assert!(replace_curve_block(src, &new_block).is_none());
    }

    #[test]
    fn replace_curve_block_preserves_trailing_blank_before_next_field() {
        // A blank line that separates the curve block from a following
        // top-level key must be preserved — it wasn't part of the curve
        // block and removing it would churn the diff and coalesce two
        // logically separate sections.
        let src = "---\ndescription: test\ncurve:\n  type: Exponential\n  half_life: 30000\n\nthreshold: 2.0\n---\nbody\n";
        let new_block = vec![
            "curve:".to_string(),
            "  type: Exponential".to_string(),
            "  half_life: 18000".to_string(),
        ];
        let out = replace_curve_block(src, &new_block).expect("curve block found");
        // The blank line between curve and threshold must survive the edit.
        assert!(out.contains("half_life: 18000\n\nthreshold: 2.0"));
        assert!(!out.contains("half_life: 30000"));
    }

    #[test]
    fn build_suggestion_respects_tolerance() {
        let cadence = WayCadence {
            way_id: "x".into(),
            sample_count: 10,
            median: 27_500,
            p90: 45_000,
        };
        let current = Some(Curve::Exponential { half_life: 30_000 });
        let s = build_suggestion(&cadence, current);
        // 27_500 rounded to 500 = 27_500. Delta from 30_000 = -8.3% < 20% → unchanged.
        assert!(!s.changed);
    }

    #[test]
    fn build_suggestion_flags_meaningful_change() {
        let cadence = WayCadence {
            way_id: "y".into(),
            sample_count: 10,
            median: 18_000,
            p90: 40_000,
        };
        let current = Some(Curve::Exponential { half_life: 30_000 });
        let s = build_suggestion(&cadence, current);
        // 18_000 vs 30_000 = -40% > 20% → flagged.
        assert!(s.changed);
        match s.suggested {
            Curve::Exponential { half_life } => assert_eq!(half_life, 18_000),
            _ => panic!("expected Exponential suggestion, got {:?}", s.suggested),
        }
    }
}
