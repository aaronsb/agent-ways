//! Display ways, checks, and core guidance — session-aware, idempotent.
//!
//! Replaces: show-way.sh, show-check.sh, show-core.sh

mod helpers;
mod metrics;
mod pull;
pub use pull::pull;

use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::OnceLock;

use crate::{frontmatter, session};
use helpers::{extract_attend_signals, check_sections_text, run_macro, MacroRun};
pub(crate) use helpers::{is_executable, is_project_trusted};
use crate::frontmatter::body_text;
use metrics::{compute_tree_metrics, count_siblings, git_version, dirty_status_text, update_status_text};

// ── ways show way ───────────────────────────────────────────────

pub fn way(id: &str, session_id: &str, trigger: &str) -> Result<String> {
    way_scored(id, session_id, trigger, None, None, None, None)
}

// ── Hook context budget ─────────────────────────────────────────

/// Claude Code's cap on one hook's `additionalContext` string. From the hooks
/// reference (<https://code.claude.com/docs/en/hooks.md>, "JSON output"): each
/// `additionalContext` is capped at 10,000 characters, and over the limit
/// Claude Code saves it to a file and hands the model the path plus a preview
/// of the first 2,000 characters. One `git commit` once matched 17,832
/// characters across seven ways, so most of that never reached the model.
pub const HOOK_CONTEXT_CAP: usize = 10_000;

/// Length as Claude Code counts it: a JavaScript string length, which is
/// UTF-16 code units. Never less than the `char` count, so it errs short.
pub fn context_chars(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The static text `way_scored` injects for a way: the body after the
/// frontmatter. Macro output is added at fire time and is not part of it.
/// `ways author lint` measures this against [`HOOK_CONTEXT_CAP`], so the show path and
/// the size rule read one definition of a way's delivered body.
pub fn static_way_body(content: &str) -> String {
    body_text(content)
}

/// Room left in one hook invocation's `additionalContext`.
///
/// A scan lane hands one budget to every way and check it shows, in the
/// admission order `scan/order.rs` sets. A unit that fits is admitted whole. A
/// unit that does not fit is skipped: it is withheld and logged, and admission
/// continues with the later candidates, so a smaller way further down can
/// still use the room left. A withheld way is not recorded as fired, so its
/// refire curve does not start for guidance the model never saw, and it is
/// free to fire on its next match. Bodies are never split.
///
/// A check and the parent way it pulls in are one unit: the check reserves
/// room for its sections while the parent is admitted, and the pair goes out
/// together or not at all.
///
/// The first unit is always admitted, even when it alone is over the cap.
/// Dropping it would withhold that way on every match for good; admitting it
/// gets Claude Code's file-plus-preview handling, the result before the budget.
#[derive(Debug)]
pub struct ContextBudget {
    used: usize,
    cap: usize,
    /// Room held back for text that must follow the next admitted body.
    reserved: usize,
    /// Units refused in this invocation. Callers read it to tell a withhold
    /// for the cap apart from a way that returned nothing for another reason.
    refusals: usize,
}

impl ContextBudget {
    pub fn new(cap: usize) -> Self {
        Self { used: 0, cap, reserved: 0, refusals: 0 }
    }

    /// A budget sized to Claude Code's hook `additionalContext` cap.
    pub fn hook() -> Self {
        Self::new(HOOK_CONTEXT_CAP)
    }

    fn fits_chars(&self, n: usize) -> bool {
        self.used == 0 || self.used + n + self.reserved <= self.cap
    }

    /// Whether `text` would be admitted now. Charges nothing.
    pub fn fits(&self, text: &str) -> bool {
        self.fits_chars(context_chars(text))
    }

    /// Admit `text` whole if it fits, and charge it. A refusal is counted and
    /// leaves the budget open for later, smaller units.
    pub fn admit(&mut self, text: &str) -> bool {
        let n = context_chars(text);
        if self.fits_chars(n) {
            self.used += n;
            true
        } else {
            self.refusals += 1;
            false
        }
    }

    /// Count a refusal the caller decided without calling `admit` (a static
    /// body that cannot fit is refused before its macro runs).
    pub fn refuse(&mut self) {
        self.refusals += 1;
    }

    /// Units refused so far in this invocation.
    pub fn refusals(&self) -> usize {
        self.refusals
    }

    /// Charge text the caller adds between admitted bodies (separators).
    pub fn charge(&mut self, text: &str) {
        self.used += context_chars(text);
    }

    /// Hold back room for `text`, which must follow the next admitted body.
    pub fn reserve(&mut self, text: &str) {
        self.reserved = context_chars(text);
    }

    /// Spend the reservation: the body it was held for was admitted.
    pub fn commit_reservation(&mut self) {
        self.used += self.reserved;
        self.reserved = 0;
    }

    /// Drop the reservation unspent.
    pub fn cancel_reservation(&mut self) {
        self.reserved = 0;
    }
}

/// Why a matched way or check was not shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Suppression {
    /// The way's refire curve (ADR-126) still holds it back.
    Refire,
    /// The hook's context budget had no room for it.
    ContextCap,
}

impl Suppression {
    fn reason(self) -> &'static str {
        match self {
            Self::Refire => "refire",
            Self::ContextCap => "context_cap",
        }
    }
}

/// Record a way or check that matched but was not shown, so the refire
/// curve's work and the context cap's are countable in the event log. Cheap
/// by design: no transcript read and no engagement reload.
///
/// `kind` is `way` or `check`. Refire rows are deduplicated by the caller to
/// one per way per fire window; context-cap rows are rare and log as they
/// happen.
#[allow(clippy::too_many_arguments)]
fn log_way_suppressed(
    kind: &str,
    id: &str,
    domain: &str,
    trigger: &str,
    why: Suppression,
    scope: &str,
    project_dir: &str,
    session_id: &str,
) {
    let agent_id = session::current_agent();
    session::log_event(&[
        ("event", "way_suppressed"),
        ("kind", kind),
        ("way", id),
        ("domain", domain),
        ("trigger", trigger),
        ("reason", why.reason()),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
        ("agent_id", &agent_id),
    ]);
}

