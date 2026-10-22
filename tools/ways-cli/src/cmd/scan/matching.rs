//! Matching a prompt-shaped surface against one way (keyword channel, semantic
//! channel, near-miss) and the telemetry the prompt-shaped lanes share.

use super::scoring::EmbedScores;
use super::{decision, late_interaction, reduce, regex_span, session, EffectiveThresholds, WayCandidate};

// ── Matching ────────────────────────────────────────────────────

/// Outcome of matching a prompt against one way.
pub(super) enum PromptMatch {
    /// The way fired. `channel` is the trigger channel; `score` is the
    /// embedding score that cleared threshold (`None` for deterministic keyword
    /// fires) — logged onto `way_fired` for embed_threshold tuning (ADR-134 D).
    /// `matched_span` is the regex match text for the keyword channel (ADR-153 §3);
    /// `None` for semantic — one embedding per way, so there is no term to recover.
    Fired { channel: String, score: Option<f64>, matched_span: Option<String> },
    /// The pattern matched but the embedding score fell below the keyword gate
    /// floor on every available model lane (ADR-155): a lexical coincidence,
    /// vetoed. Carries the already-computed evidence for `way_keyword_gated`
    /// telemetry — the stream that calibrates `keyword_gate_fraction`.
    KeywordGated(KeywordGated),
    /// The way did NOT fire, but at least one model scored within
    /// `near_miss_margin` below its effective threshold (ADR-134). Carries the
    /// already-computed scores for telemetry — no new embedding is done.
    NearMiss(NearMiss),
    /// No match, and not close enough to record.
    NoMatch,
}

/// Evidence for a gated keyword hit (ADR-155/156): what the pattern matched and
/// the calibrated relevance probability each model lane produced, against the
/// keyword floor probability τ_k that vetoed it.
pub(super) struct KeywordGated {
    matched_span: String,
    prob_en: Option<f64>,
    prob_multi: Option<f64>,
    floor: f64,
}

