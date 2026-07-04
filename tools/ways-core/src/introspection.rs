//! Session-introspection substrate (ADR-153 §2).
//!
//! One typed, serde-serializable model that joins the firing-event log to way
//! frontmatter: for a session, *which ways were injected, on which turn, and —
//! as far as the recorded data honestly allows — why*. It lives in `ways-core`
//! (not the CLI) because the ADR-201 finding pipeline shares this exact
//! correlation; building it once keeps findings and introspection from drifting.
//!
//! **Join honesty (ADR-153).** The turn ↔ fired-way link is *heuristic* until
//! fire-time enrichment (increment 3) records a transcript uuid: today the finest
//! signal is a `(session, ts, token_position)` bucket. Every [`Turn`] is labelled
//! [`JoinConfidence::Heuristic`] so a consumer never mistakes a proximity guess
//! for a foreign key, and [`Turn::transcript_uuid`] / [`FiredWay::match_detail`]
//! stay `None` until that enrichment lands.
//!
//! **No fabricated semantic "why."** There is one embedding per way, so a
//! semantic fire has no recoverable matched term — only a way-level
//! [`FiredWay::fire_score`]. `match_detail` will carry a span only for the
//! keyword/command/file channels, and only once enrichment records it.
//!
//! This model is the *analytical* substrate (why a way fired: criteria, matched
//! span, transcript key). The `rethink` **replay** pipeline (still in `ways-cli`)
//! stays a distinct *animation* projection — `build_frames` folds the full event
//! stream into cumulative "what's active at epoch N" frames. The two are **not**
//! unified into one clustering (decided 2026-07-03, ADR-153 module note / ADR-154
//! §1): they are different views serving different jobs, joined only where they
//! meet — the why-fired drill-down looks up a frame's focused way in this model
//! **by `way_id`**, which needs no epoch alignment.
//!
//! **The clustering shares the `≤3s` gap *rule* with `rethink::build_frames`, but
//! is deliberately not identical to it** — and, per the boundary above, is not
//! meant to be. Deliberate differences: this model clusters the *fire* stream only
//! (a [`Turn`] is defined by the ways that fired, not by the `session_start` /
//! `way_redisclosed` events the replay also folds in), sorts by timestamp, numbers
//! epochs over fire-bearing clusters only, and takes each turn's token position
//! from the event's own recorded value rather than a transcript-timeline lookup.
//! A consumer that needs the cumulative timeline reads `build_frames`; one that
//! needs the per-turn "why" reads this model; the drill-down bridges them by id.

use crate::util::parse_ts_secs;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ── Model ─────────────────────────────────────────────────────

/// How confident the join between a fired way and its turn is (ADR-153 §2).
/// `Keyed` = a foreign key (a transcript uuid) ties them; `Heuristic` = only a
/// time/token-bucket proximity guess. Never present `Heuristic` as if `Keyed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinConfidence {
    Keyed,
    Heuristic,
}

/// A way's fire-bearing match criteria, unified from the frontmatter fields that
/// decide whether a way fires. Consolidates the two inconsistent representations
/// that exist today (the typed `Frontmatter`, which omits pattern/files/commands/
/// trigger, and the untyped `WayCandidate` scan) into one honest surface.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MatchCriteria {
    /// Keyword channel — the regex matched against the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Bash channel — commands whose invocation fires the way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<String>,
    /// File channel — path globs whose touch fires the way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<String>,
    /// State / explicit trigger channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Semantic channel — the vocabulary the way's embedding is built from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<String>,
    /// Semantic firing threshold (way-level cosine cutoff).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_threshold: Option<f64>,
    /// Scope constraint (e.g. `subagent`, `teammate`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// What actually matched, when known (ADR-153 §3, forward-only enrichment).
/// Absent for every record today; the type exists so the model's shape is stable
/// once enrichment starts recording it. Semantic fires never get a `matched_span`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchDetail {
    /// The matched span (regex / glob / command hit) for keyword/command/file
    /// channels. `None` for semantic — there is no term to recover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_span: Option<String>,
    pub confidence: JoinConfidence,
}