/// Whether a resolved context actually detected its window, rather than falling
/// back to `DEFAULT_WINDOW`. `EnvOverride` and `ModelTable` are detections;
/// `Default` is the absence of one (ADR-166).
fn window_detected(ctx: &crate::cmd::context::ContextInfo) -> bool {
    ctx.window_source != ways_core::context_window::WindowSource::Default
}

/// The transcript the invoking hook named (`transcript_path` in its payload),
/// handed in as `--transcript` by the scan lanes. One `ways` process serves one
/// hook invocation, so this is process state rather than a parameter threaded
/// through every scan lane and into [`way_scored`]. Unset when the hook
/// predates the flag or the caller is a dry run; the firing path then falls
/// back to the session-id lookup exactly as before.
static FIRING_TRANSCRIPT: OnceLock<String> = OnceLock::new();

/// Normalise a `--transcript` value: an empty string is no path (a hook may
/// pass `--transcript=` with nothing behind it). Pure, so it is what the tests
/// pin; [`set_firing_transcript`] only stores its answer.
fn accept_transcript(path: Option<&str>) -> Option<String> {
    path.filter(|p| !p.is_empty()).map(str::to_string)
}

/// Record the transcript this process is firing ways for. A second call is a
/// no-op.
pub fn set_firing_transcript(path: Option<&str>) {
    if let Some(p) = accept_transcript(path) {
        let _ = FIRING_TRANSCRIPT.set(p);
    }
}

pub(crate) fn firing_transcript() -> Option<&'static str> {
    FIRING_TRANSCRIPT.get().map(String::as_str)
}

/// What a firing way needs from the session's transcript: the window its
/// `refire:` fraction resolves against, and the model id stamped on the event.
#[derive(Clone, Debug, PartialEq)]
struct FiringContext {
    window: u64,
    /// The model the invoking agent is running, or `None` when its own
    /// transcript ([`session::current_transcript`]) did not yield one.
    ///
    /// Only that transcript may supply it. Subagent hooks report the parent's
    /// session id, so the session-id lookup would read the *parent's*
    /// transcript for a subagent, and the project heuristic may land on a
    /// sibling session; either would stamp a fire with a model it did not run
    /// under, and a wrong model is worse than an absent one.
    model: Option<String>,
}

/// Resolved once per process. One `ways` invocation serves one hook call, and
/// every way it fires shares the transcript, so the parse is not repeated per
/// way (an invocation delivering N ways would otherwise read it N times).
static FIRING_CONTEXT: OnceLock<FiringContext> = OnceLock::new();

fn firing_context(session_id: &str, project_dir: &str) -> &'static FiringContext {
    FIRING_CONTEXT.get_or_init(|| {
        let is_main = session::current_agent() == session::MAIN_AGENT;
        firing_context_from(session::current_transcript(session_id).as_deref(), is_main, session_id, project_dir)
    })
}

/// [`firing_context`] with the agent's own transcript given: the window and
/// model come from the file the token position is read from. Only the main
/// agent falls back to the session and project lookups. For a subagent both
/// would read another agent's transcript (the session id is the parent's), so
/// a subagent without a detected window of its own takes the resolver's
/// default, which honors `CLAUDE_CONTEXT_WINDOW`.
fn firing_context_from(
    transcript: Option<&Path>,
    is_main: bool,
    session_id: &str,
    project_dir: &str,
) -> FiringContext {
    resolve_firing_context(
        transcript.and_then(|t| crate::cmd::context::get_context_for_transcript(&t.to_string_lossy()).ok()),
        || is_main.then(|| crate::cmd::context::get_context_for_session(session_id).ok()).flatten(),
        || is_main.then(|| crate::cmd::context::get_context(Some(project_dir)).ok()).flatten(),
    )
}

/// Choose the window a firing way's `refire:` fraction resolves against, and
/// the model the fire is stamped with.
///
/// Window candidates in order: the agent's own transcript (`explicit`),
/// the session-pinned lookup, then the project heuristic. A candidate
/// contributes its window only when it detected one — a transcript with no
/// assistant turn yet returns `Ok` carrying a defaulted window, and accepting
/// that would be worse than the next candidate. Both later candidates are
/// lazy: the pinned lookup walks the projects dir, the heuristic scans every
/// project dir. The model comes from `explicit` alone (see
/// [`FiringContext::model`]).
fn resolve_firing_context(
    explicit: Option<crate::cmd::context::ContextInfo>,
    pinned: impl FnOnce() -> Option<crate::cmd::context::ContextInfo>,
    fallback: impl FnOnce() -> Option<crate::cmd::context::ContextInfo>,
) -> FiringContext {
    let model = explicit
        .as_ref()
        .map(|ctx| ctx.model.clone())
        .filter(|m| m != crate::cmd::context::UNKNOWN_MODEL);
    let detected = |ctx: crate::cmd::context::ContextInfo| {
        window_detected(&ctx).then_some(ctx.tokens_total)
    };
    let window = explicit
        .and_then(detected)
        .or_else(|| pinned().and_then(detected))
        .or_else(|| fallback().and_then(detected))
        .unwrap_or_else(|| ways_core::context_window::resolve(None).tokens);
    FiringContext { window, model }
}

/// Bound a matched surface to a single readable line for the `surface` telemetry
/// field: collapse all whitespace to single spaces and truncate to ~200 chars on
/// a char boundary (appending `…`). The snippet is a human read-side aid — a
/// judgeable record of *what* the semantic channel matched on — so it favours
/// legibility over fidelity.
const SURFACE_SNIPPET_CHARS: usize = 200;
fn surface_snippet(surface: &str) -> String {
    let collapsed = surface.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SURFACE_SNIPPET_CHARS {
        return collapsed;
    }
    let head: String = collapsed.chars().take(SURFACE_SNIPPET_CHARS).collect();
    format!("{head}…")
}

/// What the show path resolves before it decides whether a way may fire.
struct Fireable {
    project_dir: String,
    domain: String,
    scope: String,
    way_file: std::path::PathBuf,
    is_project_local: bool,
    content: String,
    firing: &'static FiringContext,
    curve: sensor_trait::Curve,
}