/// A below-threshold embedding result close enough to log (ADR-134 Decision 1),
/// now in calibrated probability space (ADR-156).
pub(super) struct NearMiss {
    prob_en: Option<f64>,
    prob_multi: Option<f64>,
    tau_s: f64,
    /// Smallest `τ_s - probability` among the models within margin — how close
    /// the way came to firing on its best path.
    margin: f64,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn match_prompt(
    query: &str,
    pattern: &Option<String>,
    pattern_strict: bool,
    embeddable: bool,
    corpus_id: &str,
    thresholds: EffectiveThresholds,
    scores: &EmbedScores,
    near_miss_margin: f64,
    keyword_floor: f64,
    verdicts: Option<&late_interaction::Verdicts>,
) -> PromptMatch {
    // Calibrated relevance probability per lane (ADR-156). `None` means no
    // calibrated signal: the lane didn't run, the way isn't embeddable, or no
    // calibration is loaded. An embeddable way that ran but is missing from the
    // results scores as g(cos 0.0) — a low probability, not absence of signal.
    let tau_s = thresholds.semantic;
    let tau_k = keyword_floor;
    let prob_en = scores.prob_en(corpus_id, embeddable);
    let prob_multi = scores.prob_multi(corpus_id, embeddable);

    // Channel 1: Regex pattern — deterministic, but gated (ADR-155/156): the hit
    // fires only if the way's calibrated probability also clears the keyword
    // floor τ_k on at least one model lane, using probabilities the batch pass
    // already produced — no extra model work. It fails OPEN when there is
    // genuinely no calibrated signal either direction — the engine didn't run,
    // the way isn't embeddable, or no calibration is loaded: the author's
    // explicit trigger stands. `pattern_strict: true` restores an unconditional
    // keyword fire. A pattern miss is never a near-miss.
    let mut gated: Option<KeywordGated> = None;
    if let Some(ref pat) = pattern {
        if let Some(span) = regex_span(pat, query) {
            let no_signal = prob_en.is_none() && prob_multi.is_none();
            let clears =
                prob_en.is_some_and(|p| p >= tau_k) || prob_multi.is_some_and(|p| p >= tau_k);
            if pattern_strict || no_signal || clears {
                return PromptMatch::Fired {
                    channel: "keyword".to_string(),
                    score: None,
                    matched_span: Some(span),
                };
            }
            // Gated — but τ_k and τ_s are independent (ADR-156), so τ_k may sit
            // above τ_s. Hold the veto and let the semantic lane fire first;
            // matching stays additive-OR (a gated keyword never shadows a way
            // that clears the semantic bar).
            gated = Some(KeywordGated {
                matched_span: span,
                prob_en,
                prob_multi,
                floor: tau_k,
            });
        }
    }

    // Channel 2: Embedding. When the ADR-160 late-interaction matcher is engaged
    // it owns the semantic decision for this way (chunk → softmax-share →
    // body-confirm, run
    // once upstream); otherwise the single-vector calibrated gate does —
    // calibration makes probabilities comparable across models, so both lanes
    // share one threshold τ_s and either firing is sufficient.
    if let Some(m) = verdicts {
        if let Some(share) = m.fired_score(corpus_id) {
            return PromptMatch::Fired {
                channel: "semantic:late-interaction:en".to_string(),
                score: Some(share),
                matched_span: None,
            };
        }
    } else {
        if prob_en.is_some_and(|p| p >= tau_s) {
            return PromptMatch::Fired {
                channel: "semantic:embedding:en".to_string(),
                score: prob_en,
                matched_span: None,
            };
        }
        if prob_multi.is_some_and(|p| p >= tau_s) {
            return PromptMatch::Fired {
                channel: "semantic:embedding:multi".to_string(),
                score: prob_multi,
                matched_span: None,
            };
        }
    }

    // A keyword hit that was gated and whose way did not clear the semantic bar
    // is a vetoed lexical coincidence — report it (preempts near-miss logging).
    if let Some(kg) = gated {
        return PromptMatch::KeywordGated(kg);
    }

    // No fire. Record a near-miss when a lane's probability landed in the band
    // just below τ_s: `τ_s - margin <= p < τ_s`. The reported margin is the
    // smallest shortfall across lanes, against the SAME τ_s the fire path uses.
    let shortfall = |p: Option<f64>| -> Option<f64> {
        p.and_then(|v| {
            let gap = tau_s - v;
            (gap > 0.0 && gap <= near_miss_margin).then_some(gap)
        })
    };
    let margin = [shortfall(prob_en), shortfall(prob_multi)]
        .into_iter()
        .flatten()
        .fold(None, |acc: Option<f64>, g| Some(acc.map_or(g, |a| a.min(g))));

    match margin {
        Some(margin) => PromptMatch::NearMiss(NearMiss {
            prob_en,
            prob_multi,
            tau_s,
            margin,
        }),
        None => PromptMatch::NoMatch,
    }
}

/// Emit a `way_nearmiss` telemetry event (ADR-134 Decision 1): a way that did
/// not fire but scored within the near-miss margin of its threshold. This is
/// persistence of already-computed scores, not new work — the tuning passes
/// of ADR-134 consume the stream. The leading fields
/// (`event`, `way`, `domain`, `trigger`, `scope`, `project`, `session`) follow
/// the `way_fired` convention (scan/state.rs) for reader symmetry; the score
/// fields are near-miss-specific. There is no `team` field — team attribution
/// lives on fires (show/mod.rs), not on the below-threshold telemetry.
pub(super) fn log_near_miss(
    way: &WayCandidate,
    nm: &NearMiss,
    trigger: &str,
    scope: &str,
    project_dir: &str,
    session_id: &str,
    query: &str,
) {
    let fmt = |v: Option<f64>| v.map(|s| format!("{s:.4}")).unwrap_or_default();
    let domain = way.id.split('/').next().unwrap_or(&way.id);
    // events.jsonl is bounded by age (retention days) and size; see session::log_event.
    session::log_event(&[
        ("event", "way_nearmiss"),
        ("way", &way.id),
        ("agent_id", &session::current_agent()),
        ("corpus_id", &way.corpus_id),
        ("domain", domain),
        ("prob_en", &fmt(nm.prob_en)),
        ("prob_multi", &fmt(nm.prob_multi)),
        ("tau_s", &format!("{:.4}", nm.tau_s)),
        ("margin", &format!("{:.4}", nm.margin)),
        ("trigger", trigger),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
        ("query_tokens", &reduce::approx_tokens(query).to_string()),
    ]);
}

/// Emit a `way_keyword_gated` telemetry event (ADR-155): a pattern hit vetoed
/// because the way's embedding score sat below the gate floor on every model
/// lane. Same shape discipline as `log_near_miss` — persistence of
/// already-computed evidence, consumed by the tuning passes to calibrate
/// `keyword_gate_fraction` before any tightening. The `matched_span` names the
/// alternation that would have fired, which is exactly the per-alternation
/// precision signal the pattern-hygiene rework (ADR-155 §5) needs.
pub(super) fn log_keyword_gated(
    way: &WayCandidate,
    kg: &KeywordGated,
    trigger: &str,
    scope: &str,
    project_dir: &str,
    session_id: &str,
) {
    let fmt = |v: Option<f64>| v.map(|s| format!("{s:.4}")).unwrap_or_default();
    let domain = way.id.split('/').next().unwrap_or(&way.id);
    // token_position mirrors way_fired (show/mod.rs): the introspection join
    // clusters gated rows into the same turns as fires, so they must carry
    // the same position signal instead of an implicit 0.
    let token_pos = session::get_token_position(session_id);
    session::log_event(&[
        ("event", "way_keyword_gated"),
        ("way", &way.id),
        ("corpus_id", &way.corpus_id),
        ("domain", domain),
        ("matched_span", &kg.matched_span),
        ("prob_en", &fmt(kg.prob_en)),
        ("prob_multi", &fmt(kg.prob_multi)),
        ("floor", &format!("{:.4}", kg.floor)),
        ("trigger", trigger),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
        ("token_position", &token_pos.to_string()),
    ]);
}

/// Record a keyword-gated way in the scan's decision record (ADR-701 §2), with
/// the evidence its `way_keyword_gated` event carries.
pub(super) fn record_keyword_gated(record: &mut decision::Record, way: &WayCandidate, kg: &KeywordGated) {
    record.keyword_gated(&way.id, &kg.matched_span, kg.prob_en, kg.prob_multi, kg.floor);
}

/// Record a near miss in the scan's decision record (ADR-701 §2), with the
/// scores its `way_nearmiss` event carries.
pub(super) fn record_near_miss(record: &mut decision::Record, way: &WayCandidate, nm: &NearMiss) {
    record.near_miss(&way.id, nm.prob_en, nm.prob_multi, nm.tau_s, nm.margin);
}

#[cfg(test)]
mod near_miss_tests {
    //! ADR-134 task A: the near-miss decision in `match_prompt`. These cover
    //! the pure score/threshold arithmetic — no embedding subprocess, no I/O.
    use super::*;
    use super::super::regex_matches;

