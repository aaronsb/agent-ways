//! Scan ways and output matched content — replaces hook scan loops.
//!
//! Combines file walking, frontmatter extraction, matching (pattern + semantic),
//! scope/precondition gating, parent-threshold lowering, and show (display).

pub(crate) mod candidates;
mod candidate_log;
mod command;
mod decision;
mod file;
mod gate;
mod late_interaction;
mod matching;
mod prompt;
mod task;
mod tool;
#[cfg(test)]
pub(crate) use late_interaction::surface_chunk_count;
pub(crate) mod sidecar;
pub(crate) mod lookup;
mod lookbehind;
mod order;
pub(crate) mod probe;
mod reduce;
pub(crate) mod scoring;
mod state;
pub(crate) use scoring::{batch_embed_score, batch_embed_score_with, sibling_corpus};

// Per-hook embed-query budgets (approximate tokens). MiniLM's window
// is 128 position embeddings; we budget ~85% of that. The reducer
// passes inputs through unchanged when they already fit; long inputs
// collapse to top-salience sentences within budget. The approximate
// tokenizer here (whitespace + char-budget max) over-counts vs
// MiniLM's WordPiece, so real tokens land safely under 128 even at
// the higher budgets. See ADR-130.
const BUDGET_PROMPT: usize = 110;
const BUDGET_TASK: usize = 110;
const BUDGET_COMMAND: usize = 75;
const BUDGET_FILE: usize = 30;
pub use state::state;
pub use command::command;
pub use file::file;
pub use prompt::{messages, prompt};
pub use task::task;
pub use tool::tool;

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::session;

use candidates::{check_when, collect_candidates};
use scoring::EmbedScores;
use command::{command_hits, CommandSurface, ShellCommand};
use matching::PromptMatch;
use prompt::{is_system_envelope, prompt_outcome, PromptSurface};

pub(crate) struct WayCandidate {
    pub id: String,
    /// Namespaced id used solely for the embedding-corpus lookup. Equals `id`
    /// for global ways; for project ways it is `{project_key}/{id}`, matching
    /// how `ways corpus` namespaces project entries. Session markers, show, and
    /// parent-boost all use the bare `id`, not this.
    pub corpus_id: String,
    pub path: PathBuf,
    pub pattern: Option<String>,
    /// Opt-out from the semantic keyword gate (ADR-155). `true` means every
    /// pattern hit fires regardless of embedding score — for patterns that
    /// genuinely mean "this exact token, always" (e.g. slash-command names).
    pub pattern_strict: bool,
    pub commands: Option<String>,
    pub files: Option<String>,
    /// Regex matched against the payload's `tool_name` on the tool lane (ADR-704).
    pub tool_name: Option<String>,
    /// The raw `tool_event:` value: the hook events the tool lane fires on,
    /// read by [`crate::frontmatter::tool_events`]. `None` is `[PreToolUse]`.
    pub tool_event: Option<String>,
    pub description: String,
    pub vocabulary: String,
    /// Context-threshold percentage (only meaningful for trigger: context-threshold).
    pub threshold: f64,
    pub scope: String,
    pub when_project: Option<String>,
    pub when_file_exists: Option<String>,
    pub trigger: Option<String>,
    pub trigger_path: Option<String>,
}

impl WayCandidate {
    /// Whether this way can appear in the embedding corpus. Mirrors the corpus
    /// builder's gate (`cmd::corpus`): an entry needs both a description and a
    /// vocabulary. The keyword gate (ADR-155) uses this to tell "the way was
    /// never embedded" (fail open) apart from "the lane ran and scored this
    /// way below way-embed's 0.0 emission floor" (gate on 0.0).
    fn embeddable(&self) -> bool {
        !self.description.is_empty() && !self.vocabulary.is_empty()
    }
}

/// The project switch (ADR-184 item 6): `enabled: false` in the project's
/// `ways.yaml` makes every scan lane inject nothing there. The user config can
/// carry the same key to switch ways off everywhere.
pub fn enabled_for(project: Option<&str>) -> bool {
    let dir = match project {
        Some(p) => p.to_string(),
        None => crate::util::project_dir(),
    };
    crate::config::Config::load(&dir).enabled
}

// ── Authoring diagnostic (task #5) ─────────────────────────────

/// Probe-only switches for the body-rank evaluation (see `late_interaction::Tuning`).
pub(crate) fn set_body_rank_tuning(weight: Option<f64>, single_section_confirm: bool) {
    if weight.is_some() || single_section_confirm {
        late_interaction::set_tuning(late_interaction::Tuning {
            weight: weight.unwrap_or(late_interaction::BODY_RANK_WEIGHT),
            single_section_confirm,
        });
    }
}