/// Resolves a way for firing: the disable switches, its file, its scope and
/// its refire curve. `None` when it is disabled, missing or out of scope.
fn fireable(id: &str, session_id: &str) -> Result<Option<Fireable>> {
    fireable_in_scope(id, session_id, true)
}

/// [`fireable`], with the way's `scope:` check optional. A pull (ADR-701 §5)
/// skips it: the agent named the way, so the scope that decides what injection
/// may deliver does not decide what it may read. The disable switches still apply.
fn fireable_in_scope(id: &str, session_id: &str, enforce_scope: bool) -> Result<Option<Fireable>> {
    let project_dir = crate::util::project_dir();

    // Disable checks: domain (user scope) and per-way (project scope, ADR-131)
    let domain = id.split('/').next().unwrap_or(id).to_string();
    if session::domain_disabled(&domain) || session::way_disabled(id) {
        return Ok(None);
    }

    // Scope check
    let scope = session::detect_scope(session_id);
    let Some((way_file, is_project_local)) = session::resolve_way_file(id, &project_dir) else {
        return Ok(None);
    };

    // Read frontmatter for scope field
    let content = std::fs::read_to_string(&way_file)?;
    let scope_field = crate::frontmatter::field_in(&content, "scope").unwrap_or_default();
    if enforce_scope && !session::scope_matches(&scope_field, &scope) {
        return Ok(None);
    }

    // Session firing gate: consult the engine with this way's resolved curve.
    // First-fire always allowed; re-fire when the outward gate's salience has
    // decayed below REFIRE_FLOOR; otherwise suppress.
    //
    // ADR-126: the cadence comes from `refire:` (fraction of window), resolved
    // against the session's current window — fetched from the active transcript.
    // When the transcript can't be read at all, fall back through the one resolver
    // (ADR-166) rather than a second hardcoded constant, so the operator's
    // CLAUDE_CONTEXT_WINDOW is still honored on this path.
    //
    // Pin the lookup to the *firing* session. Resolving by `project_dir` alone
    // sends `resolve_transcript` down its newest-transcript-in-project branch,
    // which reads whichever session in that project wrote last — not necessarily
    // this one. Concurrent sessions under one project therefore resolve each
    // other's windows, and a way firing in a 200k session against a 1M sibling's
    // transcript re-fires five times too slowly (or the reverse).
    //
    // A *successful* lookup is not automatically a good one: a transcript that
    // exists but has no assistant turn yet resolves to DEFAULT_WINDOW and still
    // returns `Ok`. Accepting that would hand a launching subagent a defaulted
    // window where the project heuristic would have found a real one, so both
    // candidates are filtered on `window_source` — the field ADR-166 added so a
    // default is never mistaken for a detection. `EnvOverride` counts as
    // detected, keeping CLAUDE_CONTEXT_WINDOW authoritative on this path.
    //
    // The invoking agent's own transcript (`session::current_transcript`)
    // comes first: the token position is read from it too, and the same read
    // yields the model id the event is stamped with. Resolved once per process
    // and shared by every way this invocation fires.
    let fm = frontmatter::parse(&way_file)?;
    let firing = firing_context(session_id, &project_dir);
    let curve = fm.resolved_curve(firing.window).ok_or_else(|| {
        anyhow::anyhow!(
            "way {} is missing a `refire:` field in its frontmatter (ADR-126)",
            id
        )
    })?;
    Ok(Some(Fireable { project_dir, domain, scope, way_file, is_project_local, content, firing, curve }))
}

/// A way's delivered text: its static `body` with the `macro:` output prepended
/// or appended. A project-local macro runs only for a trusted project.
fn render_way(
    body: &str,
    content: &str,
    way_file: &Path,
    is_project_local: bool,
    run: &MacroRun,
) -> String {
    let macro_pos = crate::frontmatter::field_in(content, "macro");
    let way_dir = way_file.parent().unwrap_or(Path::new("."));
    let macro_file = way_dir.join("macro.sh");
    let macro_out = if macro_pos.is_some() && macro_file.is_file() {
        if is_project_local && !is_project_trusted(run.project_dir) {
            Some(format!(
                "**Note**: Project-local macro skipped (add {} to ~/.claude/trusted-project-macros to enable)",
                run.project_dir
            ))
        } else {
            run_macro(&macro_file, run)
        }
    } else {
        None
    };

    let mut output = String::new();

    if macro_pos.as_deref() == Some("prepend") {
        if let Some(ref out) = macro_out {
            output.push_str(out);
            output.push_str("\n\n");
        }
    }

    output.push_str(body);

    if macro_pos.as_deref() == Some("append") {
        if let Some(ref out) = macro_out {
            output.push('\n');
            output.push_str(out);
        }
    }
    output
}

/// A way as a subagent receives it at SubagentStart (`ways hook
/// subagent-start`): disable-checked, resolved across the project, user and
/// core roots, and rendered as [`way_scored`] renders it, but with no scope
/// check or refire gate. The matching `ways scan task` already chose the way
/// for the subagent's scope (`subagent` or `teammate`, exported to its macro
/// as `WAYS_SCOPE`), and the subagent starts with fresh context whatever the
/// parent session has already been shown. The fire is recorded under the
/// subagent's own state (see [`record_injected`]). Empty when disabled or
/// missing.
pub fn subagent_way(id: &str, session_id: &str, scope: &str) -> Result<String> {
    let project_dir = crate::util::project_dir();
    let domain = id.split('/').next().unwrap_or(id);
    if session::domain_disabled(domain) || session::way_disabled(id) {
        return Ok(String::new());
    }
    let Some((way_file, is_project_local)) = session::resolve_way_file(id, &project_dir) else {
        return Ok(String::new());
    };
    let content = std::fs::read_to_string(&way_file)?;
    let body = static_way_body(&content);
    let run = MacroRun { session_id, project_dir: &project_dir, scope };
    let output = render_way(&body, &content, &way_file, is_project_local, &run);
    if !output.is_empty() {
        record_injected(id, session_id, &way_file, &project_dir);
    }
    Ok(output)
}

