use anyhow::Result;
use std::collections::HashMap;

use agent_fmt::{Align, Table};
use crate::util::xdg_cache_dir;

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
        eprintln!("       Run: cd ~/.claude && make setup");
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

    let en_corpus = xdg_cache_dir().join("agent-ways/user/ways-corpus-en.jsonl");
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

