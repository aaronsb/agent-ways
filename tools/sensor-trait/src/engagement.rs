//! Unit-agnostic engagement state (ADR-123).
//!
//! `EngagementState` owns a [`Curve`] and a tick-based fire history, and
//! exposes the inward and outward gates from ADR-123 Decision 3:
//!
//! - [`Self::should_fire`] — "should this new event be allowed to fire?"
//! - [`Self::current_salience`] — "is the last-fired guidance still loud?"
//!
//! The tick axis is caller-supplied and opaque to the engine — attend
//! passes wall-clock seconds (via [`crate::epoch_secs`]), ways will pass
//! token position, future callers supply whatever monotonic matches
//! their cadence.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{Curve, Tick, TickDelta};

/// Convert a linear-per-minute decay rate (the attend-pre-ADR-123 shape)
/// into an equivalent exponential half-life in seconds.
///
/// Formula: `half_life_seconds = (ln(0.5) / ln(1 - rate_per_minute)) × 60`.
/// Plugs `decay_per_minute = 0.1 → 395 s`, etc. See ADR-123 Phase B
/// worksheet and the caveat there about linear-vs-exponential shape.
///
/// Returns `TickDelta::MAX` for degenerate inputs (rate ≤ 0 or ≥ 1),
/// which in practice means "never decay" / "always elevated" — attend
/// should treat these as misconfiguration.
pub fn rate_per_min_to_half_life_secs(rate: f64) -> TickDelta {
    if rate <= 0.0 || rate >= 1.0 {
        return TickDelta::MAX;
    }
    let half_life_min = 0.5_f64.ln() / (1.0 - rate).ln();
    let half_life_sec = (half_life_min * 60.0).round();
    if half_life_sec <= 0.0 {
        1
    } else {
        half_life_sec as TickDelta
    }
}

/// Per-subject firing dynamics state, keyed on a caller-supplied monotonic
/// progression axis. The engine does not know what a tick is — see
/// [`crate::Tick`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngagementState {
    curve: Curve,
    history: VecDeque<(Tick, f64)>,
    last_fire: Option<Tick>,
}

impl EngagementState {
    /// Create a new engagement state with the given curve and no fire history.
    pub fn new(curve: Curve) -> Self {
        Self {
            curve,
            history: VecDeque::new(),
            last_fire: None,
        }
    }

    /// Inward gate. Returns true iff the curve's refractory multiplier
    /// admits a new fire of the given magnitude at `current_tick`.
    ///
    /// Convention: magnitude is pre-normalized so that 1.0 is "normal
    /// firing strength at rest." Elevated refractory multipliers require
    /// proportionally larger magnitudes to break through. An
    /// `f64::INFINITY` multiplier (absolute refractory) blocks all fires.
    pub fn should_fire(&self, current_tick: Tick, magnitude: f64) -> bool {
        let multiplier = self.current_multiplier(current_tick);
        if !multiplier.is_finite() {
            return false;
        }
        magnitude >= multiplier
    }

    /// Record a fire at `tick` with the given magnitude. The history is
    /// kept in order; callers are expected to pass monotonic ticks.
    pub fn record_fire(&mut self, tick: Tick, magnitude: f64) {
        self.history.push_back((tick, magnitude));
        self.last_fire = Some(tick);
        self.prune_decayed(tick);
    }

    /// Outward gate. Salience of the last-fired guidance at `current_tick`,
    /// or 0.0 if nothing has fired yet.
    pub fn current_salience(&self, current_tick: Tick) -> f64 {
        let Some(last) = self.last_fire else { return 0.0 };
        let delta = current_tick.saturating_sub(last);
        self.curve.salience_at(delta)
    }

    /// Current refractory multiplier. 1.0 at rest, >1.0 during relative
    /// refractory, `f64::INFINITY` during absolute refractory.
    pub fn current_multiplier(&self, current_tick: Tick) -> f64 {
        let delta = match self.last_fire {
            Some(last) => current_tick.saturating_sub(last),
            None => 0,
        };
        let history_slice: Vec<(Tick, f64)> = self.history.iter().copied().collect();
        self.curve.multiplier_at(delta, &history_slice, current_tick)
    }

    /// Whether the inward gate is fully closed ("absolute refractory" —
    /// no fires allowed at any magnitude).
    pub fn in_absolute_refractory(&self, current_tick: Tick) -> bool {
        !self.current_multiplier(current_tick).is_finite()
    }