/// Record a way injected at SubagentStart as fired for the subagent: its
/// engagement, marker, token position and epoch, as [`way_scored`] records a
/// fire. Without it the subagent's first match would deliver the way again.
/// The tick is the subagent's token position, 0 before its transcript exists.
/// Skipped when the payload named no agent: the fire is not main's.
fn record_injected(id: &str, session_id: &str, way_file: &Path, project_dir: &str) {
    if session::current_agent() == session::MAIN_AGENT {
        return;
    }
    let Ok(fm) = frontmatter::parse(way_file) else { return };
    let Some(curve) = fm.resolved_curve(firing_context(session_id, project_dir).window) else { return };
    let tick = session::get_token_position(session_id);
    let lock = session::lock_engagement(id, session_id);
    session::record_way_fire(id, session_id, &curve, tick);
    drop(lock);
    stamp_disclosure(id, session_id, tick);
}

/// Stamp that a way was disclosed to the session at `tick`: its marker, token
/// position and epoch. Returns the epoch stamped. Injection and a pull
/// (ADR-701 §5) both end here, so a pulled way reads as shown to every reader
/// of these stamps.
fn stamp_disclosure(id: &str, session_id: &str, tick: u64) -> u64 {
    session::stamp_way_marker(id, session_id, tick);
    session::stamp_way_tokens(id, session_id, tick);
    let epoch = session::get_epoch(session_id);
    session::stamp_way_epoch(id, session_id, epoch);
    epoch
}

/// Whether [`way_scored`] would show this way now, budget aside: not disabled,
/// in scope, and allowed by its refire curve. Read-only. The relevance gate
/// asks it so it judges only ways that would reach the agent (ADR-196 §1).
pub(crate) fn would_fire(id: &str, session_id: &str) -> bool {
    match fireable(id, session_id) {
        Ok(Some(f)) => {
            let tick = session::get_token_position(session_id);
            session::way_fire_outcome(id, session_id, &f.curve, tick).outcome.is_allowed()
        }
        _ => false,
    }
}

/// Like [`way`], but records the embedding score that caused a semantic fire
/// onto the `way_fired` event (ADR-134 task D telemetry). `fire_score` is
/// `Some` only for embedding-channel fires from the in-process scan path;
/// keyword/state/CLI fires pass `None` and log no score (they have none). The
/// firing model is already implicit in `trigger` (`semantic:embedding:en|multi`).
///
/// `surface` is the noise-stripped surface the matcher scored (the `reduce_for_embed`
/// output common to both the late-interaction and single-vector paths). It is logged
/// — bounded by [`surface_snippet`] — only alongside a `fire_score`, i.e. on semantic
/// fires, giving the read-side precision instrument a judgeable record of what fired
/// each way without re-embedding history. Keyword fires already carry `matched_span`.
pub fn way_scored(
    id: &str,
    session_id: &str,
    trigger: &str,
    fire_score: Option<f64>,
    matched_span: Option<&str>,
    surface: Option<&str>,
    mut budget: Option<&mut ContextBudget>,
) -> Result<String> {
    let Some(Fireable { project_dir, domain, scope, way_file, is_project_local, content, firing, curve }) =
        fireable(id, session_id)?
    else {
        return Ok(String::new());
    };
    // One transcript read per fire: the same tick feeds the fast-path decision,
    // the re-check under the lock, the recorded fire, and the stamps below.
    let token_pos = session::get_token_position(session_id);
    // A refire suppression logs once per way per fire window; a context-cap
    // withhold logs every time (it is rare, and each one is a missed delivery).
    let suppress = |why: Suppression, last_fire: Option<u64>| {
        if why == Suppression::Refire
            && !last_fire.is_some_and(|t| session::first_suppression_in_window(id, session_id, t))
        {
            return;
        }
        log_way_suppressed("way", id, &domain, trigger, why, &scope, &project_dir, session_id);
    };

    // Fast path, unlocked: most suppressed ways stop here without the lock.
    let decision = session::way_fire_outcome(id, session_id, &curve, token_pos);
    if !decision.outcome.is_allowed() {
        suppress(Suppression::Refire, decision.last_fire);
        return Ok(String::new());
    }
    // A static body that cannot fit is withheld before its macro runs: the
    // macro only adds to it. Later, smaller candidates may still fit.
    let body = static_way_body(&content);
    if let Some(b) = budget.as_deref_mut() {
        if !b.fits(&body) {
            b.refuse();
            suppress(Suppression::ContextCap, decision.last_fire);
            return Ok(String::new());
        }
    }

    // The macro runs outside the engagement lock: a macro may call git or gh,
    // and holding the lock across it would serialize parallel hooks.
    let run = MacroRun { session_id, project_dir: &project_dir, scope: &scope };
    let output = render_way(&body, &content, &way_file, is_project_local, &run);

    // Critical section: re-check, admit, record. Parallel tool calls run
    // concurrent hooks, and another process may have fired this way while the
    // macro ran; the re-check under the lock sees its record, so a way is
    // delivered at most once per refire window. The fire is recorded only once
    // the body is admitted: a way the budget withholds must not start its
    // refire curve, or the curve would hold back guidance the model never saw.
    let lock = session::lock_engagement(id, session_id);
    let decision = session::way_fire_outcome(id, session_id, &curve, token_pos);
    if !decision.outcome.is_allowed() {
        drop(lock);
        suppress(Suppression::Refire, decision.last_fire);
        return Ok(String::new());
    }
    if let Some(b) = budget {
        if !b.admit(&output) {
            drop(lock);
            suppress(Suppression::ContextCap, decision.last_fire);
            return Ok(String::new());
        }
    }
    session::record_way_fire(id, session_id, &curve, token_pos);
    drop(lock);
    let is_redisclosure = decision.outcome.is_redisclosure();

    let epoch = stamp_disclosure(id, session_id, token_pos);

    // Tree disclosure tracking
    let (tree_depth, parent_id, parent_epoch, epoch_from_parent) =
        compute_tree_metrics(id, session_id);

    let (sibling_total, sibling_fired) = count_siblings(id, &project_dir, session_id);

    // Metrics JSONL
    let agent_id = session::current_agent();

    session::append_metric(
        session_id,
        &json!({
            "way": id,
            "parent": parent_id.as_deref().unwrap_or("none"),
            "depth": tree_depth,
            "epoch": epoch,
            "parent_epoch": parent_epoch,
            "epoch_distance": epoch_from_parent,
            "sibling_total": sibling_total,
            "sibling_fired": sibling_fired,
            "trigger": trigger,
            "agent_id": agent_id,
        }),
    );

    // Event logging
    let mut log_fields: Vec<(&str, String)> = vec![
        ("event", if is_redisclosure { "way_redisclosed" } else { "way_fired" }.to_string()),
        ("way", id.to_string()),
        ("domain", domain.to_string()),
        ("trigger", trigger.to_string()),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id.to_string()),
        ("token_position", token_pos.to_string()),
        // The model id the invoking agent was running at fire time, read from
        // the transcript the hook named. Written on every fire so `ways tune stats`
        // can split fires and re-disclosures by model. The literal `unknown`
        // records that no model was resolved: no `--transcript` given (dry run,
        // task lane, hook predating the flag), the path not readable, or no
        // assistant turn yet (the launch race). Rows that lack the field
        // predate it. Identification only: nothing reads this field to decide
        // whether a way fires.
        (
            "model",
            firing
                .model
                .clone()
                .unwrap_or_else(|| crate::cmd::context::UNKNOWN_MODEL.to_string()),
        ),
        // Which agent the fire was delivered to. Subagent hooks report the
        // parent's session id, so without this a parallel fan-out reads as one
        // session on the read side; `ways tune stats` keys hook invocations on it.
        ("agent_id", agent_id),
    ];
    // ADR-134 task D: the calibrated probability that fired this way, feeding the
    // fire-score telemetry that calibration (ADR-156) is fit from. Logged on every
    // semantic fire — first-fire *and* redisclosure — so precision is measurable
    // across the whole fire population, not just the first-fire slice. The score is
    // worth writing at fire time because it is not cheaply recoverable later (that
    // would mean re-embedding the historical surface against the corpus).
    // Calibration still isolates the *placement* population cleanly by filtering on
    // the event kind (`way_fired`, per tune_precision.rs), so widening the score to
    // redisclosures does not bias the derivation — it just makes the redisclosure
    // score available as a cadence signal. Keyword/state/CLI fires pass None and log
    // no score (they have none).
    if let Some(score) = fire_score {
        log_fields.push(("fire_score", format!("{score:.4}")));
        // The surface rides with the score (semantic fires only): together they make
        // a fire judgeable on the read side — "this score, on this text" — without
        // re-embedding the historical surface. A redisclosure carries its own surface,
        // so this is not gated on first-fire (unlike `matched_span`).
        if let Some(s) = surface.filter(|s| !s.is_empty()) {
            log_fields.push(("surface", surface_snippet(s)));
        }
    }
    // ADR-153 §3: the deterministic-channel match text, so introspection can show
    // *what* fired the way without transcript replay. First-fire only (a
    // redisclosure's re-trigger is a different match); semantic never carries one.
    if let (false, Some(span)) = (is_redisclosure, matched_span) {
        // A zero-width author pattern (e.g. `^`) fires but matches an empty string;
        // recording an empty span is pure telemetry noise, so skip it (the fire
        // decision was already made upstream — this only gates what's logged).
        if !span.is_empty() {
            log_fields.push(("matched_span", span.to_string()));
        }
    }
    if let Some(ref p) = parent_id {
        log_fields.push(("parent", p.clone()));
        log_fields.push(("tree_depth", tree_depth.to_string()));
        if let Some(dist) = epoch_from_parent {
            log_fields.push(("epoch_distance", dist.to_string()));
        }
    }
    let team = session::detect_team(session_id);
    if let Some(t) = team {
        log_fields.push(("team", t));
    }
    let refs: Vec<(&str, &str)> = log_fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    session::log_event(&refs);

    Ok(output)
}