pub(crate) use late_interaction::{admission_rule, chunk_sections, Admission, BodyRank, DiagRow, DIAG_CONFIRM_GATE, DIAG_PEAK_GATE};

/// Run the late-interaction matcher over `query` for way authoring — the modern
/// equivalent of the single-vector `ways author match`. Reduces the query exactly as the
/// prompt scan does (so the diagnostic sees the surface production sees), then
/// returns the top candidates' evidence (peak / share / body-confirm / fired) with
/// the reduced surface for context. `None` means late-interaction could not run
/// (engine unavailable, or the surface is too sparse to chunk) — the caller then
/// falls back to the single-vector view, mirroring production's fail-safe.
///
/// The competing set is the one a prompt scan in `agent` scope uses (toggles,
/// scope and `when:` against `project`), so the shares match the live fire path.
/// `unfiltered` competes every candidate instead, for seeing how a way would rank
/// among all of them. `admission` is the stage-4 rule the live matcher would use.
pub fn diagnose(query: &str, project: Option<&str>, top_n: usize, unfiltered: bool, admission: Admission, body_rank: BodyRank) -> Option<(String, Vec<DiagRow>)> {
    let project_dir = project.map(|s| s.to_string()).unwrap_or_else(crate::util::project_dir);
    let candidates = collect_candidates(&project_dir);
    let reduced = reduce::reduce_for_embed(query, BUDGET_PROMPT);
    let bodies = body_map(diag_candidates(&candidates, &project_dir, unfiltered).into_iter());
    let rows = late_interaction::run_diagnostic(&reduced, &bodies, top_n, admission, body_rank)?;
    Some((reduced, rows))
}

/// The candidates `ways author match` competes: the prompt lane in `agent`
/// scope, or all of them when `unfiltered`.
fn diag_candidates<'a>(candidates: &'a [WayCandidate], project_dir: &str, unfiltered: bool) -> Vec<&'a WayCandidate> {
    candidates.iter().filter(|w| unfiltered || eligible(w, Lane::Prompt { scope: "agent" }, project_dir)).collect()
}

/// The surface a scan serves, which decides what may fire on it.
#[derive(Clone, Copy)]
enum Lane<'a> {
    /// A user prompt, in the session's scope.
    Prompt { scope: &'a str },
    /// A subagent or teammate dispatch.
    Task { teammate: bool },
}

/// Whether `way` can fire on this lane at all: scope, state trigger and `when:`
/// preconditions. The scan loops apply it to each candidate, and the matchers
/// take their competing set from it, so a way that cannot fire here takes no
/// softmax share and no confirmation slot (ADR-701 §1). Toggles are already
/// applied by `collect_candidates`.
fn eligible(way: &WayCandidate, lane: Lane<'_>, project_dir: &str) -> bool {
    let lane_ok = match lane {
        Lane::Prompt { scope } => session::scope_matches(&way.scope, scope),
        // A task scan needs subagent scope (or teammate for a team) and skips
        // state-triggered ways.
        Lane::Task { teammate } => {
            (way.scope.contains("subagent") || (teammate && way.scope.contains("teammate"))) && way.trigger.is_none()
        }
    };
    lane_ok && check_when(&way.when_project, &way.when_file_exists, project_dir)
}

/// The ways a prompt in `scope` competes: embeddable and [`eligible`] on the
/// prompt lane. The scan's matcher, its logged candidates and a lookup's
/// search all take their set from here, so none can drift from the others.
pub(crate) fn prompt_competitors<'a>(candidates: &'a [WayCandidate], scope: &str, project_dir: &str) -> Vec<&'a WayCandidate> {
    candidates.iter().filter(|c| c.embeddable() && eligible(c, Lane::Prompt { scope }, project_dir)).collect()
}

/// ADR-701 §7: the body sidecar a prompt scan in `project_dir` would use, as
/// (ways, section vectors), or why it would confirm per call. For `ways status`.
pub(crate) fn sidecar_state(project_dir: &str) -> Result<(usize, usize), sidecar::Fallback> {
    let bin = crate::paths::way_embed().ok_or(sidecar::Fallback::NoEmbedder)?;
    let candidates = collect_candidates(project_dir);
    let enabled = body_map(prompt_competitors(&candidates, "agent", project_dir).into_iter());
    let sc = sidecar::state(&crate::paths::corpus_dir(), &bin, enabled.keys().map(String::as_str))?;
    Ok((sc.way_count(), sc.vector_count()))
}