    use ways_core::calibration::{Calibration, ModelCalibration};

    // ADR-156: match_prompt scores calibrated probabilities. The tests feed raw
    // cosines through a fixed test calibration `g(s) = σ(10·s − 2.5)`, chosen so
    // g(0.25)=0.5 exactly (the τ_s boundary) and g(0.0)≈0.076 (a missing row —
    // negative cosine — lands well below the keyword floor).
    const THR: EffectiveThresholds = EffectiveThresholds { semantic: 0.5 };
    const MARGIN: f64 = 0.05;
    const FLOOR: f64 = 0.15; // τ_k, mirrors config default keyword_floor_probability
    const TEST_CAL: ModelCalibration = ModelCalibration { a: 10.0, b: -2.5, auc: 1.0, n: 0 };

    /// Calibrated probability for a cosine under the test calibration.
    fn p(cosine: f64) -> f64 {
        TEST_CAL.probability(cosine)
    }

    fn scores(en: Option<f64>, multi: Option<f64>) -> EmbedScores {
        EmbedScores {
            en: en.map(|s| vec![("w".to_string(), s)]),
            multi: multi.map(|s| vec![("w".to_string(), s)]),
            calibration: Calibration { en: Some(TEST_CAL), multi: Some(TEST_CAL) },
        }
    }

    fn run(en: Option<f64>, multi: Option<f64>, pattern: Option<&str>) -> PromptMatch {
        run_full(scores(en, multi), pattern, false, true)
    }

    fn run_full(
        scores: EmbedScores,
        pattern: Option<&str>,
        strict: bool,
        embeddable: bool,
    ) -> PromptMatch {
        match_prompt(
            "query text",
            &pattern.map(|p| p.to_string()),
            strict,
            embeddable,
            "w",
            THR,
            &scores,
            MARGIN,
            FLOOR,
            None,
        )
    }

