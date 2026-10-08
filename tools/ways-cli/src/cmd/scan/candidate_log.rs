//! scan/candidate_log.rs — a scan's ranked candidates (ADR-701 §2, §4).
//!
//! Every prompt and task scan records its top candidates with cosine, share and
//! margin in its decision record (scan/decision.rs), so the run logs show how a
//! fire was won or missed and not only that it happened. The rows are the ones
//! the scan already holds: `way-embed match --threshold 0.0` returns every
//! way's cosine, so this adds no embedding work.
//!
//! Only enabled ways appear, and only enabled ways compete in the share: a way
//! the project or user switched off is not a neighbour (ADR-701 §1).

use std::collections::HashMap;

/// Softmax temperature for share. The same τ the late-interaction matcher uses.
pub(super) const SHARE_TAU: f64 = super::late_interaction::SOFTMAX_TAU;
/// Candidates entering the share softmax.
pub(super) const SHARE_WINDOW: usize = super::late_interaction::TOP_K_PER_CHUNK;
/// Candidates written to the log.
pub(super) const LOGGED: usize = 5;

/// One logged candidate.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Candidate {
    /// Bare way id.
    pub way: String,
    pub cosine: f64,
    /// Softmax share over the top [`SHARE_WINDOW`] enabled candidates.
    pub share: f64,
    /// Cosine gap to the next enabled candidate; `None` for the last one.
    pub margin: Option<f64>,
}

/// The top [`LOGGED`] enabled candidates of one lane.
///
/// `rows` are `(corpus_id, cosine)` as way-embed prints them; a way appearing
/// more than once keeps its best cosine. `enabled` maps a corpus id to the way's
/// bare id and is the only set that competes. Ties order by id so the output is
/// stable.
pub(super) fn top_candidates(rows: &[(String, f64)], enabled: &HashMap<&str, &str>) -> Vec<Candidate> {
    ranked_candidates(rows, enabled, LOGGED)
}

/// The top `limit` enabled candidates, with share and margin defined as in
/// [`top_candidates`]: the softmax and the margin never depend on `limit`, so a
/// lookup that asks for more rows reads the same measures the log records.
pub(super) fn ranked_candidates(rows: &[(String, f64)], enabled: &HashMap<&str, &str>, limit: usize) -> Vec<Candidate> {
    let mut best: HashMap<&str, f64> = HashMap::new();
    for (id, cos) in rows {
        if let Some(way) = enabled.get(id.as_str()) {
            let e = best.entry(way).or_insert(f64::MIN);
            if *cos > *e {
                *e = *cos;
            }
        }
    }
    let mut ranked: Vec<(&str, f64)> = best.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(b.0)));

    let window = &ranked[..ranked.len().min(SHARE_WINDOW)];
    let denom: f64 = window.iter().map(|(_, c)| (c / SHARE_TAU).exp()).sum();
    ranked
        .iter()
        .enumerate()
        .take(limit)
        .map(|(i, (way, cos))| Candidate {
            way: (*way).to_string(),
            cosine: *cos,
            share: if denom > 0.0 { (cos / SHARE_TAU).exp() / denom } else { 0.0 },
            margin: ranked.get(i + 1).map(|(_, next)| cos - next),
        })
        .collect()
}

/// The lane the scan's candidates come from. The multilingual lane runs only in
/// localized mode (ADR-139) and is the lane that mode matches in, so it wins
/// when it ran; otherwise the English lane. `None` when neither ran.
pub(super) fn scan_lane(scores: &super::scoring::EmbedScores) -> Option<(&'static str, &[(String, f64)])> {
    match (&scores.multi, &scores.en) {
        (Some(rows), _) => Some(("multi", rows)),
        (None, Some(rows)) => Some(("en", rows)),
        (None, None) => None,
    }
}

