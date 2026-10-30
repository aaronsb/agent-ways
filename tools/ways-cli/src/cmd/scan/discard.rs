//! scan/discard.rs — switched-off ways that would have fired: discarded fires.
//!
//! A switched-off way is removed before matching: collection drops it and the
//! semantic lane masks it before the softmax, so it takes no share and no
//! survivor slot from an enabled way (ADR-701 §1). Each lane then runs its own
//! matcher over the switched-off ways on the scores it already holds, one way
//! at a time beside the enabled set, and logs each one that would have fired
//! as a discarded fire. Nothing is embedded for this pass. A way that would not
//! have fired logs nothing, so a disabled domain does not flood the event
//! stream. The relevance judge is never asked about a switched-off way.

use std::collections::{HashMap, HashSet};

use agent_settings::LayerScope;

use crate::config::Config;
use crate::session;

use super::late_interaction::{Shadow, Verdicts};
use super::matching::{match_prompt, PromptMatch};
use super::scoring::EmbedScores;
use super::{candidate_log, effective_thresholds, effective_thresholds_in_scan, WayCandidate};

/// What switched a way off.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Discard {
    /// `toggle`, `domain` or `project_off`.
    pub filter: &'static str,
    /// The toggle key that won, when the filter is `toggle`.
    pub toggle: Option<String>,
    /// The config layer holding the switch (`user` or `project`), when known.
    pub scope: Option<&'static str>,
}

/// A target's own config layer belongs to the operator, not the project.
fn layer_name(layer: LayerScope) -> &'static str {
    match layer {
        LayerScope::Project => "project",
        LayerScope::User | LayerScope::Target => "user",
    }
}

impl Discard {
    /// Why `cfg` switches `way_id` off, or `None` when it is on. A disabled
    /// domain wins over a toggle.
    pub(crate) fn of(cfg: &Config, way_id: &str) -> Option<Self> {
        let domain = way_id.split('/').next().unwrap_or(way_id);
        if cfg.disabled_domains.iter().any(|d| d == domain) {
            return Some(Discard { filter: "domain", toggle: None, scope: cfg.disabled_domains_layer.map(layer_name) });
        }
        // Toggles are read from the project layer only (ADR-131).
        let key = cfg.disabling_toggle(way_id)?;
        Some(Discard { filter: "toggle", toggle: Some(key.to_string()), scope: Some("project") })
    }

    /// `enabled: false`, which switches every way off.
    pub(crate) fn project_off(cfg: &Config) -> Self {
        Discard { filter: "project_off", toggle: None, scope: cfg.enabled_layer.map(layer_name) }
    }
}

/// Why `way_id` is switched off under the process config.
pub(crate) fn reason(way_id: &str) -> Option<Discard> {
    Discard::of(crate::config::global(), way_id)
}

/// The switched-off ways a collection found, parsed from their frontmatter.
#[derive(Default)]
pub(crate) struct Switched {
    pub ways: Vec<WayCandidate>,
    why: HashMap<String, Discard>,
}

impl Switched {
    pub(super) fn push(&mut self, way: WayCandidate, why: Discard) {
        self.why.insert(way.id.clone(), why);
        self.ways.push(way);
    }

    fn why(&self, id: &str) -> Option<&Discard> {
        self.why.get(id)
    }
}

/// Where a lane logs its discards.
pub(crate) struct LaneLog<'a> {
    pub lane: &'static str,
    /// The session's scope, as `way_fired` carries it.
    pub scope: &'a str,
    pub session: &'a str,
    pub project: &'a str,
}

/// What would have fired a switched-off way.
pub(crate) struct Hit<'a> {
    pub way: &'a str,
    /// The trigger channel, as `way_fired` names it.
    pub trigger: &'a str,
    pub span: Option<&'a str>,
    /// The semantic scores, on a semantic channel.
    pub scores: Option<Shadow>,
}

/// Log a discarded fire for a way of `off`.
pub(crate) fn log(off: &Switched, at: &LaneLog<'_>, hit: &Hit<'_>) {
    if let Some(why) = off.why(hit.way) {
        log_one(hit, why, at);
    }
}

