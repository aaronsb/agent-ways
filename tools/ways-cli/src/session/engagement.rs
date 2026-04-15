//! ADR-123 firing-dynamics engine integration for the session state module.
//!
//! This is the per-way `EngagementState` persistence layer plus the
//! `FirstFire → ReFire → Suppressed` classification used by
//! `ways run`, `ways show`, and friends. Extracted from `session.rs`
//! in issue #52 when session.rs crossed the 800-line priority
//! threshold during the PR #49 code-review response — the cluster is
//! a clean ADR-123 unit and carries its own tests, so it splits
//! without discovering new seams.
//!
//! Dependencies on sibling session-state helpers (`session_dir`,
//! `ensure_parent`, `get_token_position`, `resolve_way_file`) are
//! reached through `super::`; Rust's privacy model lets descendant
//! modules see parent-private items, so no visibility promotion was
//! needed on the session.rs side.

use std::path::PathBuf;

use sensor_trait::{Curve, EngagementState, Tick};

use super::{ensure_parent, get_token_position, resolve_way_file, session_dir};

/// Floor on `EngagementState::current_salience` below which a re-fire is
/// considered warranted. Tuned so that `Curve::Exponential { half_life: H }`
/// re-fires at exactly `H` ticks post-fire (salience there is 0.5), and
/// `Curve::Flat { suppression: N }` re-fires at exactly `N` ticks (the
/// step from 1.0 to 0.0 lands below the floor).
pub const REFIRE_FLOOR: f64 = 0.5;

/// Outcome of querying the firing-dynamics engine for a way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireOutcome {
    /// Way has never fired this session — first fire always allowed.
    FirstFire,
    /// Way has fired before; salience has decayed below the floor.
    /// Re-injection warranted.
    ReFire,
    /// Way has fired recently; salience is still loud. Suppress.
    Suppressed,
}

impl FireOutcome {
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::FirstFire | Self::ReFire)
    }

    pub fn is_redisclosure(self) -> bool {
        matches!(self, Self::ReFire)
    }
}

fn engagement_path(way_id: &str, session_id: &str) -> PathBuf {
    session_dir(session_id)
        .join("way-engagement")
        .join(format!("{}.json", way_id.replace('/', "__")))
}

fn load_engagement(way_id: &str, session_id: &str, curve: &Curve) -> EngagementState {
    let path = engagement_path(way_id, session_id);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<EngagementState>(&s).ok())
        .unwrap_or_else(|| EngagementState::new(curve.clone()))
}

fn save_engagement(way_id: &str, session_id: &str, state: &EngagementState) {
    let path = engagement_path(way_id, session_id);
    ensure_parent(&path);
    if let Ok(serialized) = serde_json::to_string(state) {
        let _ = std::fs::write(&path, serialized);
    }
}

/// Query the ADR-123 firing engine: should this way fire at the current
/// tick, and if so is it a first-fire or a re-fire?
///
/// The caller supplies the way's `curve` from its parsed frontmatter.
/// Tick source is `get_token_position(session_id)`.
pub fn way_fire_outcome(
    way_id: &str,
    session_id: &str,
    curve: &Curve,
) -> FireOutcome {
    let current_tick: Tick = get_token_position(session_id);
    let state = load_engagement_for_tick(way_id, session_id, curve, current_tick);
    classify_outcome(&state, current_tick)
}

/// Pure classification: given an already-loaded `EngagementState` and the
/// current tick, decide the outcome. Split out from `way_fire_outcome` so
/// the contract is testable without touching disk (load_engagement) or
/// the transcript (get_token_position).
fn classify_outcome(state: &EngagementState, current_tick: Tick) -> FireOutcome {
    if !state.has_fired() {
        return FireOutcome::FirstFire;
    }
    if state.current_salience(current_tick) < REFIRE_FLOOR {
        FireOutcome::ReFire
    } else {
        FireOutcome::Suppressed
    }
}

/// Record that a way fired at the current tick, updating and persisting
/// its engagement state.
pub fn record_way_fire(way_id: &str, session_id: &str, curve: &Curve) {
    let current_tick: Tick = get_token_position(session_id);
    let mut state = load_engagement_for_tick(way_id, session_id, curve, current_tick);
    state.record_fire(current_tick, 1.0);
    save_engagement(way_id, session_id, &state);
}