// ── ways show check ─────────────────────────────────────────────

pub fn check(id: &str, session_id: &str, trigger: &str, match_score: f64) -> Result<String> {
    check_within(id, session_id, trigger, match_score, None)
}

/// [`check`] charged against a hook's [`ContextBudget`] when one is given.
pub fn check_within(
    id: &str,
    session_id: &str,
    trigger: &str,
    match_score: f64,
    mut budget: Option<&mut ContextBudget>,
) -> Result<String> {
    let project_dir = crate::util::project_dir();

    // Disable checks: domain (user scope) and per-way (project scope, ADR-131)
    let domain = id.split('/').next().unwrap_or(id);
    if session::domain_disabled(domain) || session::way_disabled(id) {
        return Ok(String::new());
    }

    // Scope check
    let scope = session::detect_scope(session_id);

    let (check_file, _is_project_local) = match session::resolve_check_file(id, &project_dir) {
        Some(r) => r,
        None => return Ok(String::new()),
    };

    let check_content = std::fs::read_to_string(&check_file)?;
    let scope_field = crate::frontmatter::field_in(&check_content, "scope").unwrap_or_default();
    if !scope_field.is_empty() && !session::scope_matches(&scope_field, &scope) {
        return Ok(String::new());
    }

    // Epoch distance
    let epoch = session::get_epoch(session_id);
    let way_has_fired = session::way_is_shown(id, session_id);
    let epoch_distance = if way_has_fired {
        session::epoch_distance(id, session_id).min(30)
    } else {
        30
    };

    // Fire count
    let fire_count = session::get_check_fires(id, session_id);

    // Scoring curve
    let distance_factor = ((epoch_distance as f64) + 1.0).ln() + 1.0;
    let decay_factor = 1.0 / (fire_count as f64 + 1.0);
    let effective_score = match_score * distance_factor * decay_factor;

    // Threshold
    let threshold: f64 = crate::frontmatter::field_in(&check_content, "threshold")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);

    if effective_score < threshold {
        return Ok(String::new());
    }

    // Include anchor section when epoch distance >= 5
    let include_anchor = epoch_distance >= 5;
    let sections = check_sections_text(&check_content, include_anchor);

    // A check and the parent way it pulls in are one budget unit: the pair
    // goes out together or not at all. A withheld check is logged and its fire
    // count is not bumped.
    let withhold = || {
        log_way_suppressed("check", id, domain, trigger, Suppression::ContextCap, &scope, &project_dir, session_id);
        Ok(String::new())
    };
    let mut output = String::new();
    let mut parent_shown = false;

    // If parent way hasn't fired, pull it in alongside the check, holding back
    // room for the check's sections (and the newline between them) meanwhile.
    let refusals_before = budget.as_deref().map_or(0, ContextBudget::refusals);
    if !way_has_fired {
        if let Some(b) = budget.as_deref_mut() {
            b.reserve(&format!("\n{sections}"));
        }
        let parent_out =
            way_scored(id, session_id, "check-pull", None, None, None, budget.as_deref_mut())?;
        if let Some(b) = budget.as_deref_mut() {
            if parent_out.is_empty() {
                b.cancel_reservation();
            } else {
                b.commit_reservation();
            }
        }
        if !parent_out.is_empty() {
            output.push_str(&parent_out);
            output.push('\n');
            parent_shown = true;
        }
    }

    // Parent shown: its reservation already paid for the sections. Otherwise
    // the check stands alone, and is withheld when the parent was withheld for
    // the cap (the pair is one unit) or the sections do not fit.
    if !parent_shown {
        if let Some(b) = budget {
            if b.refusals() > refusals_before || !b.admit(&sections) {
                return withhold();
            }
        }
    }

    output.push_str(&sections);

    // Bump fire count
    session::bump_check_fires(id, session_id);

    // Log
    let anchored = if include_anchor { "true" } else { "false" };
    let way_epoch = session::get_way_epoch(id, session_id);
    session::log_event(&[
        ("event", "check_fired"),
        ("check", id),
        ("domain", domain),
        ("trigger", trigger),
        ("epoch", &epoch.to_string()),
        ("way_epoch", &way_epoch.to_string()),
        ("distance", &epoch_distance.to_string()),
        ("fire_count", &(fire_count + 1).to_string()),
        ("match_score", &format!("{match_score:.2}")),
        ("effective_score", &format!("{effective_score:.2}")),
        ("anchored", anchored),
        ("scope", &scope),
        ("project", &project_dir),
        ("session", session_id),
        // Whose fire count this is: check state is kept per agent.
        ("agent_id", &session::current_agent()),
    ]);

    Ok(output)
}