/// Log a discarded fire: a `way_fired` event whose `discarded` object names
/// the switch, so fire statistics count it and readers of delivered fires
/// leave it out. It carries a delivered fire's leading fields, the lane, and on
/// a semantic channel the scores under the candidate log's names
/// (scan/candidate_log.rs).
pub(crate) fn log_one(hit: &Hit<'_>, why: &Discard, at: &LaneLog<'_>) {
    let domain = hit.way.split('/').next().unwrap_or(hit.way);
    let token_position = session::get_token_position(at.session).to_string();
    let model = crate::cmd::show::firing_model(at.session, at.project);
    let agent = session::current_agent();
    let mut fields: Vec<(&str, &str)> = vec![
        ("event", "way_fired"),
        ("way", hit.way),
        ("domain", domain),
        ("trigger", hit.trigger),
        ("scope", at.scope),
        ("project", at.project),
        ("session", at.session),
        ("token_position", &token_position),
        ("model", &model),
        ("agent_id", &agent),
        ("lane", at.lane),
    ];
    if let Some(span) = hit.span.filter(|s| !s.is_empty()) {
        fields.push(("matched_span", span));
    }
    let mut discarded = serde_json::json!({ "filter": why.filter });
    if let Some(key) = why.toggle.as_deref() {
        discarded["toggle"] = key.into();
    }
    if let Some(scope) = why.scope {
        discarded["scope"] = scope.into();
    }
    let round = |v: f64| serde_json::json!((v * 10_000.0).round() / 10_000.0);
    let mut extra: Vec<(&str, serde_json::Value)> = vec![("discarded", discarded)];
    if let Some(s) = hit.scores {
        extra.extend([("cosine", round(s.cosine)), ("share", round(s.share)), ("margin", s.margin.map_or(serde_json::Value::Null, round))]);
    }
    session::log_event_with(&fields, &extra);
}

/// The single-vector scores of `way` had it been enabled beside `enabled`
/// (corpus id to bare id), as the candidate log would record them.
pub(crate) fn single_scores(scores: &EmbedScores, enabled: &HashMap<&str, &str>, way: &WayCandidate) -> Option<Shadow> {
    let (_, rows) = candidate_log::scan_lane(scores)?;
    let mut with = enabled.clone();
    with.insert(&way.corpus_id, &way.id);
    candidate_log::ranked_candidates(rows, &with, usize::MAX)
        .into_iter()
        .find(|c| c.way == way.id)
        .map(|c| Shadow { cosine: c.cosine, share: c.share, margin: c.margin })
}

/// What a prompt-shaped lane matched its enabled ways against.
pub(crate) struct PromptShadow<'a> {
    pub query: &'a str,
    pub masked: &'a str,
    pub session: &'a str,
    pub scores: &'a EmbedScores,
    /// The prompt-only scores the keyword gate's re-check already computed,
    /// when the response contributed to `scores`. The shadow pass never
    /// computes them.
    pub recheck: Option<&'a EmbedScores>,
    /// The late-interaction verdicts, whose shadow pass decided each
    /// switched-off way's semantic channel.
    pub verdicts: Option<&'a Verdicts>,
    /// Ways fired in this scan, which boost their children, or `None` on a
    /// lane without the in-scan boost.
    pub fired: Option<&'a HashSet<String>>,
    /// The enabled competitors, corpus id to bare id.
    pub enabled: &'a HashMap<&'a str, &'a str>,
}

/// The prompt and task lanes' shadow pass: each eligible switched-off way
/// matched by `match_prompt` as an enabled way is, and logged when it fires.
/// A gated keyword hit stays gated unless the prompt-only scores already exist.
pub(crate) fn prompt_shaped(off: &Switched, eligible: impl Fn(&WayCandidate) -> bool, s: &PromptShadow<'_>, at: &LaneLog<'_>) {
    let cfg = crate::config::global();
    let shadow = s.verdicts.map(Verdicts::shadow);
    for way in off.ways.iter().filter(|w| eligible(w)) {
        let regex_text = if way.pattern_strict { s.query } else { s.masked };
        let thresholds = match s.fired {
            Some(fired) => effective_thresholds_in_scan(way, s.session, fired).0,
            None => effective_thresholds(way, s.session),
        };
        let run = |scores: &EmbedScores| {
            match_prompt(
                regex_text,
                &way.pattern,
                way.pattern_strict,
                way.embeddable(),
                &way.corpus_id,
                thresholds,
                scores,
                cfg.near_miss_margin,
                cfg.keyword_floor_probability,
                shadow.as_ref(),
            )
        };
        let mut outcome = run(s.scores);
        let mut scores = s.scores;
        if let (PromptMatch::KeywordGated(_), Some(recheck)) = (&outcome, s.recheck) {
            outcome = run(recheck);
            scores = recheck;
        }
        let PromptMatch::Fired { channel, score, matched_span } = outcome else { continue };
        let semantic = match s.verdicts {
            _ if score.is_none() => None,
            Some(v) => v.discarded(&way.corpus_id),
            None => single_scores(scores, s.enabled, way),
        };
        log(off, at, &Hit { way: &way.id, trigger: &channel, span: matched_span.as_deref(), scores: semantic });
    }
}

