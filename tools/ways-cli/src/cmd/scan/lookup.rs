//! scan/lookup.rs — the candidate list behind `ways_search` (ADR-701 §5).
//!
//! The same ranking a prompt scan logs as `scan_candidates`: the prompt lane's
//! single-vector rows, restricted to the ways [`eligible`] for the session's
//! scope, with share and margin as `candidate_log` defines them. A lookup runs
//! no judge and fires nothing; the agent chose to ask.

use std::collections::HashMap;

use super::candidate_log::ranked_candidates;
use super::{batch_embed_score, candidate_log, collect_candidates, eligible, reduce, Lane, WayCandidate, BUDGET_PROMPT};
use crate::session;

/// One ranked way.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hit {
    /// Bare way id, the one `ways_read` takes.
    pub way: String,
    /// The way's place in the tree, domain to leaf.
    pub route: String,
    pub description: String,
    pub cosine: f64,
    pub share: f64,
    pub margin: Option<f64>,
    /// The body section that matched. `None` until the body sidecar exists
    /// (ADR-701 §6, increment 4); the output omits the field then.
    pub section: Option<String>,
}

/// What a search ran on.
pub(crate) struct Search {
    /// `en` or `multi`, the lane the rows came from.
    pub lane: &'static str,
    pub hits: Vec<Hit>,
}

/// Rank `query` against the ways eligible on the prompt lane for the session's
/// scope. `None` when no embedding lane ran, so there is nothing to rank.
pub(crate) fn search(query: &str, session_id: Option<&str>, project_dir: &str, top_n: usize) -> Option<Search> {
    let scope = session_id.map_or_else(|| "agent".to_string(), session::detect_scope);
    let candidates = collect_candidates(project_dir);
    let eligible_ways: Vec<&WayCandidate> =
        candidates.iter().filter(|c| c.embeddable() && eligible(c, Lane::Prompt { scope: &scope }, project_dir)).collect();
    let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
    let scores = batch_embed_score(&reduced);
    let (lane, rows) = candidate_log::scan_lane(&scores)?;
    Some(Search { lane, hits: hits_from_rows(rows, &eligible_ways, top_n) })
}

/// The ranked hits for `rows` over `ways`. A row naming a way outside `ways`
/// (disabled, out of scope, failing its `when:`) takes no share and is no one's
/// next candidate.
fn hits_from_rows(rows: &[(String, f64)], ways: &[&WayCandidate], top_n: usize) -> Vec<Hit> {
    let enabled: HashMap<&str, &str> = ways.iter().map(|c| (c.corpus_id.as_str(), c.id.as_str())).collect();
    let by_id: HashMap<&str, &&WayCandidate> = ways.iter().map(|c| (c.id.as_str(), c)).collect();
    ranked_candidates(rows, &enabled, top_n)
        .into_iter()
        .map(|c| Hit {
            description: by_id.get(c.way.as_str()).map(|w| w.description.clone()).unwrap_or_default(),
            route: route_of(&c.way),
            way: c.way,
            cosine: c.cosine,
            share: c.share,
            margin: c.margin,
            section: None,
        })
        .collect()
}

/// A way id read as a route: `softwaredev/code/quality` is
/// `softwaredev > code > quality`.
pub(crate) fn route_of(id: &str) -> String {
    id.split('/').collect::<Vec<_>>().join(" > ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way(id: &str, corpus_id: &str, description: &str) -> WayCandidate {
        WayCandidate {
            id: id.into(),
            corpus_id: corpus_id.into(),
            path: std::path::PathBuf::new(),
            pattern: None,
            pattern_strict: false,
            commands: None,
            files: None,
            description: description.into(),
            vocabulary: "v".into(),
            threshold: 0.0,
            scope: "agent".into(),
            when_project: None,
            when_file_exists: None,
            trigger: None,
            trigger_path: None,
        }
    }

    fn rows(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(i, c)| (i.to_string(), *c)).collect()
    }

    #[test]
    fn hits_carry_route_description_and_the_logged_measures() {
        let a = way("d/code/quality", "d/code/quality", "quality bar");
        let b = way("d/code/testing", "d/code/testing", "test craft");
        let r = rows(&[("d/code/quality", 0.6), ("d/code/testing", 0.5)]);
        let got = hits_from_rows(&r, &[&a, &b], 5);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].way, "d/code/quality");
        assert_eq!(got[0].route, "d > code > quality");
        assert_eq!(got[0].description, "quality bar");
        assert!((got[0].margin.unwrap() - 0.1).abs() < 1e-12);
        assert_eq!(got[1].margin, None);
        let enabled: HashMap<&str, &str> =
            [("d/code/quality", "d/code/quality"), ("d/code/testing", "d/code/testing")].into_iter().collect();
        let logged = candidate_log::top_candidates(&r, &enabled);
        assert_eq!(got[0].share, logged[0].share, "one definition of share for the log and the lookup");
        assert!(got.iter().all(|h| h.section.is_none()), "no sidecar, no section");
    }

    #[test]
    fn a_way_outside_the_eligible_set_takes_no_share_and_does_not_appear() {
        let a = way("d/a", "d/a", "");
        let b = way("d/b", "d/b", "");
        // "d/off" outranks both but is not eligible (disabled, wrong scope, failed `when:`).
        let got = hits_from_rows(&rows(&[("d/off", 0.9), ("d/a", 0.6), ("d/b", 0.5)]), &[&a, &b], 5);
        assert_eq!(got.iter().map(|h| h.way.as_str()).collect::<Vec<_>>(), ["d/a", "d/b"]);
        assert!((got[0].share + got[1].share - 1.0).abs() < 1e-12);
    }

    #[test]
    fn top_n_limits_the_list_not_the_share_window() {
        let ids: Vec<String> = (0..10).map(|i| format!("w{i}")).collect();
        let ways: Vec<WayCandidate> = ids.iter().map(|i| way(i, i, "")).collect();
        let refs: Vec<&WayCandidate> = ways.iter().collect();
        let r: Vec<(String, f64)> = ids.iter().enumerate().map(|(i, id)| (id.clone(), 0.9 - i as f64 * 0.05)).collect();
        let two = hits_from_rows(&r, &refs, 2);
        let nine = hits_from_rows(&r, &refs, 9);
        assert_eq!((two.len(), nine.len()), (2, 9));
        assert_eq!(two[0].share, nine[0].share);
    }

    #[test]
    fn a_project_way_maps_its_namespaced_corpus_id_to_the_bare_id() {
        let a = way("ops/safety", "-proj/ops/safety", "");
        let got = hits_from_rows(&rows(&[("-proj/ops/safety", 0.7)]), &[&a], 5);
        assert_eq!(got[0].way, "ops/safety");
    }
}