/// One way injected into context at a turn, with why it fired — or, for a
/// `gated: true` row, a suppressed candidate: a pattern hit vetoed by the
/// semantic keyword gate (ADR-155), shown so "why didn't X fire" is
/// answerable from the session record. Gated rows inject nothing and are
/// excluded from the fire counts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FiredWay {
    pub way_id: String,
    /// The match *channel* the event recorded: `keyword` / `semantic:embedding:*`
    /// / `bash` / `file` / `state`. Gated rows carry `keyword:gated`. The
    /// mechanism, never the matched term.
    pub trigger_channel: String,
    /// `true` for a suppressed candidate (ADR-155 keyword gate) — the pattern
    /// matched (see `match_detail`) but the way did not fire.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gated: bool,
    /// Way-level cosine for semantic fires; `None` for deterministic channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub way_path: Option<String>,
    pub criteria: MatchCriteria,
    /// What hit — `None` until fire-time enrichment (increment 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_detail: Option<MatchDetail>,
}

/// One turn (an epoch cluster of fires) of a session.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Turn {
    pub epoch: u64,
    /// Recorded token position of the fires in this turn (the honest value the
    /// event carried; `0` for channels that don't record one, e.g. state fires).
    pub token_position: u64,
    pub ts: String,
    /// The triggering message id — `None` until enrichment (increment 3) turns the
    /// heuristic time-join into a foreign key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_uuid: Option<String>,
    /// Confidence of this turn's fired-way join. `Heuristic` until enrichment.
    pub join_confidence: JoinConfidence,
    pub fired_ways: Vec<FiredWay>,
}

/// Aggregate counts over a session's introspection.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct IntrospectionSummary {
    pub turns: usize,
    pub distinct_ways: usize,
    pub total_fires: u64,
    /// Pattern hits vetoed by the semantic keyword gate (ADR-155). Counted
    /// separately — a gated candidate injected nothing, so it never inflates
    /// `total_fires` or `distinct_ways`.
    #[serde(skip_serializing_if = "u64_is_zero")]
    pub gated_candidates: u64,
    /// Turns whose join was upgraded to `Keyed` by a transcript foreign key
    /// (ADR-153 §3). `0` until [`SessionIntrospection::join_transcript`] runs.
    pub keyed_turns: usize,
}

fn u64_is_zero(v: &u64) -> bool {
    *v == 0
}

/// A session's introspection: its turns and the ways each fired.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionIntrospection {
    pub id: String,
    pub project: String,
    pub window_k: u64,
    pub summary: IntrospectionSummary,
    pub turns: Vec<Turn>,
}

/// A way's resolved metadata: its file path and parsed criteria.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WayMeta {
    pub path: Option<String>,
    pub criteria: MatchCriteria,
}

/// Lookup from way id to its resolved metadata, built once per introspection.
pub type CriteriaMap = HashMap<String, WayMeta>;

// ── Build (the join) ──────────────────────────────────────────