// ── A project switched off ─────────────────────────────────────

/// A hook surface in a project switched off with `enabled: false`.
pub enum OffSurface<'a> {
    Prompt { query: &'a str },
    Command { command: &'a str, description: Option<&'a str> },
    File { path: &'a str },
    Tool { tool: &'a str, hook_event: &'a str },
    Task { query: &'a str, team: Option<&'a str> },
}

/// The shadow pass for a project switched off as a whole. Every way is off,
/// and only the pattern channels are tested: the semantic lanes would need
/// an embedding the switched-off scan never makes. Nothing else of a scan
/// runs: no epoch, no decision record, no output.
pub fn project_off(surface: OffSurface<'_>, session_id: &str, project: Option<&str>) {
    let project_dir = project.map_or_else(crate::util::project_dir, str::to_string);
    let off = super::candidates::collect_all_off(&project_dir, &Discard::project_off(&Config::load(&project_dir)));
    if off.ways.is_empty() {
        return;
    }
    let scope = session::detect_scope(session_id);
    let none = EmbedScores { en: None, multi: None, calibration: Default::default() };
    let at = |lane, scope| LaneLog { lane, scope, session: session_id, project: &project_dir };
    match surface {
        OffSurface::Prompt { query } if !super::is_system_envelope(query) => {
            let lane = super::Lane::Prompt { scope: &scope };
            pattern_only(&off, query, session_id, &none, |w| super::eligible(w, lane, &project_dir), &at("prompt", &scope));
        }
        OffSurface::Prompt { .. } => {}
        OffSurface::Task { query, team } => {
            let lane = super::Lane::Task { teammate: team.is_some() };
            let task_scope = if team.is_some() { "teammate" } else { "subagent" };
            pattern_only(&off, query, session_id, &none, |w| super::eligible(w, lane, &project_dir), &at("task", task_scope));
        }
        OffSurface::Command { command, description } => {
            let command = super::ShellCommand::parse(command);
            let surface = super::CommandSurface { command: &command, description, session_id, scope: &scope, project_dir: &project_dir };
            super::command::log_discarded(&surface, &[], &off, &none);
        }
        OffSurface::File { path } => super::file::log_discarded(&off, path, &at("file", &scope)),
        OffSurface::Tool { tool, hook_event } => super::tool::log_discarded(&off, tool, hook_event, &at("tool", &scope)),
    }
}

/// [`prompt_shaped`] with no semantic signal: a keyword hit fires open, as it
/// does when the engine did not run.
fn pattern_only(off: &Switched, query: &str, session_id: &str, none: &EmbedScores, eligible: impl Fn(&WayCandidate) -> bool, at: &LaneLog<'_>) {
    let masked = super::mask_nonlinguistic(query);
    let enabled = HashMap::new();
    let shadow = PromptShadow { query, masked: &masked, session: session_id, scores: none, recheck: None, verdicts: None, fired: None, enabled: &enabled };
    prompt_shaped(off, eligible, &shadow, at);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(domains: &[&str], layer: Option<LayerScope>) -> Config {
        let mut c = Config::default();
        c.disabled_domains = domains.iter().map(|d| d.to_string()).collect();
        c.disabled_domains_layer = layer;
        c
    }

    #[test]
    fn a_disabled_domain_names_its_layer() {
        let c = cfg(&["lab"], Some(LayerScope::Project));
        assert_eq!(Discard::of(&c, "lab/x"), Some(Discard { filter: "domain", toggle: None, scope: Some("project") }));
        let c = cfg(&["lab"], Some(LayerScope::Target));
        assert_eq!(Discard::of(&c, "lab/x").and_then(|d| d.scope), Some("user"), "a target's layer is the operator's");
        assert_eq!(Discard::of(&c, "labx/y"), None, "a domain is the first path part, not a prefix");
    }

    #[test]
    fn an_enabled_way_has_no_discard() {
        assert_eq!(Discard::of(&Config::default(), "lab/x"), None);
    }
}