    #[test]
    fn en_clears_fires_en() {
        // cos 0.35 → g ≈ 0.73 ≥ τ_s 0.5.
        assert!(matches!(run(Some(0.35), None, None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:en"));
    }

    #[test]
    fn semantic_fire_carries_its_probability_keyword_does_not() {
        // ADR-134 D / ADR-156: the firing calibrated probability rides on Fired
        // for telemetry; a deterministic keyword fire carries none.
        match run(Some(0.35), None, None) {
            PromptMatch::Fired { score, .. } => {
                assert!((score.unwrap() - p(0.35)).abs() < 1e-9);
            }
            _ => panic!("expected Fired"),
        }
        // multi fire (EN below τ_s) carries the multi probability, not EN's.
        match run(Some(0.20), Some(0.35), None) {
            PromptMatch::Fired { channel, score, matched_span } => {
                assert_eq!(channel, "semantic:embedding:multi");
                assert!((score.unwrap() - p(0.35)).abs() < 1e-9);
                assert_eq!(matched_span, None, "semantic carries no matched term");
            }
            _ => panic!("expected multi Fired"),
        }
        match run(Some(0.35), None, Some("query")) {
            PromptMatch::Fired { channel, score, matched_span } => {
                assert_eq!(channel, "keyword");
                assert_eq!(score, None);
                assert_eq!(matched_span.as_deref(), Some("query"), "keyword records the regex match");
            }
            _ => panic!("expected keyword Fired"),
        }
    }

    #[test]
    fn trigger_regex_is_case_insensitive() {
        // ADR-157: lowercase patterns match the uppercase acronyms users type,
        // corpus-wide, without per-way (?i). Both helpers share compile_trigger.
        assert!(regex_matches(r"\bssh\b", "connect over SSH"));
        assert!(regex_matches(r"\bpr\b", "open a PR for this"));
        assert_eq!(
            regex_span(r"\berd\b", "draw an ERD diagram").as_deref(),
            Some("ERD"),
            "captured span keeps the input's original case"
        );
        // Deliberately-uppercase pattern still matches its lowercase form.
        assert!(regex_matches(r"SKILL\.md", "editing skill.md"));
        // An inline (?-i) scope can still opt back into case-sensitivity.
        assert!(!regex_matches(r"(?-i)SSH", "lowercase ssh only"));
    }

    #[test]
    fn multi_clears_when_en_below_fires_multi() {
        assert!(matches!(run(Some(0.20), Some(0.35), None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:multi"));
    }

    #[test]
    fn within_margin_is_near_miss_with_shortfall() {
        // cos 0.24 → g ≈ 0.475, just below τ_s 0.5 (shortfall ≈ 0.025 < MARGIN).
        match run(Some(0.24), None, None) {
            PromptMatch::NearMiss(nm) => {
                assert!((nm.margin - (0.5 - p(0.24))).abs() < 1e-9, "margin = τ_s - prob");
                assert!((nm.prob_en.unwrap() - p(0.24)).abs() < 1e-9);
                assert_eq!(nm.prob_multi, None);
            }
            other => panic!("expected NearMiss, got {:?}", discriminant(&other)),
        }
    }

    #[test]
    fn smallest_shortfall_wins_across_models() {
        // en short by ~0.025, multi short by ~0.0125 -> reported margin is the multi one.
        match run(Some(0.24), Some(0.245), None) {
            PromptMatch::NearMiss(nm) => {
                assert!((nm.margin - (0.5 - p(0.245))).abs() < 1e-9);
            }
            other => panic!("expected NearMiss, got {:?}", discriminant(&other)),
        }
    }

    #[test]
    fn beyond_margin_is_no_match() {
        // cos 0.20 → g ≈ 0.378, shortfall ≈ 0.12 > MARGIN.
        assert!(matches!(run(Some(0.20), None, None), PromptMatch::NoMatch));
    }

    #[test]
    fn pattern_match_preempts_near_miss() {
        // Would be a near-miss, but a keyword hit (g ≈ 0.475 ≥ τ_k) fires.
        assert!(matches!(run(Some(0.24), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn absent_scores_are_no_match() {
        assert!(matches!(run(None, None, None), PromptMatch::NoMatch));
    }

    #[test]
    fn probability_exactly_at_threshold_fires_not_near_miss() {
        // cos 0.25 → g = 0.5 exactly = τ_s. The `>=` fire check and the
        // `gap > 0.0` near-miss guard must agree: it fires, never a near-miss.
        assert!(matches!(run(Some(0.25), None, None),
            PromptMatch::Fired { channel: c, .. } if c == "semantic:embedding:en"));
    }

    fn discriminant(m: &PromptMatch) -> &'static str {
        match m {
            PromptMatch::Fired { .. } => "Fired",
            PromptMatch::KeywordGated(_) => "KeywordGated",
            PromptMatch::NearMiss(_) => "NearMiss",
            PromptMatch::NoMatch => "NoMatch",
        }
    }

    // ── ADR-155: the semantic gate on keyword fires ──────────────

    #[test]
    fn keyword_below_gate_floor_is_gated_with_evidence() {
        // cos 0.05 → g ≈ 0.119 < τ_k 0.15: a lexical coincidence, vetoed.
        match run(Some(0.05), None, Some("query")) {
            PromptMatch::KeywordGated(kg) => {
                assert_eq!(kg.matched_span, "query");
                assert!((kg.prob_en.unwrap() - p(0.05)).abs() < 1e-9);
                assert_eq!(kg.prob_multi, None);
                assert!((kg.floor - FLOOR).abs() < 1e-9);
            }
            other => panic!("expected KeywordGated, got {}", discriminant(&other)),
        }
    }

    #[test]
    fn keyword_above_gate_floor_fires() {
        // cos 0.10 → g ≈ 0.182 ≥ τ_k 0.15.
        assert!(matches!(run(Some(0.10), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn either_lane_clearing_the_floor_passes_the_gate() {
        // EN below τ_k (g ≈ 0.119), multi above it (g ≈ 0.182) — passes.
        assert!(matches!(run(Some(0.05), Some(0.10), Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn pattern_strict_bypasses_the_gate() {
        assert!(matches!(run_full(scores(Some(0.05), None), Some("query"), true, true),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn gate_fails_open_when_no_lane_ran() {
        // Engine unavailable: the explicit trigger stands.
        assert!(matches!(run(None, None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn missing_row_on_a_ran_lane_gates_an_embeddable_way() {
        // way-embed emits only scores >= 0.0: a lane that ran with no row for an
        // embeddable way means NEGATIVE cosine — the strongest "unrelated"
        // signal. It scores as g(0.0) ≈ 0.076, below τ_k, so it gates rather
        // than fails open (gate monotonicity, ADR-155/156).
        let lane_ran_no_row = EmbedScores {
            en: Some(vec![]),
            multi: None,
            calibration: Calibration { en: Some(TEST_CAL), multi: None },
        };
        match run_full(lane_ran_no_row, Some("query"), false, true) {
            PromptMatch::KeywordGated(kg) => {
                assert!((kg.prob_en.unwrap() - p(0.0)).abs() < 1e-9, "missing row → g(0.0)");
                assert_eq!(kg.prob_multi, None, "lane that didn't run stays None");
            }
            other => panic!("expected KeywordGated, got {}", discriminant(&other)),
        }
    }

    #[test]
    fn non_embeddable_way_fails_open_even_when_lanes_ran() {
        // A trigger-only way (no description/vocabulary) can't be in the corpus;
        // a missing row says nothing about it. The explicit trigger stands.
        let lane_ran_no_row = EmbedScores {
            en: Some(vec![]),
            multi: None,
            calibration: Calibration { en: Some(TEST_CAL), multi: None },
        };
        assert!(matches!(
            run_full(lane_ran_no_row, Some("query"), false, false),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"
        ));
    }

    #[test]
    fn no_calibration_fails_open_on_keyword() {
        // A corpus that predates calibration: no calibrated signal, so the
        // keyword fires open (degraded), never the retired raw-cosine path.
        let uncalibrated = EmbedScores {
            en: Some(vec![("w".to_string(), 0.05)]),
            multi: None,
            calibration: Calibration::default(),
        };
        assert!(matches!(run_full(uncalibrated, Some("query"), false, true),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn gated_keyword_does_not_shadow_a_semantic_fire() {
        // cos 0.35 clears τ_s: it also clears τ_k, so a pattern hit fires on the
        // keyword channel (gate passes trivially).
        assert!(matches!(run(Some(0.35), None, Some("query")),
            PromptMatch::Fired { channel: c, .. } if c == "keyword"));
    }

    #[test]
    fn high_keyword_floor_does_not_shadow_a_semantic_fire() {
        // τ_k (0.6) > τ_s (0.5): a pattern hit at cos 0.27 (g ≈ 0.55) fails the
        // keyword floor but clears the semantic bar — it must fire semantically,
        // not be vetoed by the gate (ADR-156: τ_k and τ_s are independent).
        let outcome = match_prompt(
            "query text", &Some("query".to_string()), false, true, "w",
            THR, &scores(Some(0.27), None), MARGIN, 0.6, None,
        );
        assert!(matches!(outcome,
            PromptMatch::Fired { channel: ref c, .. } if c == "semantic:embedding:en"),
            "got {:?}", discriminant(&outcome));
    }
}