impl SessionIntrospection {
    /// Build the introspection for one session from the raw event log, joining
    /// each `way_fired` event to its frontmatter criteria via `criteria`.
    ///
    /// The turn↔fire join is HEURISTIC — every [`Turn`] is labelled as such —
    /// until enrichment (ADR-153 §3) supplies a transcript uuid. Fires are grouped
    /// into turns by the same `≤3s` timestamp-gap *rule* `rethink::build_frames`
    /// uses (over the fire stream only — see the module note; the two are
    /// reconciled at increment 4, not assumed identical). A way absent from
    /// `criteria` still appears, with empty criteria — unknown, not invented.
    pub fn build(
        events: &[Value],
        session_id: &str,
        project: &str,
        window_k: u64,
        criteria: &CriteriaMap,
    ) -> Self {
        let mut fires: Vec<FireRow> = events
            .iter()
            .filter(|e| e["session"].as_str() == Some(session_id))
            .filter(|e| {
                matches!(
                    e["event"].as_str(),
                    Some("way_fired") | Some("way_keyword_gated")
                )
            })
            .filter_map(FireRow::from_value)
            .collect();
        fires.sort_by(|a, b| a.ts.cmp(&b.ts));

        let mut turns: Vec<Turn> = Vec::new();
        let mut epoch: u64 = 0;
        let mut cluster: Vec<&FireRow> = Vec::new();
        let mut last_secs: u64 = 0;

        for f in &fires {
            let secs = parse_ts_secs(&f.ts);
            if !cluster.is_empty() && secs > last_secs + 3 {
                epoch += 1;
                turns.push(build_turn(&cluster, epoch, criteria));
                cluster.clear();
            }
            cluster.push(f);
            last_secs = secs;
        }
        if !cluster.is_empty() {
            epoch += 1;
            turns.push(build_turn(&cluster, epoch, criteria));
        }

        // Gated candidates are shown in their turn but never counted as fires —
        // they injected nothing (ADR-155).
        let total_fires: u64 = turns
            .iter()
            .map(|t| t.fired_ways.iter().filter(|w| !w.gated).count() as u64)
            .sum();
        let gated_candidates: u64 = turns
            .iter()
            .map(|t| t.fired_ways.iter().filter(|w| w.gated).count() as u64)
            .sum();
        let distinct: HashSet<&str> = turns
            .iter()
            .flat_map(|t| t.fired_ways.iter().filter(|w| !w.gated).map(|w| w.way_id.as_str()))
            .collect();
        let summary = IntrospectionSummary {
            turns: turns.len(),
            distinct_ways: distinct.len(),
            total_fires,
            gated_candidates,
            keyed_turns: 0, // set by join_transcript once a transcript is joined
        };

        SessionIntrospection {
            id: session_id.to_string(),
            project: project.to_string(),
            window_k,
            summary,
            turns,
        }
    }

    /// Convenience: build for a session from the live event log (the increment-1
    /// union reader) and the project-aware way corpus (so a fired way resolves to
    /// the criteria that actually fired, honoring ADR-143 override precedence),
    /// then upgrade the join with the session transcript when it's present.
    pub fn from_session(session_id: &str, project: &str, window_k: u64) -> Self {
        let events = crate::firing::load_events();
        let criteria = project_criteria_map(project);
        let mut s = Self::build(&events, session_id, project, window_k, &criteria);
        s.join_transcript(&crate::transcript::prompt_turns(project, session_id));
        s
    }

    /// Attach `transcript_uuid` to each turn by matching its fire timestamp to the
    /// nearest `UserPromptSubmit` injection, flipping matched turns
    /// `Heuristic`→`Keyed` (ADR-153 §3). The turn→injection match is the one
    /// heuristic step (session + sub-second timestamp — the fire happens *inside*
    /// the hook); the injection→message link the [`crate::transcript`] reader
    /// already followed is keyed. Turns with no injection within tolerance keep
    /// their `Heuristic` label. Passing an empty slice (absent transcript) is a
    /// no-op — the model stays honestly heuristic.
    pub fn join_transcript(&mut self, prompt_turns: &[crate::transcript::PromptTurn]) {
        for turn in &mut self.turns {
            if let Some(pt) = nearest_prompt_turn(&turn.ts, prompt_turns) {
                turn.transcript_uuid = Some(pt.user_uuid.clone());
                turn.join_confidence = JoinConfidence::Keyed;
            }
        }
        self.summary.keyed_turns = self
            .turns
            .iter()
            .filter(|t| t.join_confidence == JoinConfidence::Keyed)
            .count();
    }
}

/// The tolerance for matching a turn's fire timestamp to a prompt injection. The
/// injection is recorded when the hook returns — at/just after the fires it
/// carries — so a few seconds covers hook runtime and clock skew without reaching
/// an adjacent prompt.
const JOIN_TOLERANCE_SECS: u64 = 8;