/// A record's `candidates` value: one object per candidate. Lane, basis and
/// sidecar are properties of the scan and sit once on the record.
pub(super) fn candidates_json(cands: &[Candidate]) -> serde_json::Value {
    let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
    serde_json::Value::Array(
        cands
            .iter()
            .map(|c| {
                serde_json::json!({
                    "way": c.way,
                    "cosine": round(c.cosine),
                    "share": round(c.share),
                    "margin": c.margin.map(round),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(i, c)| (i.to_string(), *c)).collect()
    }

    fn all(ids: &[&'static str]) -> HashMap<&'static str, &'static str> {
        ids.iter().map(|i| (*i, *i)).collect()
    }

    #[test]
    fn share_is_a_softmax_at_tau_over_the_enabled_candidates() {
        let got = top_candidates(&rows(&[("a", 0.6), ("b", 0.5)]), &all(&["a", "b"]));
        let (ea, eb) = ((0.6f64 / 0.08).exp(), (0.5f64 / 0.08).exp());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].way, "a");
        assert!((got[0].share - ea / (ea + eb)).abs() < 1e-12);
        assert!((got[1].share - eb / (ea + eb)).abs() < 1e-12);
        assert!((got[0].share + got[1].share - 1.0).abs() < 1e-12);
    }

    #[test]
    fn margin_is_the_gap_to_the_next_enabled_candidate() {
        let got = top_candidates(&rows(&[("a", 0.6), ("b", 0.5), ("c", 0.2)]), &all(&["a", "b", "c"]));
        assert!((got[0].margin.unwrap() - 0.1).abs() < 1e-12);
        assert!((got[1].margin.unwrap() - 0.3).abs() < 1e-12);
        assert_eq!(got[2].margin, None, "nothing follows the last candidate");
    }

    #[test]
    fn a_disabled_way_is_no_neighbour() {
        // "off" outranks everything but is not enabled: it takes no share and
        // is not the next candidate whose gap a margin measures.
        let r = rows(&[("off", 0.9), ("a", 0.6), ("b", 0.5)]);
        let with = top_candidates(&r, &all(&["a", "b"]));
        let without = top_candidates(&rows(&[("a", 0.6), ("b", 0.5)]), &all(&["a", "b"]));
        assert_eq!(with.len(), 2);
        assert_eq!(with, without);
        assert!(with.iter().all(|c| c.way != "off"));
    }

    #[test]
    fn the_log_holds_five_and_the_share_window_eight() {
        let ids: Vec<String> = (0..10).map(|i| format!("w{i}")).collect();
        let r: Vec<(String, f64)> = ids.iter().enumerate().map(|(i, id)| (id.clone(), 0.9 - i as f64 * 0.05)).collect();
        let enabled: HashMap<&str, &str> = ids.iter().map(|i| (i.as_str(), i.as_str())).collect();
        let got = top_candidates(&r, &enabled);
        assert_eq!(got.len(), 5);
        // Share sums to 1 over eight, so five of them hold less than 1.
        let five: f64 = got.iter().map(|c| c.share).sum();
        assert!(five < 1.0 && five > 0.5);
        let w: f64 = r[..8].iter().map(|(_, c)| (c / 0.08).exp()).sum();
        assert!((got[0].share - (0.9f64 / 0.08).exp() / w).abs() < 1e-12);
    }

    #[test]
    fn a_way_seen_twice_keeps_its_best_cosine_and_maps_to_its_bare_id() {
        let mut enabled = HashMap::new();
        enabled.insert("proj/a", "a");
        let got = top_candidates(&rows(&[("proj/a", 0.3), ("proj/a", 0.7)]), &enabled);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].way, "a");
        assert!((got[0].cosine - 0.7).abs() < 1e-12);
    }

    fn scores(en: Option<Vec<(String, f64)>>, multi: Option<Vec<(String, f64)>>) -> super::super::scoring::EmbedScores {
        super::super::scoring::EmbedScores { en, multi, calibration: Default::default() }
    }

    #[test]
    fn the_lane_is_multi_when_it_ran_else_en_else_none() {
        let en = Some(rows(&[("a", 0.5)]));
        let multi = Some(rows(&[("a", 0.4)]));
        assert_eq!(scan_lane(&scores(en.clone(), multi.clone())).unwrap().0, "multi");
        assert_eq!(scan_lane(&scores(en, None)).unwrap().0, "en");
        assert!(scan_lane(&scores(None, None)).is_none());
    }

    #[test]
    fn json_carries_the_measures_and_nothing_per_scan() {
        let got = vec![
            Candidate { way: "a".into(), cosine: 0.6, share: 0.7, margin: Some(0.1) },
            Candidate { way: "b".into(), cosine: 0.5, share: 0.3, margin: None },
        ];
        let v = candidates_json(&got);
        assert_eq!(v[0]["way"], "a");
        assert!(v[0].get("lane").is_none() && v[0].get("sidecar").is_none());
        assert_eq!(v[0]["cosine"], 0.6);
        assert!(v[1]["margin"].is_null());
    }
}
