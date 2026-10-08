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
/// The single-vector view, shown only as `run_late`'s fallback when
/// late-interaction cannot run.
fn run(query: String) -> Result<()> {
    let scores = super::scan::batch_embed_score_with(&query, None);

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

    // Descriptions come from the English corpus beside the one being scored, so
    // an isolated corpus describes its own ways. The canonical file covers the
    // case where that sibling is absent.
    let canonical = crate::paths::corpus_dir();
    let mut en_corpus = super::scan::sibling_corpus(None, &canonical, "ways-corpus-en.jsonl");
    if !en_corpus.is_file() {
        en_corpus = canonical.join("ways-corpus-en.jsonl");
    }
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
///
/// The ways that compete are those a prompt scan in `agent` scope competes, so the
/// shares match the live fire path; `unfiltered` competes every way. Admission
/// follows `matching.admission` as a scan in `project` would read it (the
/// current directory's project when none is given).
///
/// `json` prints one object with every candidate, not the top 20: `reduced`,
/// `admission`, and `rows` of `{id, peak, share, won_chunk, confirm, admitted,
/// capped, fired}`. When late interaction cannot run it prints `{"reduced": null, ...}`
/// with no rows, in place of the single-vector view.
pub fn run_late(query: String, project: Option<&str>, unfiltered: bool, json: bool) -> Result<()> {
    let top_n = if json { usize::MAX } else { 20 };
    let (admission, body_rank) = match project {
        Some(dir) => {
            let cfg = crate::config::Config::load(dir);
            (cfg.admission, cfg.body_rank)
        }
        None => (crate::config::global().admission, crate::config::global().body_rank),
    };
    let diag = crate::cmd::scan::diagnose(&query, project, top_n, unfiltered, admission, body_rank);
    if json {
        let (reduced, rows) = match diag {
            Some((reduced, rows)) => (Some(reduced), rows),
            None => (None, Vec::new()),
        };
        let rows: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id, "peak": r.peak, "share": r.share, "won_chunk": r.won_chunk,
                    "confirm": r.confirm, "admitted": r.admitted, "capped": r.capped, "fired": r.fired,
                })
            })
            .collect();
        let out = serde_json::json!({ "reduced": reduced, "admission": admission.as_str(), "rows": rows });
        println!("{out}");
        return Ok(());
    }
    let Some((reduced, rows)) = diag else {
        eprintln!(
            "late-interaction unavailable for this query (surface too sparse to chunk, \
             or the embedding engine is not set up) — showing the single-vector view.\n"
        );
        return run(query);
    };

    // Hand-format: `agent_fmt::Table` shrinks columns to the terminal width when
    // piped, ellipsizing the scores to `0.5…` — useless for a diagnostic. Fixed
    // columns keep full precision; only the won-chunk (last) column is bounded.
    let rule = crate::cmd::scan::admission_rule(admission);
    let peak_gate = crate::cmd::scan::DIAG_PEAK_GATE;
    let confirm_gate = crate::cmd::scan::DIAG_CONFIRM_GATE;
    let fired_n = rows.iter().filter(|r| r.fired).count();

    println!();
    println!(
        "late-interaction (ADR-160) · admit: {rule} OR peak ≥ {peak_gate:.2} · confirm ≥ {confirm_gate:.2} · {fired_n} would fire"
    );
    println!("reduced surface: {reduced}");
    println!();
    println!("  {:<34}  {:>5}  {:>5}  {:>7}  {:<8}  won chunk", "way", "peak", "share", "confirm", "outcome");
    println!("  {}", "─".repeat(96));

    for r in rows {
        let confirm = match r.confirm {
            Some(c) => format!("{c:.3}"),
            None => "  —  ".to_string(), // not admitted → confirmation not run
        };
        // Annotate why a candidate did not fire, so authoring is actionable:
        // admitted by neither the rule nor peak, passed but cut by the cap, or
        // admitted but the body failed to corroborate the won chunk.
        let outcome = if r.fired {
            "fired ✓"
        } else if r.capped {
            "< cap"
        } else if !r.admitted {
            "< gate"
        } else {
            "< confirm"
        };
        let id = agent_fmt::truncate_visible(&r.id, 34);
        let chunk = agent_fmt::truncate_visible(&r.won_chunk, 46);
        println!(
            "  {id:<34}  {:>5.3}  {:>5.3}  {confirm:>7}  {outcome:<8}  {chunk}",
            r.peak, r.share
        );
    }

    println!();
    println!("fired ✓ = admitted and confirmed · '< gate' = neither the admission rule nor peak · '< cap' = cut by the cap of 6 · '< confirm' = the body did not corroborate");
    println!("(ranked by share, the summed softmax mass per chunk; peak is the way's strongest single-chunk cosine)");
    println!();
    Ok(())
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