/// The prompt injection whose timestamp is closest to `turn_ts`, within tolerance
/// — but only when the match is **unambiguous**. If two prompts submitted within
/// tolerance of each other collapse into one turn (fires ≤3s apart cluster, and a
/// rapid re-submit can land inside that window), their injections resolve to
/// *different* user messages; there is then no single correct key for the turn, so
/// this returns `None` and the turn stays `Heuristic`. `Keyed` thus always means an
/// unambiguous foreign key — never a coin-flip between two candidate messages.
fn nearest_prompt_turn<'a>(
    turn_ts: &str,
    prompt_turns: &'a [crate::transcript::PromptTurn],
) -> Option<&'a crate::transcript::PromptTurn> {
    let turn_secs = parse_ts_secs(turn_ts);
    let in_tolerance: Vec<(&crate::transcript::PromptTurn, u64)> = prompt_turns
        .iter()
        .map(|pt| (pt, parse_ts_secs(&pt.ts).abs_diff(turn_secs)))
        .filter(|(_, diff)| *diff <= JOIN_TOLERANCE_SECS)
        .collect();

    // Ambiguity guard: ≥2 candidates pointing at different messages → don't key.
    let distinct: HashSet<&str> = in_tolerance.iter().map(|(pt, _)| pt.user_uuid.as_str()).collect();
    if distinct.len() > 1 {
        return None;
    }
    in_tolerance
        .into_iter()
        .min_by_key(|(_, diff)| *diff)
        .map(|(pt, _)| pt)
}

fn build_turn(cluster: &[&FireRow], epoch: u64, criteria: &CriteriaMap) -> Turn {
    let ts = cluster.first().map(|f| f.ts.clone()).unwrap_or_default();
    let token_position = cluster.iter().map(|f| f.token_position).max().unwrap_or(0);
    let fired_ways = cluster
        .iter()
        .map(|f| {
            let meta = criteria.get(&f.way).cloned().unwrap_or_default();
            FiredWay {
                way_id: f.way.clone(),
                trigger_channel: f.trigger.clone(),
                gated: f.gated,
                fire_score: f.fire_score,
                way_path: meta.path,
                criteria: meta.criteria,
                // The matched span is a fire-time record of exactly what hit, so
                // it is itself Keyed when present (ADR-153 §3). Absent for semantic
                // (no term) and for pre-enrichment records.
                match_detail: f.matched_span.clone().map(|span| MatchDetail {
                    matched_span: Some(span),
                    confidence: JoinConfidence::Keyed,
                }),
            }
        })
        .collect();
    Turn {
        epoch,
        token_position,
        ts,
        transcript_uuid: None,                    // enrichment: increment 3
        join_confidence: JoinConfidence::Heuristic,
        fired_ways,
    }
}

/// A single `way_fired` row, projected from the JSONL. `log_event` writes every
/// field as a JSON *string*, so numeric fields are parsed from strings (with a
/// number fallback for robustness).
struct FireRow {
    ts: String,
    way: String,
    trigger: String,
    fire_score: Option<f64>,
    token_position: u64,
    matched_span: Option<String>,
    /// `true` for a `way_keyword_gated` event (ADR-155): a suppressed
    /// candidate, not a fire.
    gated: bool,
}

impl FireRow {
    fn from_value(v: &Value) -> Option<Self> {
        let way = v["way"].as_str().filter(|s| !s.is_empty())?;
        let gated = v["event"].as_str() == Some("way_keyword_gated");
        Some(FireRow {
            ts: v["ts"].as_str().unwrap_or("").to_string(),
            way: way.to_string(),
            // The gated event's `trigger` field records the surface
            // (prompt/task, the near-miss convention); the channel shown is
            // the gate's own name so a suppressed row is unmistakable.
            trigger: if gated {
                "keyword:gated".to_string()
            } else {
                v["trigger"].as_str().unwrap_or("").to_string()
            },
            fire_score: str_or_num_f64(&v["fire_score"]),
            token_position: str_or_num_u64(&v["token_position"]).unwrap_or(0),
            matched_span: v["matched_span"].as_str().map(|s| s.to_string()),
            gated,
        })
    }
}