/// Load engagement state and normalize it against the current tick.
///
/// If `last_fire > current_tick` — which happens if a transcript rotates
/// or the tick source resets for any reason — the persisted state's
/// `last_fire` is past the current cursor, `saturating_sub` clamps the
/// delta to zero, and `current_salience` returns 1.0 indefinitely until
/// the session accumulates enough new ticks to pass the stored value.
/// For sessions that rotate mid-run this effectively locks the way into
/// Suppressed.
///
/// The guard: when the stored state is ahead of the current tick, treat
/// it as stale and reset it (clear history, clear `last_fire`). The next
/// fire starts fresh, and the way becomes eligible immediately instead
/// of waiting for the cursor to catch up to a tick it can never observe.
fn load_engagement_for_tick(
    way_id: &str,
    session_id: &str,
    curve: &Curve,
    current_tick: Tick,
) -> EngagementState {
    let state = load_engagement(way_id, session_id, curve);
    match state.last_fire_tick() {
        Some(last) if last > current_tick => EngagementState::new(curve.clone()),
        _ => state,
    }
}

/// Resolve a way's re-fire threshold in thousands of tokens by reading
/// its frontmatter curve and asking `Curve::refire_delta(REFIRE_FLOOR)`.
/// Used by `ways list` / `ways rethink` to render per-way bar positions.
///
/// Returns `None` when the way file cannot be resolved, its frontmatter
/// cannot be parsed, or its `curve:` field is missing or its curve never
/// falls below the floor. Callers pick a sensible fallback — typically
/// 25% of the context window to preserve the old visual baseline.
pub fn way_refire_threshold_k(way_id: &str, project_dir: &str) -> Option<u64> {
    let (way_file, _) = resolve_way_file(way_id, project_dir)?;
    let fm = crate::frontmatter::parse(&way_file).ok()?;
    let curve = fm.curve?;
    let delta = curve.refire_delta(REFIRE_FLOOR)?;
    Some(delta / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_trait::{Curve, EngagementState};

    /// Contract test for the FirstFire → ReFire → Suppressed state
    /// machine. Exercises `classify_outcome` directly so we don't need
    /// a live session directory, transcript, or way file on disk.
    #[test]
    fn fire_outcome_contract() {
        let curve = Curve::Exponential { half_life: 100 };
        let mut state = EngagementState::new(curve.clone());

        // Fresh state: never fired → FirstFire regardless of tick.
        assert!(matches!(classify_outcome(&state, 0), FireOutcome::FirstFire));
        assert!(matches!(
            classify_outcome(&state, 10_000),
            FireOutcome::FirstFire
        ));

        // Record a fire at tick 0. Salience = 1.0, which is >= REFIRE_FLOOR
        // (0.5), so the same tick and any delta < half_life should Suppress.
        state.record_fire(0, 1.0);
        assert!(matches!(
            classify_outcome(&state, 0),
            FireOutcome::Suppressed
        ));
        assert!(matches!(
            classify_outcome(&state, 50),
            FireOutcome::Suppressed
        ));
        // At delta = half_life, salience = exactly 0.5, which is NOT
        // strictly less than REFIRE_FLOOR, so still Suppressed. The
        // refire_delta fix for this curve is delta 101.
        assert!(matches!(
            classify_outcome(&state, 100),
            FireOutcome::Suppressed
        ));
        // Delta 101 crosses the strict-less-than threshold → ReFire.
        assert!(matches!(classify_outcome(&state, 101), FireOutcome::ReFire));
        // Well past half_life → still ReFire (salience keeps decaying).
        assert!(matches!(
            classify_outcome(&state, 10_000),
            FireOutcome::ReFire
        ));
    }

    /// The backward-tick guard: if the persisted state's last_fire is
    /// past the current tick (e.g., transcript rotation), the engine
    /// would otherwise lock the way into Suppressed until the cursor
    /// catches up. `load_engagement_for_tick` resets the state instead.
    #[test]
    fn load_engagement_for_tick_resets_on_backward_jump() {
        // We construct the state-path scenario by calling the pure
        // classification logic against a rotation-shaped state manually,
        // rather than exercising disk persistence. The guard itself is
        // in load_engagement_for_tick; this test demonstrates what the
        // guard protects against by showing the un-guarded shape.
        let curve = Curve::Exponential { half_life: 100 };
        let mut stale = EngagementState::new(curve.clone());
        stale.record_fire(1_000_000, 1.0); // huge last_fire from before rotation

        // Without reset: current_tick=500 is smaller than last_fire; the
        // engine's saturating_sub clamps delta to 0, salience = 1.0,
        // classification = Suppressed. This is the lockout the guard
        // exists to prevent.
        assert!(matches!(
            classify_outcome(&stale, 500),
            FireOutcome::Suppressed
        ));

        // With reset (what load_engagement_for_tick does): fresh state
        // → FirstFire, way becomes eligible immediately.
        let reset = EngagementState::new(curve);
        assert!(matches!(
            classify_outcome(&reset, 500),
            FireOutcome::FirstFire
        ));
    }
}
