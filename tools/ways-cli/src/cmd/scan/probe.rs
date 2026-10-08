//! Probe scans (ADR-701): what the prompt and Bash lanes decide for one piece of
//! text, with nothing carried between calls.
//!
//! A probe is a fresh session. It reads no refire stamps and no parent markers
//! (the session id names no state), runs no relevance judge, and shows no body,
//! so it writes no marker, log or decision record. Every decision comes from the
//! scan's own functions: candidate collection, eligibility, the late-interaction
//! matcher, the per-way outcome ([`super::prompt_outcome`]), the Bash lane's hits
//! ([`super::command_hits`]) and the admission order. Only the show step is left
//! out, and a parent-boost fire is withheld when no parent was shown, as it is
//! in the scan.

use std::collections::{HashMap, HashSet};

use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, EmbedScores};
use super::{
    body_map, collect_candidates, command_hits, eligible, has_shown_ancestor, late_interaction, mask_nonlinguistic,
    prompt_competitors, prompt_outcome, reduce, CommandSurface, Lane, PromptMatch, PromptSurface, WayCandidate,
    BUDGET_COMMAND, BUDGET_PROMPT,
};
use crate::config::Admission;

/// A probe's session id. It names no session, so no marker answers to it.
const PROBE_SESSION: &str = "ways-probe-fresh";

/// One competing way's standing in a probe.
pub(crate) struct ProbeRow {
    pub id: String,
    /// The ranking quantity: the summed softmax share on the late-interaction
    /// path, the calibrated probability on the single-vector path.
    pub score: f64,
    /// Strongest single-chunk cosine; late-interaction path only.
    pub peak: Option<f64>,
    /// Body confirmation; late-interaction path, admitted ways only.
    pub confirm: Option<f64>,
}

/// What one probe found.
pub(crate) struct ProbeScan {
    /// Competing ways, best first (ties by id).
    pub rows: Vec<ProbeRow>,
    /// Ways whose body the scan would have shown, in admission order, each
    /// with the channel that fired it.
    pub fired: Vec<(String, String)>,
    /// Every way id the scan knows, with the stage that decided it.
    pub stages: HashMap<String, &'static str>,
    /// The late-interaction matcher ran. False means the single-vector
    /// fail-safe decided.
    pub late: bool,
    /// Some way fired only on a parent boost from a way fired in this probe.
    pub boost_exercised: bool,
}

/// Probe the prompt lane with `query`.
pub(crate) fn prompt(query: &str, project_dir: &str, admission: Admission) -> ProbeScan {
    let scope = "agent";
    let candidates = collect_candidates(project_dir);
    let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
    let embed_matches = batch_embed_score(&reduced);
    let masked = mask_nonlinguistic(query);
    let competitors = prompt_competitors(&candidates, scope, project_dir);
    let bodies = body_map(competitors.iter().copied());
    let verdicts = late_interaction::run(&reduced, &bodies, admission);
    let diag = verdicts.as_ref().and_then(|_| late_interaction::run_diagnostic(&reduced, &bodies, usize::MAX, admission));

    let mut stages: HashMap<String, &'static str> = HashMap::new();
    let mut hits: Vec<Hit<(String, bool)>> = Vec::new();
    let mut fired_ids: HashSet<String> = HashSet::new();
    let mut prompt_only: Option<EmbedScores> = None;
    let surface = PromptSurface {
        query,
        masked: &masked,
        session_id: PROBE_SESSION,
        response_context: None,
        embed_matches: &embed_matches,
        verdicts: verdicts.as_ref(),
    };
    for way in &candidates {
        if !eligible(way, Lane::Prompt { scope }, project_dir) {
            stages.insert(way.id.clone(), "masked");
            continue;
        }
        if !way.embeddable() && way.pattern.is_none() {
            stages.insert(way.id.clone(), "not-embeddable");
            continue;
        }
        let (outcome, needs_parent) = prompt_outcome(way, &surface, &fired_ids, &mut prompt_only);
        match outcome {
            PromptMatch::Fired { channel, score, matched_span } => {
                fired_ids.insert(way.id.clone());
                let payload = (channel, needs_parent);
                hits.push(match (&matched_span, way.pattern.as_deref()) {
                    (Some(span), Some(pat)) if score.is_none() => Hit::explicit(&way.id, pat, span, payload),
                    _ => Hit::scored(&way.id, score, payload),
                });
            }
            PromptMatch::KeywordGated(_) => {
                stages.insert(way.id.clone(), "keyword-gated");
            }
            PromptMatch::NearMiss(_) | PromptMatch::NoMatch => {}
        }
    }
    order_hits(&mut hits);
    let boost_exercised = hits.iter().any(|h| h.payload.1);
    let fired = admit(&hits, &mut stages, |h| (h.payload.0.clone(), h.payload.1));

    // Stage of the ways the matcher saw but did not fire.
    let by_corpus: HashMap<&str, &str> = candidates.iter().map(|c| (c.corpus_id.as_str(), c.id.as_str())).collect();
    let rows = match &diag {
        Some(diag) => {
            let mut rows = Vec::new();
            for r in diag {
                let Some(id) = by_corpus.get(r.id.as_str()) else { continue };
                if !stages.contains_key(*id) {
                    let stage = if r.capped {
                        "capped"
                    } else if r.admitted {
                        "not-confirmed"
                    } else {
                        "not-admitted"
                    };
                    stages.insert((*id).to_string(), stage);
                }
                rows.push(ProbeRow { id: (*id).to_string(), score: r.share, peak: Some(r.peak), confirm: r.confirm });
            }
            rows
        }
        None => single_vector_rows(&competitors, &embed_matches),
    };
    for c in &competitors {
        stages.entry(c.id.clone()).or_insert("below-threshold");
    }
    let mut rows = rows;
    sort_rows(&mut rows);
    ProbeScan { rows, fired, stages, late: diag.is_some(), boost_exercised }
}