    /// Effective firing threshold given a caller-supplied `base` threshold.
    /// Returns `None` during absolute refractory (hard block), otherwise
    /// `Some(base × refractory_multiplier)`. This is the shape attend's old
    /// `EngagementState::effective_threshold` had, preserved as a helper so
    /// migration is mechanical.
    pub fn effective_threshold(&self, base: f64, current_tick: Tick) -> Option<f64> {
        let multiplier = self.current_multiplier(current_tick);
        if !multiplier.is_finite() {
            return None;
        }
        Some(base * multiplier)
    }

    /// Access the underlying curve (for diagnostics and tune-time access).
    pub fn curve(&self) -> &Curve {
        &self.curve
    }

    /// Replace the curve, preserving fire history.
    ///
    /// The curve is part of the serialized state, so a state reloaded from disk
    /// carries whatever curve was current at first fire. When the curve is
    /// derived from something that can change within a session — a `refire:`
    /// fraction resolved against the context window, which is unknown until the
    /// first assistant turn names a model — the persisted copy is stale and the
    /// caller's freshly resolved one is authoritative. History stays: only the
    /// decay shape is being corrected, not the record of what fired when.
    pub fn set_curve(&mut self, curve: Curve) {
        self.curve = curve;
    }

    /// Whether any fire has been recorded. Useful for first-fire vs
    /// re-fire discrimination in the caller.
    pub fn has_fired(&self) -> bool {
        self.last_fire.is_some()
    }

    /// Tick of the most recent fire, if any.
    pub fn last_fire_tick(&self) -> Option<Tick> {
        self.last_fire
    }