// ── ways show core ──────────────────────────────────────────────

pub fn core(session_id: &str) -> Result<String> {
    let ways_dir = crate::paths::projected_ways_root();
    let mut output = String::new();

    // Run the macro for the dynamic ways table
    let macro_file = ways_dir.join("macro.sh");
    if macro_file.is_file() {
        let project_dir = crate::util::project_dir();
        let run = MacroRun { session_id, project_dir: &project_dir, scope: "agent" };
        if let Some(out) = run_macro(&macro_file, &run) {
            output.push_str(&out);
            output.push('\n');
        }
    }

    // Output core.md body with language substitution
    let core_file = ways_dir.join("core.md");
    if core_file.is_file() {
        let content = std::fs::read_to_string(&core_file)?;
        let mut body = body_text(&content);

        // Replace hardcoded English directive with configured language
        let lang = crate::agents::resolve_language();
        if lang != "en" {
            body = body.replace(
                "All file output (commit messages, comments, documentation, PR descriptions) must be in English regardless of interface language setting.",
                &format!("All file output (commit messages, comments, documentation, PR descriptions) must be in {lang}. Code identifiers (variable names, function names) should remain in English."),
            );
        }

        output.push_str(&body);
    }

    // Version info
    let claude_dir = crate::paths::projection_root();
    let version = git_version(&claude_dir);
    output.push_str(&format!("\n---\n_Ways version: {version}_"));

    // Update status from cache
    output.push_str(&update_status_text());

    // Dirty file enumeration
    output.push_str(&dirty_status_text(&claude_dir));

    // Stamp core marker
    session::stamp_core(session_id);

    Ok(output)
}

// ── ways show attend/<signal> ──────────────────────────────────

pub fn attend(signal: &str, session_id: &str) -> Result<String> {
    // Every root the engine reads, project first: the first eligible way wins.
    let project_dir = crate::util::project_dir();
    let dirs = crate::paths::ways_roots(Some(std::path::Path::new(&project_dir)));

    let matched_ids = attend_ids(signal, &dirs);

    if matched_ids.is_empty() {
        return Ok(format!("No way handles attend signal '{signal}'.\n"));
    }

    // Show first matching way through the standard disclosure pipeline
    let mut output = String::new();
    for id in &matched_ids {
        let trigger = format!("attend:{signal}");
        let result = way(id, session_id, &trigger)?;
        if !result.is_empty() {
            output.push_str(&result);
            break; // First eligible way wins
        }
    }

    Ok(output)
}