/// Map each embeddable candidate's corpus id to its `.md` path, for the
/// late-interaction matcher's body-confirmation stage (ADR-160) and as the set
/// of ways allowed to compete.
fn body_map<'a>(candidates: impl Iterator<Item = &'a WayCandidate>) -> std::collections::HashMap<String, PathBuf> {
    candidates.filter(|c| c.embeddable()).map(|c| (c.corpus_id.clone(), c.path.clone())).collect()
}

/// The effective semantic fire probability τ_s for a way at a given moment in a
/// session (ADR-156). Calibration makes probabilities comparable across models,
/// so one threshold serves both lanes; the keyword floor τ_k is global and read
/// separately.
#[derive(Clone, Copy)]
struct EffectiveThresholds {
    semantic: f64,
}

/// Compute the effective semantic fire probability, accounting for parent-boost.
///
/// Parent-boost (ADR-125): if any ancestor has fired in the session, the base
/// τ_s is multiplied by `parent_threshold_multiplier` (default 0.8), floored at
/// `parent_boost_floor`. The floor prevents cascading boosts from pushing
/// children into the noise band. All values are calibrated probabilities.
fn effective_thresholds(way: &WayCandidate, session_id: &str) -> EffectiveThresholds {
    effective_thresholds_in_scan(way, session_id, &HashSet::new()).0
}

/// Proper ancestors of a way id, nearest first (`a/b/c` → `a/b`, `a`).
fn ancestors(id: &str) -> impl Iterator<Item = &str> {
    let mut path = id;
    std::iter::from_fn(move || {
        let idx = path.rfind('/')?;
        path = &path[..idx];
        Some(path)
    })
}

fn has_shown_ancestor(id: &str, shown: &HashSet<String>) -> bool {
    ancestors(id).any(|a| shown.contains(a))
}

/// [`effective_thresholds`], also boosting a way whose ancestor fired earlier
/// in the same scan (`fired_in_scan`). Lanes show their hits after matching
/// (scan/order.rs), so an ancestor fired in this scan has no session marker yet;
/// the lanes walk candidates in tree order, so the ancestor is decided first.
/// The flag is true when only this scan's ancestor supplied the boost: the
/// caller then shows the way only if that ancestor is shown.
fn effective_thresholds_in_scan(
    way: &WayCandidate,
    session_id: &str,
    fired_in_scan: &HashSet<String>,
) -> (EffectiveThresholds, bool) {
    let cfg = crate::config::global();
    let base = cfg.semantic_fire_probability;

    let by_session = ancestors(&way.id).any(|a| session::way_is_shown(a, session_id));
    let by_scan = !by_session && ancestors(&way.id).any(|a| fired_in_scan.contains(a));

    let semantic = if by_session || by_scan {
        (base * cfg.parent_threshold_multiplier).max(cfg.parent_boost_floor)
    } else {
        base
    };
    (EffectiveThresholds { semantic }, by_scan)
}

/// Semantic score for a check, taking the higher of the two model paths
/// that clears its own threshold. The two models are evaluated
/// independently (apples and oranges); if either path's score >= its
/// threshold, the check fires at that score. Returns 0.0 if neither
/// path clears.
fn check_semantic_score(check: &WayCandidate, session_id: &str, scores: &EmbedScores) -> f64 {
    let t = effective_thresholds(check, session_id);
    let en = scores
        .prob_en(&check.corpus_id, check.embeddable())
        .filter(|p| *p >= t.semantic);
    let mu = scores
        .prob_multi(&check.corpus_id, check.embeddable())
        .filter(|p| *p >= t.semantic);
    match (en, mu) {
        (Some(e), Some(m)) => e.max(m),
        (Some(s), None) | (None, Some(s)) => s,
        (None, None) => 0.0,
    }
}

/// Mask non-linguistic spans out of the text the keyword channel matches
/// (ADR-155 §2): fenced code blocks first (they often contain URLs), then
/// URLs. A pasted link containing "github" is not GitHub-workflow intent, and
/// pasted code is quoted material, not the user speaking. Each masked span is
/// replaced by a single space so word boundaries around it survive. The embed
/// lane sees the original text — the ADR-130 reducer already weighs pasted
/// content by sentence salience there. An unclosed fence is left as-is: better
/// to over-match than to blind the keyword channel to half the prompt.
fn mask_nonlinguistic(text: &str) -> String {
    let fenced = Regex::new(r"(?s)```.*?```").expect("static regex");
    let url = Regex::new(r"https?://\S+").expect("static regex");
    let no_fences = fenced.replace_all(text, " ");
    url.replace_all(&no_fences, " ").into_owned()
}