    /// Prune history entries whose multiplier contribution has decayed
    /// past the burst-consideration epsilon. Keeps the history bounded
    /// over long sessions without losing burst-detection fidelity.
    fn prune_decayed(&mut self, current: Tick) {
        let half_life = match &self.curve {
            Curve::ActionPotential {
                multiplier_half_life,
                ..
            } => *multiplier_half_life,
            // Non-refractory curves don't need burst history, so prune
            // everything older than the most recent fire.
            _ => 0,
        };
        if half_life == 0 {
            // Keep only the most recent fire — it's what last_fire
            // already tracks anyway.
            while self.history.len() > 1 {
                self.history.pop_front();
            }
            return;
        }
        const EPSILON: f64 = 0.01;
        while let Some((tick, _)) = self.history.front().copied() {
            let delta = current.saturating_sub(tick);
            let contribution = 0.5_f64.powf(delta as f64 / half_life as f64);
            if contribution <= EPSILON {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_curve_replaces_decay_shape_and_keeps_history() {
        // The curve is serialized with the state, so a state reloaded from disk
        // carries whatever window was resolvable at first fire. Callers re-apply
        // the current curve on load; that must not discard what already fired.
        let mut state = EngagementState::new(Curve::Exponential { half_life: 30_000 });
        state.record_fire(500, 1.0);

        state.set_curve(Curve::Exponential { half_life: 150_000 });

        assert!(matches!(
            state.curve(),
            Curve::Exponential {
                half_life: 150_000
            }
        ));
        assert!(state.has_fired(), "history dropped when the curve was swapped");
        assert_eq!(state.last_fire_tick(), Some(500));
    }

    #[test]
    fn a_widened_curve_suppresses_where_the_stale_one_would_refire() {
        // Why the swap matters: at tick 40_000 a latched 30k half-life has
        // decayed past REFIRE_FLOOR (0.5) and would re-disclose, while the
        // correctly-resolved 150k curve has not.
        let mut stale = EngagementState::new(Curve::Exponential { half_life: 30_000 });
        stale.record_fire(0, 1.0);
        assert!(stale.current_salience(40_000) < 0.5);

        let mut fresh = EngagementState::new(Curve::Exponential { half_life: 30_000 });
        fresh.record_fire(0, 1.0);
        fresh.set_curve(Curve::Exponential { half_life: 150_000 });
        assert!(fresh.current_salience(40_000) >= 0.5);
    }

    fn ap_curve() -> Curve {
        Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        }
    }

    #[test]
    fn new_state_has_no_history_and_allows_firing() {
        let s = EngagementState::new(ap_curve());
        assert_eq!(s.current_salience(0), 0.0);
        assert_eq!(s.current_multiplier(0), 1.0);
        assert!(s.should_fire(0, 1.0));
    }

    #[test]
    fn full_burst_decay_cycle() {
        let mut s = EngagementState::new(ap_curve());

        // Fire 1: at rest, should be allowed.
        assert!(s.should_fire(0, 1.0));
        s.record_fire(0, 1.0);

        // Fire 2: still at rest (below burst threshold).
        assert!(s.should_fire(100, 1.0));
        s.record_fire(100, 1.0);

        // Fire 3: this brings us to threshold — still allowed because
        // multiplier is computed before the fire is recorded.
        assert!(s.should_fire(200, 1.0));
        s.record_fire(200, 1.0);

        // Immediately after: absolute refractory blocks everything.
        assert!(!s.should_fire(220, 1.0));
        assert!(!s.should_fire(220, 1_000.0));
        assert!(s.current_multiplier(220).is_infinite());

        // Past absolute refractory but still in relative refractory:
        // multiplier is elevated above 1.0.
        let m = s.current_multiplier(300);
        assert!(m.is_finite());
        assert!(m > 1.0, "expected elevated multiplier, got {}", m);
        // Weak stimulus blocked, strong stimulus passes.
        assert!(!s.should_fire(300, 1.0));
        assert!(s.should_fire(300, 10.0));

        // Far past burst — back to rest.
        let m_rest = s.current_multiplier(200_000);
        assert!(
            (m_rest - 1.0).abs() < 1e-9,
            "expected rest multiplier, got {}",
            m_rest
        );
        assert!(s.should_fire(200_000, 1.0));
    }

    #[test]
    fn salience_decays_over_ticks_for_exponential() {
        let curve = Curve::Exponential { half_life: 100 };
        let mut s = EngagementState::new(curve);
        s.record_fire(0, 1.0);
        assert!((s.current_salience(0) - 1.0).abs() < 1e-9);
        assert!((s.current_salience(100) - 0.5).abs() < 1e-9);
        assert!((s.current_salience(200) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn flat_curve_salience_matches_step() {
        let curve = Curve::Flat { suppression: 500 };
        let mut s = EngagementState::new(curve);
        s.record_fire(0, 1.0);
        // Loud during the suppression window, faded after.
        assert_eq!(s.current_salience(100), 1.0);
        assert_eq!(s.current_salience(499), 1.0);
        assert_eq!(s.current_salience(500), 0.0);
        assert_eq!(s.current_salience(10_000), 0.0);
    }

    #[test]
    fn prune_bounds_history_on_non_refractory_curve() {
        let mut s = EngagementState::new(Curve::Exponential { half_life: 100 });
        for t in 0..10 {
            s.record_fire(t * 10, 1.0);
        }
        // Non-refractory curves don't need burst history — only one
        // entry should remain (the most recent).
        assert!(s.history.len() <= 1);
        assert_eq!(s.last_fire, Some(90));
    }

    #[test]
    fn rate_per_min_conversion_sanity() {
        // Documented in ADR-123 Phase B worksheet: 0.10 → ~395s.
        assert_eq!(rate_per_min_to_half_life_secs(0.10), 395);
        // Plug-back: 0.5^(60 / half_life) ≈ (1 - 0.1) per minute.
        let hl = rate_per_min_to_half_life_secs(0.10) as f64;
        let per_min = 0.5_f64.powf(60.0 / hl);
        assert!((per_min - 0.9).abs() < 0.001);

        // Degenerate inputs collapse to "never decay".
        assert_eq!(rate_per_min_to_half_life_secs(0.0), TickDelta::MAX);
        assert_eq!(rate_per_min_to_half_life_secs(1.0), TickDelta::MAX);
        assert_eq!(rate_per_min_to_half_life_secs(-0.1), TickDelta::MAX);
    }

    #[test]
    fn prune_bounds_history_on_action_potential() {
        let mut s = EngagementState::new(ap_curve());
        // Fires that are far enough apart that earlier ones decay out
        // before later ones are recorded.
        s.record_fire(0, 1.0);
        s.record_fire(10_000, 1.0); // earlier fire should be pruned (far past multiplier_half_life)
        assert!(s.history.len() <= 1);
    }
}