/// The ids of the ways that handle attend `signal`, in root order. A way
/// counts only when its own file declares the signal and is the copy that
/// resolves: a project way that drops the trigger opts out, and a shadowed
/// copy never speaks for the way.
fn attend_ids(signal: &str, dirs: &[std::path::PathBuf]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for dir in dirs {
        for path in crate::scanner::md_files(dir, crate::scanner::MdKind::Ways) {
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            if !extract_attend_signals(&content).iter().any(|s| s == signal) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(dir) else { continue };
            // "attend/context-pressure/context-pressure" → "attend/context-pressure"
            let id = normalize_way_id(&rel.with_extension("").to_string_lossy().replace('\\', "/"));
            let resolved = dirs.iter().find_map(|d| session::find_way_in_dir(&d.join(&id)));
            if resolved.as_deref() == Some(path.as_path()) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Normalize a way ID: if the last segment matches its parent dir name, collapse.
/// e.g., "attend/context-pressure/context-pressure" → "attend/context-pressure"
fn normalize_way_id(id: &str) -> String {
    let parts: Vec<&str> = id.split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 1] == parts[parts.len() - 2] {
        parts[..parts.len() - 1].join("/")
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ways_core::context_window::WindowSource;

    fn ctx(tokens_total: u64, window_source: WindowSource) -> crate::cmd::context::ContextInfo {
        crate::cmd::context::ContextInfo {
            tokens_used: 0,
            tokens_total,
            tokens_remaining: tokens_total,
            pct_used: 0,
            pct_remaining: 100,
            model: "test".to_string(),
            method: "test".to_string(),
            session: "test".to_string(),
            window_source,
            transcript: String::new(),
            usage_tail: Vec::new(),
        }
    }

    fn attend_way(root: &std::path::Path, id: &str, trigger: bool) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let fm = if trigger { "trigger:\n  type: attend\n  signals:\n    - sig\n" } else { "description: opted out\n" };
        std::fs::write(dir.join(format!("{}.md", id.rsplit('/').next().unwrap())), format!("---\n{fm}---\nbody\n")).unwrap();
    }

    #[test]
    fn attend_reads_the_resolving_copy_only() {
        let base = std::env::temp_dir().join(format!("ways-attend-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (proj, ship) = (base.join("proj"), base.join("ship"));
        // The project overrides `a/dropped` without the trigger: it opts out.
        attend_way(&proj, "a/dropped", false);
        attend_way(&ship, "a/dropped", true);
        // Both roots carry `a/both`: it is listed once.
        attend_way(&proj, "a/both", true);
        attend_way(&ship, "a/both", true);
        attend_way(&ship, "a/shipped", true);
        let ids = super::attend_ids("sig", &[proj, ship]);
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert!(ids.contains(&"a/both".to_string()) && ids.contains(&"a/shipped".to_string()), "{ids:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    fn ctx_model(
        tokens_total: u64,
        window_source: WindowSource,
        model: &str,
    ) -> crate::cmd::context::ContextInfo {
        let mut c = ctx(tokens_total, window_source);
        c.model = model.to_string();
        c
    }

    /// The pre-flag shape: no explicit transcript, the pinned lookup first.
    fn window_of(
        pinned: Option<crate::cmd::context::ContextInfo>,
        fallback: impl FnOnce() -> Option<crate::cmd::context::ContextInfo>,
    ) -> u64 {
        resolve_firing_context(None, || pinned, fallback).window
    }

    #[test]
    fn firing_window_prefers_the_pinned_session() {
        // The fix: the firing session's own window wins over whatever the
        // project heuristic's newest-transcript scan happens to land on.
        let got = window_of(Some(ctx(200_000, WindowSource::ModelTable)), || {
            Some(ctx(1_000_000, WindowSource::ModelTable))
        });
        assert_eq!(got, 200_000);
    }

    #[test]
    fn defaulted_pinned_window_yields_to_the_fallback() {
        // The regression this guards: a subagent transcript exists but has no
        // assistant turn yet, so the pinned lookup returns Ok carrying
        // DEFAULT_WINDOW. Accepting it would latch a 200k curve on a 1M
        // session; the project heuristic's real detection must win instead.
        let got = window_of(Some(ctx(200_000, WindowSource::Default)), || {
            Some(ctx(1_000_000, WindowSource::ModelTable))
        });
        assert_eq!(got, 1_000_000);
    }

    #[test]
    fn env_override_counts_as_detected() {
        // ADR-166 keeps CLAUDE_CONTEXT_WINDOW authoritative, so an override on
        // the pinned lookup must not be treated as a miss.
        let got = window_of(Some(ctx(500_000, WindowSource::EnvOverride)), || {
            Some(ctx(1_000_000, WindowSource::ModelTable))
        });
        assert_eq!(got, 500_000);
    }

    #[test]
    fn both_defaulted_falls_through_to_the_resolver() {
        // Neither candidate detected anything: fall through to the single
        // resolver rather than propagating either defaulted number.
        let got = window_of(Some(ctx(123, WindowSource::Default)), || {
            Some(ctx(456, WindowSource::Default))
        });
        assert_eq!(got, ways_core::context_window::resolve(None).tokens);
    }

    #[test]
    fn fallback_is_not_evaluated_when_pinned_is_detected() {
        // The heuristic scans every project dir and this runs on every fire,
        // so a usable pinned answer must short-circuit it.
        let mut called = false;
        let got = window_of(Some(ctx(1_000_000, WindowSource::ModelTable)), || {
            called = true;
            None
        });
        assert_eq!(got, 1_000_000);
        assert!(!called, "fallback ran despite a detected pinned window");
    }

    #[test]
    fn explicit_transcript_short_circuits_both_lookups() {
        // The hook's own transcript_path resolved a window and a model, so
        // neither the projects-dir walk nor the project heuristic runs.
        let mut pinned_called = false;
        let mut fallback_called = false;
        let got = resolve_firing_context(
            Some(ctx_model(1_000_000, WindowSource::ModelTable, "claude-fable-5-1")),
            || {
                pinned_called = true;
                None
            },
            || {
                fallback_called = true;
                None
            },
        );
        assert_eq!(got.window, 1_000_000);
        assert_eq!(got.model.as_deref(), Some("claude-fable-5-1"));
        assert!(!pinned_called && !fallback_called);
    }

    #[test]
    fn model_is_never_taken_from_a_lookup() {
        // No --transcript (dry run, task lane, old hook). The pinned lookup
        // resolves the parent's transcript for a subagent and the project
        // heuristic may land on a sibling session, so both supply a window
        // only; the model stays unknown rather than someone else's.
        let got = resolve_firing_context(
            None,
            || Some(ctx_model(200_000, WindowSource::ModelTable, "claude-opus-5")),
            || Some(ctx_model(1_000_000, WindowSource::ModelTable, "claude-fable-5-1")),
        );
        assert_eq!(got.window, 200_000);
        assert_eq!(got.model, None);
    }

    #[test]
    fn unknown_explicit_model_stays_unknown() {
        // Launch race: the named (subagent) transcript has no assistant turn
        // yet. The pinned lookup is still consulted for the window, but it is
        // the parent's transcript, so its model must not be stamped on this
        // agent's fire.
        let got = resolve_firing_context(
            Some(ctx_model(200_000, WindowSource::Default, "unknown")),
            || Some(ctx_model(1_000_000, WindowSource::ModelTable, "claude-opus-5")),
            || None,
        );
        assert_eq!(got.window, 1_000_000);
        assert_eq!(got.model, None);
    }

    #[test]
    fn accept_transcript_rejects_empty_and_keeps_paths() {
        // A hook may pass `--transcript=` with nothing behind it; an empty
        // value must not claim the slot.
        assert_eq!(accept_transcript(Some("")), None);
        assert_eq!(accept_transcript(None), None);
        assert_eq!(accept_transcript(Some("/t/s.jsonl")), Some("/t/s.jsonl".to_string()));
    }

    #[test]
    fn surface_snippet_collapses_whitespace() {
        assert_eq!(
            surface_snippet("write   an\nADR\tand   open a PR"),
            "write an ADR and open a PR"
        );
    }

    #[test]
    fn surface_snippet_bounds_long_input_on_char_boundary() {
        let long = "é".repeat(500); // multi-byte; truncation must not split a char
        let out = surface_snippet(&long);
        assert_eq!(out.chars().count(), SURFACE_SNIPPET_CHARS + 1, "200 chars + ellipsis");
        assert!(out.ends_with('…'));
    }

    #[test]
    fn surface_snippet_leaves_short_input_untruncated() {
        let s = "short surface";
        assert_eq!(surface_snippet(s), s);
    }

    #[test]
    fn normalize_collapses_duplicate_leaf() {
        assert_eq!(
            normalize_way_id("meta/attend/context-pressure/context-pressure"),
            "meta/attend/context-pressure"
        );
    }

    #[test]
    fn normalize_preserves_distinct_leaf() {
        assert_eq!(
            normalize_way_id("softwaredev/code/testing"),
            "softwaredev/code/testing"
        );
    }

    #[test]
    fn normalize_single_segment() {
        assert_eq!(normalize_way_id("testing"), "testing");
    }

    // ── ContextBudget (#528) ────────────────────────────────────

    #[test]
    fn budget_skips_a_body_that_does_not_fit_and_admits_later_ones() {
        let mut b = ContextBudget::new(10);
        assert!(b.admit("abcd"));
        b.charge("\n\n");
        assert!(b.admit("ab"), "6 + 2 = 8 fits under 10");
        assert!(!b.admit("abc"), "8 + 3 = 11 is over the cap");
        assert_eq!(b.refusals(), 1);
        assert!(b.admit("xy"), "a refusal does not stop admission: 8 + 2 = 10 fits");
        assert!(!b.admit("z"));
        assert!(b.admit(""), "an empty body always fits");
    }

    #[test]
    fn budget_admits_an_oversized_first_body() {
        let mut b = ContextBudget::new(10);
        assert!(b.admit(&"x".repeat(25)), "the first body is never dropped");
        assert!(!b.admit("y"));
        assert_eq!(b.refusals(), 1);
    }

    #[test]
    fn budget_counts_utf16_units_like_claude_code() {
        // One astral-plane char is two UTF-16 units, as a JS string counts it.
        assert_eq!(context_chars("😀"), 2);
        assert_eq!(context_chars("é"), 1);
        let mut b = ContextBudget::new(3);
        assert!(b.admit("😀"));
        assert!(!b.admit("😀"), "2 + 2 = 4 is over a cap of 3");
    }

    #[test]
    fn check_and_parent_are_one_unit() {
        // Room for the parent alone but not the parent plus the check's
        // reserved sections: the parent is refused, and the refusal count tells
        // the check to withhold itself too.
        let mut b = ContextBudget::new(20);
        assert!(b.admit("0123456789"));
        let before = b.refusals();
        b.reserve("abcde");
        assert!(!b.admit("012345"), "10 + 6 + 5 reserved = 21 is over");
        b.cancel_reservation();
        assert!(b.refusals() > before, "the pair's refusal is visible to the check");
        assert!(b.admit("0123"), "later, smaller units are still admitted");

        // The pair fits: the parent is admitted and the reservation is spent.
        let mut b = ContextBudget::new(20);
        assert!(b.admit("0123456789"));
        b.reserve("abcd");
        assert!(b.admit("0123"));
        b.commit_reservation();
        assert!(!b.admit("xyz"), "18 + 3 is over");
    }

    #[test]
    fn hook_budget_is_the_documented_cap() {
        assert_eq!(HOOK_CONTEXT_CAP, 10_000);
        let mut b = ContextBudget::hook();
        assert!(b.admit(&"x".repeat(6_000)));
        assert!(b.admit(&"x".repeat(4_000)), "exactly at the cap fits");
        assert!(!b.admit("x"));
    }

    /// The hook names the parent's transcript, but a subagent's window and
    /// model come from its own file, the one its token position is read from.
    #[test]
    fn a_subagent_fires_against_its_own_window_and_model() {
        let root = std::env::temp_dir().join(format!("ways-firing-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let turn = |model: &str| {
            format!(r#"{{"type":"assistant","message":{{"model":"{model}","usage":{{"input_tokens":5000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#)
                + "\n"
        };
        let dir = root.join(".claude/projects/-srv-p");
        std::fs::create_dir_all(dir.join("sess/subagents")).unwrap();
        let parent = dir.join("sess.jsonl");
        std::fs::write(&parent, turn("claude-opus-4-8")).unwrap();
        std::fs::write(dir.join("sess/subagents/agent-asub.jsonl"), turn("claude-haiku-4-5")).unwrap();

        let own = session::transcript_in(&claude, parent.to_str(), "/srv/p", "sess", "asub");
        assert_eq!(own.as_deref(), Some(dir.join("sess/subagents/agent-asub.jsonl").as_path()));
        let ctx = firing_context_from(own.as_deref(), false, "sess", "/srv/p");
        assert_eq!((ctx.model.as_deref(), ctx.window), (Some("claude-haiku-4-5"), 200_000));
        // The main agent, given the same hook transcript, reads the parent's.
        let main = session::transcript_in(&claude, parent.to_str(), "/srv/p", "sess", session::MAIN_AGENT);
        let ctx = firing_context_from(main.as_deref(), true, "sess", "/srv/p");
        assert_eq!((ctx.model.as_deref(), ctx.window), (Some("claude-opus-4-8"), 1_000_000));
        // A subagent with no transcript yet takes no session lookup: no model,
        // and the resolver's default window.
        let none = firing_context_from(None, false, "sess", "/srv/p");
        assert_eq!((none.model, none.window), (None, ways_core::context_window::resolve(None).tokens));
        std::fs::remove_dir_all(&root).ok();
    }
}