/// Compile a trigger pattern case-insensitively (ADR-157). The keyword lane
/// means the *concept*, not a casing — `\bssh\b` must match `SSH`, `\bpr\b`
/// must match `PR`. The flag lives on the compiled pattern rather than on the
/// text so deliberately-uppercase patterns (`SKILL\.md`) still match and the
/// captured span keeps its original case. A pattern may override with an inline
/// `(?-i)` scope if it ever needs case-sensitivity.
pub(crate) fn compile_trigger(pattern: &str) -> Option<Regex> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()
}

fn regex_matches(pattern: &str, text: &str) -> bool {
    compile_trigger(pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

/// The first regex match in `text`, length-capped for the event log (ADR-153 §3
/// `matched_span`). `None` if the pattern is invalid or doesn't match — mirroring
/// [`regex_matches`]' error tolerance. The cap bounds how much of the matched
/// input (a prompt/command/path fragment) lands in local telemetry; the pattern
/// itself is author-controlled, so a match is normally a bounded keyword.
fn regex_span(pattern: &str, text: &str) -> Option<String> {
    Some(capped_span(compile_trigger(pattern)?.find(text)?.as_str()))
}

/// A matched span cut to the event log's length cap ([`regex_span`]).
fn capped_span(s: &str) -> String {
    const MAX_SPAN: usize = 120;
    match s.char_indices().nth(MAX_SPAN) {
        Some((byte, _)) => format!("{}…", &s[..byte]),
        None => s.to_string(),
    }
}

/// Emit accumulated context in the canonical `hookSpecificOutput` envelope —
/// the only JSON shape the current hooks reference documents, for any event.
///
/// A bare top-level `additionalContext` was emitted for `SessionStart` here
/// until session transcripts proved the harness never delivered it: the
/// stdout landed in `hook_success` bookkeeping, no context attachment was
/// created, and the payload (the ways catalog and the core posture) reached
/// zero sessions on record. Undocumented JSON stdout is dropped silently, so
/// nothing but the canonical envelope belongs here.
///
/// The PreToolUse lanes ([`command`], [`file`]) made the same mistake with a
/// top-level `{"decision":"approve","additionalContext":…}`: over 30 days,
/// about 5,458 PreToolUse invocations delivered context 0 times, while
/// PostToolUse, already canonical, delivered 143 of 143 (#528). They route
/// through here now. The envelope deliberately carries no
/// `permissionDecision`: per the hooks reference
/// (<https://code.claude.com/docs/en/hooks.md>, "PreToolUse decision
/// control"), omitting it means the hook makes no decision and the tool call
/// continues through the normal permission flow. `"allow"` would bypass the
/// permission prompt, which a guidance hook must never do.
///
/// Callers keep `context` within [`crate::cmd::show::HOOK_CONTEXT_CAP`] via a
/// [`ContextBudget`]; over that cap Claude Code replaces the string with a
/// file path and a 2,000-character preview.
pub(crate) fn emit_hook_context(hook_event: &str, context: &str) {
    let payload = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": hook_event,
            "additionalContext": context,
        }
    });
    println!("{payload}");
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    // ── ADR-155 §2: masking the keyword lane ─────────────────────

    #[test]
    fn urls_are_masked_but_prose_survives() {
        let masked = mask_nonlinguistic(
            "inspired by https://github.com/example/flow and btop's graphs",
        );
        assert!(!masked.contains("github"), "URL text must not feed the regex lane");
        assert!(masked.contains("btop's graphs"), "prose survives masking");
        // Word boundaries around the masked span survive (replaced by a space,
        // so the neighbors never fuse into one token).
        assert!(!masked.contains("byand"), "masked span must not fuse neighbors: {masked:?}");
    }

    #[test]
    fn fenced_code_is_masked_including_urls_inside() {
        let masked = mask_nonlinguistic(
            "please review\n```\ngit remember = https://github.com/x\n```\nthe diff",
        );
        assert!(!masked.contains("remember"));
        assert!(!masked.contains("github"));
        assert!(masked.contains("please review"));
        assert!(masked.contains("the diff"));
    }

    #[test]
    fn unclosed_fence_is_left_intact() {
        let text = "start ```unclosed block with words";
        assert_eq!(mask_nonlinguistic(text), text);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn enabled_for_reads_the_project_switch() {
        let dir = std::env::temp_dir().join(format!("ways-enabled-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/ways.yaml"), "enabled: false\n").unwrap();
        assert!(!super::enabled_for(Some(dir.to_str().unwrap())));
        std::fs::write(dir.join(".claude/ways.yaml"), "ways: {}\n").unwrap();
        assert!(super::enabled_for(Some(dir.to_str().unwrap())));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ancestors_walk_up_nearest_first() {
        let got: Vec<&str> = super::ancestors("a/b/c").collect();
        assert_eq!(got, vec!["a/b", "a"]);
        assert_eq!(super::ancestors("top").count(), 0);
    }

    #[test]
    fn shown_ancestor_is_any_level_up() {
        let shown: std::collections::HashSet<String> = ["a".to_string()].into_iter().collect();
        assert!(super::has_shown_ancestor("a/b/c", &shown));
        assert!(!super::has_shown_ancestor("a", &shown), "a way is not its own ancestor");
        assert!(!super::has_shown_ancestor("ab/c", &shown), "a text prefix is not an ancestor");
    }
}

#[cfg(test)]
mod eligibility_tests {
    //! ADR-701 §1: the competing set is the ways that can fire on the lane.
    use super::*;

    pub(super) fn way(id: &str, scope: &str, trigger: Option<&str>) -> WayCandidate {
        WayCandidate {
            id: id.to_string(),
            corpus_id: id.to_string(),
            path: PathBuf::from(format!("/{id}.md")),
            pattern: None,
            pattern_strict: false,
            commands: None,
            files: None,
            tool_name: None,
            tool_event: None,
            description: "d".into(),
            vocabulary: "v".into(),
            threshold: 0.0,
            scope: scope.to_string(),
            when_project: None,
            when_file_exists: None,
            trigger: trigger.map(String::from),
            trigger_path: None,
        }
    }

    #[test]
    fn each_lane_admits_what_its_scan_loop_admits() {
        let agent = way("a", "agent", None);
        let sub = way("s", "subagent", None);
        let team = way("t", "teammate", None);
        let state = way("st", "subagent", Some("context-threshold"));
        let p = Lane::Prompt { scope: "agent" };
        assert!(eligible(&agent, p, "/p") && !eligible(&sub, p, "/p"));
        let task = Lane::Task { teammate: false };
        assert!(eligible(&sub, task, "/p") && !eligible(&agent, task, "/p") && !eligible(&team, task, "/p"));
        assert!(!eligible(&state, task, "/p"), "a state-triggered way skips the task lane");
        assert!(eligible(&team, Lane::Task { teammate: true }, "/p"));
    }

    #[test]
    fn a_when_precondition_that_fails_makes_a_way_ineligible() {
        let mut w = way("w", "agent", None);
        w.when_project = Some("/definitely/not/this/project".into());
        assert!(!eligible(&w, Lane::Prompt { scope: "agent" }, "/p"));
    }

    /// An agent-scope way that beats every chunk must not take share from the
    /// subagent-scope ways that can fire on the task lane.
    #[test]
    fn an_ineligible_by_scope_way_takes_no_share_on_the_task_lane() {
        let cands = [way("agent-only", "agent", None), way("a", "subagent", None), way("b", "subagent", None)];
        let row = |v: &[(&str, f64)]| v.iter().map(|(i, c)| (i.to_string(), *c)).collect::<Vec<_>>();
        let rows = vec![
            row(&[("agent-only", 0.9), ("a", 0.6), ("b", 0.5)]),
            row(&[("agent-only", 0.8), ("b", 0.55), ("a", 0.3)]),
        ];
        let lane = Lane::Task { teammate: false };
        let eligible_map = body_map(cands.iter().filter(|w| eligible(w, lane, "/p")));
        let got = late_interaction::shares_for_test(rows.clone(), &eligible_map);
        let absent: Vec<Vec<(String, f64)>> =
            rows.iter().map(|r| r.iter().filter(|(i, _)| i != "agent-only").cloned().collect()).collect();
        let want = late_interaction::shares_for_test(absent, &eligible_map);
        assert_eq!(got, want);
        assert!(got.iter().all(|(id, _)| id != "agent-only"));
        // With the old toggle-only set it would have competed.
        let all = body_map(cands.iter());
        assert!(late_interaction::shares_for_test(rows, &all).iter().any(|(id, _)| id == "agent-only"));
    }

    #[test]
    fn the_authoring_view_competes_the_prompt_lane_unless_asked_for_all() {
        let cands = [way("agent", "agent", None), way("sub", "subagent", None)];
        let ids = |unfiltered| diag_candidates(&cands, "/p", unfiltered).iter().map(|w| w.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(false), ["agent"], "default: what a prompt scan in agent scope competes");
        assert_eq!(ids(true), ["agent", "sub"]);
    }
}