fn str_or_num_f64(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64())
}

fn str_or_num_u64(v: &Value) -> Option<u64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())
}

// ── Criteria parsing ──────────────────────────────────────────

impl MatchCriteria {
    /// Parse the fire-bearing fields from a way's raw frontmatter YAML. Reads each
    /// field leniently and per-field, so an unexpected type on one field never
    /// nukes the rest, and list-valued fields (`files: [a, b]`) join with `, `.
    pub fn from_frontmatter_str(fm: &str) -> Self {
        let map: Value = serde_yaml::from_str::<serde_yaml::Value>(fm)
            .ok()
            .and_then(|y| serde_json::to_value(y).ok())
            .unwrap_or(Value::Null);
        let get = |k: &str| flex_string(&map[k]);
        MatchCriteria {
            pattern: get("pattern"),
            commands: get("commands"),
            files: get("files"),
            trigger: get("trigger"),
            vocabulary: get("vocabulary"),
            embed_threshold: str_or_num_f64(&map["embed_threshold"]),
            scope: get("scope"),
        }
    }
}

/// A scalar (string/number/bool) → its text; a sequence → its items joined with
/// `, `; anything else (map/null/missing/empty seq) → `None`.
fn flex_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(seq) => {
            let parts: Vec<String> = seq.iter().filter_map(flex_string).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        _ => None,
    }
}

/// The YAML frontmatter block of a way file — the text between the first two
/// `---` fences. `None` if the file doesn't open with a fence.
fn frontmatter_block(content: &str) -> Option<String> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

// ── Corpus resolver ───────────────────────────────────────────

/// Build a [`CriteriaMap`] by scanning way roots once. First writer wins per id,
/// so pass roots **highest-precedence first** (ADR-143: project > user > core, a
/// user way shadows the core way of the same id). Unreadable roots/files skipped.
///
/// Recognition matches the *fire path* (`scan::candidates`), not the stricter
/// [`crate::scanner`]: any `.md` with a frontmatter block is a way (skipping
/// `.check.` re-fire files). Requiring a `description:` — as `scanner` does —
/// would miss trigger-only ways like `meta/todos`, which fire but resolve to
/// nothing. The key is the *domain-prefixed* id the fire sites record
/// (`util::path_to_id` of the way file's parent dir — the same derivation
/// `candidates::way_id_from_path` uses), so it joins the event `way` field.
///
/// A way whose id has since changed on disk (corpus evolution) simply won't be
/// found by an old event referencing the former id — that fire keeps empty
/// criteria (unknown, not invented), which the model surfaces honestly.
pub fn build_criteria_map(roots: &[PathBuf]) -> CriteriaMap {
    let mut map = CriteriaMap::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains(".check."))
            {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            if !crate::util::has_frontmatter(&content) {
                continue;
            }
            let Some(rel) = path.parent().and_then(|p| p.strip_prefix(root).ok()) else {
                continue;
            };
            let id = crate::util::path_to_id(rel);
            if id.is_empty() || map.contains_key(&id) {
                continue;
            }
            let criteria = frontmatter_block(&content)
                .map(|fm| MatchCriteria::from_frontmatter_str(&fm))
                .unwrap_or_default();
            map.insert(
                id,
                WayMeta {
                    path: Some(path.to_string_lossy().into_owned()),
                    criteria,
                },
            );
        }
    }
    map
}

/// The env-independent corpus in ADR-143 precedence order: user shadows core.
pub fn default_criteria_map() -> CriteriaMap {
    build_criteria_map(&[
        crate::paths::user_ways_root(),
        crate::paths::core_ways_root(),
    ])
}