/// Probe the Bash lane with `text` as the tool description and an empty command.
pub(crate) fn bash(text: &str, project_dir: &str) -> ProbeScan {
    let scope = "agent";
    let candidates = collect_candidates(project_dir);
    // The lane embeds the lookbehind, the command and the description. A probe
    // has no transcript, so only the description is left.
    let reduced = reduce::reduce_for_embed(&format!("  {text}"), BUDGET_COMMAND);
    let embed_matches = batch_embed_score(&reduced);
    let hits = command_hits(
        &CommandSurface { cmd: "", description: Some(text), session_id: PROBE_SESSION, scope, project_dir },
        &candidates,
        &embed_matches,
    );
    let boost_exercised = hits.iter().any(|h| h.payload.2);

    let mut stages: HashMap<String, &'static str> = HashMap::new();
    let lane_ok = |w: &WayCandidate| {
        crate::session::scope_matches(&w.scope, scope)
            && super::candidates::check_when(&w.when_project, &w.when_file_exists, project_dir)
            && w.trigger.is_none()
    };
    for w in &candidates {
        if !lane_ok(w) {
            // A state-triggered way (session-start, context-threshold) fires from
            // a condition; the Bash lane's semantic matcher skips it.
            stages.insert(w.id.clone(), if w.trigger.is_some() { "state-trigger" } else { "masked" });
        }
    }
    let fired = admit(&hits, &mut stages, |h| (h.payload.0.to_string(), h.payload.2));
    let competitors: Vec<&WayCandidate> = candidates.iter().filter(|c| c.embeddable() && lane_ok(c)).collect();
    let mut rows = single_vector_rows(&competitors, &embed_matches);
    sort_rows(&mut rows);
    for c in &competitors {
        stages.entry(c.id.clone()).or_insert("below-threshold");
    }
    ProbeScan { rows, fired, stages, late: false, boost_exercised }
}

/// Admit the ordered hits as the scan's show loop does, minus the show: a hit
/// that fired only on a parent boost is withheld unless an ancestor was shown.
/// Marks each hit's stage and returns the shown ids with their channels.
fn admit<T>(
    hits: &[Hit<T>],
    stages: &mut HashMap<String, &'static str>,
    info: impl Fn(&Hit<T>) -> (String, bool),
) -> Vec<(String, String)> {
    let mut shown: HashSet<String> = HashSet::new();
    let mut fired = Vec::new();
    for hit in hits {
        let (channel, needs_parent) = info(hit);
        if needs_parent && !has_shown_ancestor(&hit.id, &shown) {
            stages.insert(hit.id.clone(), "withheld-parent");
            continue;
        }
        shown.insert(hit.id.clone());
        stages.insert(hit.id.clone(), "fired");
        fired.push((hit.id.clone(), channel));
    }
    fired
}

/// Calibrated EN probabilities for the competing ways: the single-vector
/// fail-safe's own quantity.
fn single_vector_rows(competitors: &[&WayCandidate], scores: &EmbedScores) -> Vec<ProbeRow> {
    competitors
        .iter()
        .filter_map(|c| {
            scores
                .prob_en(&c.corpus_id, c.embeddable())
                .map(|p| ProbeRow { id: c.id.clone(), score: p, peak: None, confirm: None })
        })
        .collect()
}

fn sort_rows(rows: &mut [ProbeRow]) {
    rows.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));
}
