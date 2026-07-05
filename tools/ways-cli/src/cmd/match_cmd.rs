use anyhow::Result;
use std::collections::HashMap;

use agent_fmt::{Align, Table};

/// (max EN score, max multi score) for a single way.
type ScorePair = (Option<f64>, Option<f64>);
type Row = (String, ScorePair);

/// Embedding-only match (ADR-125).
///
/// Shows both EN and multi-model scores when each way is present in the
/// results, so users diagnosing a match can see which model carried the
/// signal. Rows are ranked by the **EN score** — the English anchor every
/// way has — so an English-only way is never buried by a slightly higher
/// multi score on another way (ADR-139). The multi column is a diagnostic
/// display, dormant on English installs (all "—"), populated only when an
/// adopter has localized via ways-localize.
pub fn run(query: String, _corpus: Option<String>) -> Result<()> {
    let scores = super::scan::batch_embed_score(&query);

    if !scores.any_ran() {
        eprintln!("ERROR: embedding engine unavailable.");
        eprintln!(
            "       Run: cd {} && make setup",
            crate::paths::data_root().display()
        );
        std::process::exit(1);
    }

    // Collect max score per (way, model).
    let mut rows: HashMap<String, (Option<f64>, Option<f64>)> = HashMap::new();
    if let Some(en) = scores.en.as_deref() {
        for (id, s) in en {
            let entry = rows.entry(id.clone()).or_insert((None, None));
            entry.0 = Some(entry.0.map_or(*s, |existing: f64| existing.max(*s)));
        }
    }
    if let Some(mu) = scores.multi.as_deref() {
        for (id, s) in mu {
            let entry = rows.entry(id.clone()).or_insert((None, None));
            entry.1 = Some(entry.1.map_or(*s, |existing: f64| existing.max(*s)));
        }
    }

    if rows.is_empty() {
        eprintln!("no matches above threshold");
        std::process::exit(1);
    }

    // Display order: rank by the EN score, the anchor every way has. The
    // multi column is shown for diagnostics but does not rank the list, so
    // an English-only way is never buried by a slightly higher multi score
    // on another way (ADR-139). Falls back to multi only if a way somehow
    // lacks an EN score (e.g. a locale alias with no English corpus entry).
    let mut sorted: Vec<Row> = rows.into_iter().collect();
    sorted.sort_by(|a, b| {
        let key = |pair: &ScorePair| pair.0.or(pair.1).unwrap_or(f64::NEG_INFINITY);
        key(&b.1).partial_cmp(&key(&a.1)).unwrap_or(std::cmp::Ordering::Equal)
    });

    let en_corpus = crate::paths::corpus_dir().join("ways-corpus-en.jsonl");
    let descriptions = load_descriptions(en_corpus.to_str().unwrap_or(""));

    let mut t = Table::new(&["Way", "EN", "Multi", "Description"]);
    t.align(1, Align::Right);
    t.align(2, Align::Right);
    t.max_width(0, 38);
    t.max_width(3, 44);

    for (id, (en_s, mu_s)) in sorted.into_iter().take(25) {
        let desc = descriptions.get(&id).cloned().unwrap_or_default();
        t.add_owned(vec![
            id.clone(),
            en_s.map_or("—".to_string(), |s| format!("{s:.4}")),
            mu_s.map_or("—".to_string(), |s| format!("{s:.4}")),
            desc,
        ]);
    }

    println!();
    t.print();
    println!();
    Ok(())
}

/// Late-interaction diagnostic (ADR-160) — the authoring view that reflects the
/// live fire path, replacing the single-vector cosine table for way authoring.
///
/// For each top candidate it shows the matcher's actual decision quantities: the
/// **peak** per-chunk cosine (specificity), the summed-**share** (what the share
/// gate reads), the **body-confirm** (winning chunk vs the way's own body prose),
/// and whether the way **fired** — plus the surface chunk it won on, so an author
/// can see the exact evidence a fire hinges on. Falls back to the single-vector
/// view when late-interaction cannot run (sparse surface / no engine), mirroring
/// production's fail-safe.
pub fn run_late(query: String, project: Option<&str>) -> Result<()> {
    const TOP_N: usize = 20;
    let Some((reduced, rows)) = crate::cmd::scan::diagnose(&query, project, TOP_N) else {
        eprintln!(
            "late-interaction unavailable for this query (surface too sparse to chunk, \
             or the embedding engine is not set up) — showing the single-vector view.\n"
        );
        return run(query, None);
    };

    // Hand-format: `agent_fmt::Table` shrinks columns to the terminal width when
    // piped, ellipsizing the scores to `0.5…` — useless for a diagnostic. Fixed
    // columns keep full precision; only the won-chunk (last) column is bounded.
    let share_gate = crate::cmd::scan::DIAG_SHARE_GATE;
    let confirm_gate = crate::cmd::scan::DIAG_CONFIRM_GATE;
    let fired_n = rows.iter().filter(|r| r.fired).count();

    println!();
    println!(
        "late-interaction (ADR-160) · share gate ≥ {share_gate:.2} · confirm gate ≥ {confirm_gate:.2} · {fired_n} would fire"
    );
    println!("reduced surface: {reduced}");
    println!();
    println!("  {:<34}  {:>5}  {:>5}  {:>7}  {:<8}  won chunk", "way", "peak", "share", "confirm", "outcome");
    println!("  {}", "─".repeat(96));

    for r in rows {
        let confirm = match r.confirm {
            Some(c) => format!("{c:.3}"),
            None => "  —  ".to_string(), // below share gate → confirmation not run
        };
        // Annotate why a candidate did not fire, so authoring is actionable.
        let outcome = if r.fired {
            "fired ✓"
        } else if r.share < share_gate {
            "< share"
        } else {
            "< confirm"
        };
        let id = truncate_col(&r.id, 34);
        let chunk = truncate_col(&r.won_chunk, 46);
        println!(
            "  {id:<34}  {:>5.3}  {:>5.3}  {confirm:>7}  {outcome:<8}  {chunk}",
            r.peak, r.share
        );
    }

    println!();
    println!("fired ✓ = cleared both gates · '< share'/'< confirm' = fell short of that gate");
    println!("(ranked by share, the share-gate quantity; peak is the way's strongest single-chunk cosine)");
    println!();
    Ok(())
}

/// Truncate a display column on a char boundary, appending `…` when cut.
fn truncate_col(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let head: String = s.chars().take(width.saturating_sub(1)).collect();
    format!("{head}…")
}

fn load_descriptions(corpus_path: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(content) = std::fs::read_to_string(corpus_path) {
        for line in content.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(id), Some(desc)) = (
                    v.get("id").and_then(|v| v.as_str()),
                    v.get("description").and_then(|v| v.as_str()),
                ) {
                    map.entry(id.to_string()).or_insert_with(|| desc.to_string());
                }
            }
        }
    }
    map
}