/// The full ADR-143 corpus for a given project: project ways shadow user, user
/// shadows core — so a fired way resolves to the criteria that actually fired.
/// An empty `project` (unknown project) resolves user+core only — never a
/// *relative* `.claude/ways` that would fold in whatever repo the CWD happens to
/// be inside.
pub fn project_criteria_map(project: &str) -> CriteriaMap {
    if project.is_empty() {
        return default_criteria_map();
    }
    build_criteria_map(&[
        PathBuf::from(project).join(".claude").join("ways"),
        crate::paths::user_ways_root(),
        crate::paths::core_ways_root(),
    ])
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn crit(pattern: &str) -> MatchCriteria {
        MatchCriteria {
            pattern: Some(pattern.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn criteria_parses_lenient_fields() {
        let fm = "\
description: A way
pattern: foo|bar
files:
  - src/**/*.rs
  - README.md
vocabulary: commit changes
embed_threshold: 0.42
scope: subagent
";
        let c = MatchCriteria::from_frontmatter_str(fm);
        assert_eq!(c.pattern.as_deref(), Some("foo|bar"));
        assert_eq!(c.files.as_deref(), Some("src/**/*.rs, README.md")); // list joined
        assert_eq!(c.vocabulary.as_deref(), Some("commit changes"));
        assert_eq!(c.embed_threshold, Some(0.42));
        assert_eq!(c.scope.as_deref(), Some("subagent"));
        assert!(c.commands.is_none()); // absent stays None
    }

    #[test]
    fn criteria_tolerates_missing_and_empty() {
        assert_eq!(MatchCriteria::from_frontmatter_str(""), MatchCriteria::default());
        // A bad type on one field must not nuke the others.
        let c = MatchCriteria::from_frontmatter_str("pattern: ok\nfiles: {a: 1}\n");
        assert_eq!(c.pattern.as_deref(), Some("ok"));
        assert!(c.files.is_none()); // a map is not a fire-bearing string
    }

    #[test]
    fn frontmatter_block_extracts_between_fences() {
        let doc = "---\npattern: x\nscope: y\n---\n# body\ntext\n";
        let fm = frontmatter_block(doc).unwrap();
        assert_eq!(fm, "pattern: x\nscope: y");
        assert!(frontmatter_block("no frontmatter here").is_none());
    }

    #[test]
    fn build_clusters_turns_and_labels_heuristic() {
        // Two fires 1s apart (one turn), then a third 10s later (a second turn).
        // A fire from another session and a non-fire event must be excluded.
        let events = vec![
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"keyword","token_position":"100","matched_span":"commit"}),
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:00:01Z","way":"d/b","trigger":"semantic:embedding:en","fire_score":"0.73","token_position":"120"}),
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:00:11Z","way":"d/a","trigger":"keyword","token_position":"400"}),
            json!({"event":"way_fired","session":"other","ts":"2026-01-01T00:00:01Z","way":"d/z","trigger":"keyword"}),
            json!({"event":"session_start","session":"s1","ts":"2026-01-01T00:00:00Z"}),
        ];
        let mut criteria = CriteriaMap::new();
        criteria.insert("d/a".into(), WayMeta { path: Some("/w/a.md".into()), criteria: crit("aaa") });
        // d/b intentionally absent → resolves to empty criteria, not a panic.

        let s = SessionIntrospection::build(&events, "s1", "/proj", 200, &criteria);

        assert_eq!(s.id, "s1");
        assert_eq!(s.turns.len(), 2, "two epoch clusters");
        assert_eq!(s.summary.turns, 2);
        assert_eq!(s.summary.total_fires, 3, "other-session fire excluded");
        assert_eq!(s.summary.distinct_ways, 2, "d/a and d/b");

        let t0 = &s.turns[0];
        assert_eq!(t0.epoch, 1);
        assert_eq!(t0.token_position, 120, "max token_position in the cluster");
        assert_eq!(t0.join_confidence, JoinConfidence::Heuristic);
        assert_eq!(t0.transcript_uuid, None);
        assert_eq!(t0.fired_ways.len(), 2);

        // Resolved way carries its criteria + path; semantic carries a score.
        let a = &t0.fired_ways[0];
        assert_eq!(a.way_id, "d/a");
        assert_eq!(a.trigger_channel, "keyword");
        assert_eq!(a.fire_score, None);
        assert_eq!(a.way_path.as_deref(), Some("/w/a.md"));
        assert_eq!(a.criteria.pattern.as_deref(), Some("aaa"));
        // The recorded matched_span becomes a Keyed MatchDetail.
        let md = a.match_detail.as_ref().expect("matched_span → match_detail");
        assert_eq!(md.matched_span.as_deref(), Some("commit"));
        assert_eq!(md.confidence, JoinConfidence::Keyed);

        let b = &t0.fired_ways[1];
        assert_eq!(b.way_id, "d/b");
        assert_eq!(b.fire_score, Some(0.73)); // parsed from string
        assert_eq!(b.criteria, MatchCriteria::default()); // unresolved → empty
        assert!(b.match_detail.is_none(), "semantic fire has no matched span");

        assert_eq!(s.turns[1].epoch, 2);
        assert_eq!(s.turns[1].token_position, 400);
    }

    #[test]
    fn gated_candidates_join_as_suppressed_rows_not_fires() {
        // ADR-155: a way_keyword_gated event lands in its turn as a gated row
        // with the gate's own channel label, but never counts as a fire.
        let events = vec![
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"keyword","token_position":"100","matched_span":"commit"}),
            json!({"event":"way_keyword_gated","session":"s1","ts":"2026-01-01T00:00:01Z","way":"d/g","trigger":"prompt","matched_span":"remember","score_en":"0.1450","floor_en":"0.2000"}),
        ];
        let s = SessionIntrospection::build(&events, "s1", "/proj", 200, &CriteriaMap::new());

        assert_eq!(s.summary.total_fires, 1, "gated row is not a fire");
        assert_eq!(s.summary.distinct_ways, 1, "gated way not counted as fired");
        assert_eq!(s.summary.gated_candidates, 1);

        let t0 = &s.turns[0];
        assert_eq!(t0.fired_ways.len(), 2, "gated row still shown in its turn");
        let g = t0.fired_ways.iter().find(|w| w.gated).expect("gated row present");
        assert_eq!(g.way_id, "d/g");
        assert_eq!(g.trigger_channel, "keyword:gated");
        let md = g.match_detail.as_ref().expect("gated row records what hit");
        assert_eq!(md.matched_span.as_deref(), Some("remember"));
        assert!(!t0.fired_ways.iter().find(|w| w.way_id == "d/a").unwrap().gated);
    }

    #[test]
    fn criteria_map_keys_match_event_ids_and_first_root_wins() {
        let base = std::env::temp_dir().join(format!("ways-critmap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let hi = base.join("hi");
        let lo = base.join("lo");
        let write = |root: &std::path::Path, rel: &str, body: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
        };
        write(&hi, "api-design/dual-modes/dual-modes.md", "---\ndescription: d\npattern: HIGH\n---\nx\n");
        write(&lo, "api-design/dual-modes/dual-modes.md", "---\ndescription: d\npattern: LOW\n---\nx\n");
        write(&lo, "build/build.md", "---\ndescription: d\npattern: B\n---\nx\n");
        // A trigger-only way with NO `description:` — fires in reality, so it must
        // resolve here (the recognition bug that missed `meta/todos`).
        write(&lo, "meta/todos/todos.md", "---\ntrigger: context-threshold\nthreshold: 75\n---\nx\n");
        // A `.check.` re-fire file must be skipped.
        write(&lo, "meta/todos/todos.check.md", "---\ndescription: check\n---\nx\n");

        let map = build_criteria_map(&[hi.clone(), lo.clone()]);
        // Keyed by the domain-prefixed id the fire sites record — not the
        // scanner's domain-stripped id — or the join to events resolves nothing.
        let dm = map.get("api-design/dual-modes").expect("keyed by domain-prefixed id");
        assert_eq!(dm.criteria.pattern.as_deref(), Some("HIGH"), "first root shadows later");
        assert!(!map.contains_key("dual-modes"), "domain-stripped id must not be a key");
        // A top-level way keeps the bare domain as its id (matches events like `build`).
        assert_eq!(map.get("build").and_then(|m| m.criteria.pattern.as_deref()), Some("B"));
        // The description-less trigger way resolves, carrying its trigger criterion.
        let todos = map.get("meta/todos").expect("trigger-only way must resolve");
        assert_eq!(todos.criteria.trigger.as_deref(), Some("context-threshold"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn join_transcript_keys_matching_turns_only() {
        use crate::transcript::PromptTurn;
        // Two turns, 5 min apart (distinct epochs).
        let events = vec![
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"keyword"}),
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:05:00Z","way":"d/b","trigger":"keyword"}),
        ];
        let mut s = SessionIntrospection::build(&events, "s1", "/p", 200, &CriteriaMap::new());
        assert_eq!(s.turns.len(), 2);
        assert!(s.turns.iter().all(|t| t.join_confidence == JoinConfidence::Heuristic));

        // One injection ~1s after the first turn; nothing near the second.
        let pts = vec![PromptTurn {
            ts: "2026-01-01T00:00:01Z".into(),
            user_uuid: "user-1".into(),
        }];
        s.join_transcript(&pts);

        assert_eq!(s.turns[0].join_confidence, JoinConfidence::Keyed);
        assert_eq!(s.turns[0].transcript_uuid.as_deref(), Some("user-1"));
        assert_eq!(s.turns[1].join_confidence, JoinConfidence::Heuristic, "no injection within tolerance");
        assert_eq!(s.turns[1].transcript_uuid, None);
        assert_eq!(s.summary.keyed_turns, 1);

        // Absent transcript (empty slice) is a no-op — stays as-is.
        let mut s2 = SessionIntrospection::build(&events, "s1", "/p", 200, &CriteriaMap::new());
        s2.join_transcript(&[]);
        assert_eq!(s2.summary.keyed_turns, 0);
        assert!(s2.turns.iter().all(|t| t.join_confidence == JoinConfidence::Heuristic));
    }

    #[test]
    fn join_leaves_ambiguous_turns_heuristic() {
        use crate::transcript::PromptTurn;
        let events = vec![
            json!({"event":"way_fired","session":"s1","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"keyword"}),
        ];
        // Two DIFFERENT prompts within tolerance of the one turn → can't attribute.
        let ambiguous = vec![
            PromptTurn { ts: "2026-01-01T00:00:01Z".into(), user_uuid: "user-A".into() },
            PromptTurn { ts: "2026-01-01T00:00:02Z".into(), user_uuid: "user-B".into() },
        ];
        let mut s = SessionIntrospection::build(&events, "s1", "/p", 200, &CriteriaMap::new());
        s.join_transcript(&ambiguous);
        assert_eq!(s.turns[0].join_confidence, JoinConfidence::Heuristic, "ambiguous → not keyed");
        assert_eq!(s.turns[0].transcript_uuid, None);
        assert_eq!(s.summary.keyed_turns, 0);

        // Two injections resolving to the SAME message are not ambiguous — key it.
        let same = vec![
            PromptTurn { ts: "2026-01-01T00:00:01Z".into(), user_uuid: "user-A".into() },
            PromptTurn { ts: "2026-01-01T00:00:02Z".into(), user_uuid: "user-A".into() },
        ];
        let mut s2 = SessionIntrospection::build(&events, "s1", "/p", 200, &CriteriaMap::new());
        s2.join_transcript(&same);
        assert_eq!(s2.turns[0].join_confidence, JoinConfidence::Keyed);
        assert_eq!(s2.turns[0].transcript_uuid.as_deref(), Some("user-A"));
    }

    #[test]
    fn build_on_empty_events_is_empty() {
        let s = SessionIntrospection::build(&[], "s1", "/p", 200, &CriteriaMap::new());
        assert_eq!(s.turns.len(), 0);
        assert_eq!(s.summary, IntrospectionSummary::default());
    }
}
