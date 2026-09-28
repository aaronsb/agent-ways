//! Firing-dynamics curve shapes (ADR-123 Decision 2).
//!
//! `Curve` is a first-class parameter of the firing engine. It describes
//! how salience decays and how refractory multipliers behave over a
//! caller-supplied progression axis (`Tick` / `TickDelta` — see `lib.rs`).
//!
//! All decay parameters are expressed as `half_life` in caller ticks. The
//! engine does not know whether a tick is a wall-clock second, a token
//! position, a turn count, or anything else — that is the caller's call.

use serde::{Deserialize, Serialize};

use crate::{Tick, TickDelta};

/// Shape of the firing dynamics for a single engaged subject (a sensor
/// slot, a way, or any other firing source). Sealed enum rather than a
/// trait object — cheap to match on, serializable straight into frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Curve {
    /// Smooth exponential decay. `salience_at(delta) = 0.5^(delta / half_life)`.
    /// Primary shape for outward-gate salience fading.
    Exponential { half_life: TickDelta },

    /// Action-potential model: event-count burst detection raises a
    /// refractory multiplier that then decays back toward 1.0 over tick
    /// distance. Ported from ADR-119 with event-count windowing // adr-cite-ignore
    /// (see ADR-123 Decision 2) so it is robust to chunky progression axes.
    ///
    /// The "burst window" is NOT a separate tick span. It is implicit in
    /// which history entries still contribute non-trivial multiplier
    /// mass (via `multiplier_half_life`). A single event advancing the
    /// tick by thousands cannot swallow the window — it just ages the
    /// older entries out by their own decay.
    ActionPotential {
        /// Number of fires in recent history needed to trigger a burst.
        burst_threshold: usize,
        /// Multiplier applied at burst peak (e.g., 1.5 or 2.0). Higher
        /// values make the refractory gate stricter.
        peak_multiplier: f64,
        /// Hard-suppression window after a burst. During this window
        /// the multiplier is effectively infinite — nothing fires.
        absolute_refractory: TickDelta,
        /// Half-life of the refractory multiplier's decay back toward
        /// 1.0. Also implicitly defines how long a fire contributes to
        /// burst detection.
        multiplier_half_life: TickDelta,
    },

    /// Explicit re-fire schedule at tick deltas with diminishing salience.
    /// Progressive disclosure as a first-class pattern: salience at a
    /// given delta is the magnitude of the most recent step whose delta
    /// is less than or equal to the current delta.
    ProgressiveStaircase { steps: Vec<(TickDelta, f64)> },

    /// Discontinuous step function: suppressed for `suppression` ticks,
    /// then fully recovered. Valid first-class choice for ways that want
    /// all-or-nothing gating without smooth decay.
    Flat { suppression: TickDelta },
}

impl Curve {
    /// Outward-gate salience at a given delta from the most recent fire.
    /// "How loud is the last-fired guidance right now?" 1.0 = just-fired,
    /// 0.0 = fully faded.
    pub fn salience_at(&self, delta: TickDelta) -> f64 {
        match self {
            Curve::Exponential { half_life } => {
                if *half_life == 0 {
                    return 0.0;
                }
                0.5_f64.powf(delta as f64 / *half_life as f64)
            }
            // ActionPotential governs firing via its multiplier, not via
            // a decaying outward salience. The outward gate for an action-
            // potential curve is "fully present until suppressed" — callers
            // that want smooth salience decay should layer Exponential on
            // top or pick a different curve.
            Curve::ActionPotential { .. } => 1.0,
            Curve::ProgressiveStaircase { steps } => {
                let mut current = 0.0;
                for (step_delta, step_salience) in steps {
                    if *step_delta <= delta {
                        current = *step_salience;
                    } else {
                        break;
                    }
                }
                current
            }
            // Flat salience is the mirror of its suppression semantic:
            // "loud" (1.0) for the first `suppression` ticks post-fire,
            // then "faded" (0.0) thereafter. This keeps the uniform
            // caller rule "re-inject when salience < floor" working the
            // same way for Flat as for Exponential — the Flat variant is
            // just the discontinuous analog of the same fade shape.
            Curve::Flat { suppression } => {
                if delta < *suppression {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// Predict the tick delta at which salience first falls below
    /// `floor` — the natural "re-fire" distance for the outward gate.
    ///
    /// For [`Curve::Exponential`], inverts `0.5^(d/H) = floor` to
    /// `d = H · log2(1/floor)`. For [`Curve::Flat`], this is exactly
    /// `suppression`. For [`Curve::ProgressiveStaircase`], this is the
    /// first step whose salience is strictly less than `floor`. For
    /// [`Curve::ActionPotential`], salience is a flat 1.0 so the
    /// outward gate never opens on salience alone — this method returns
    /// `absolute_refractory + multiplier_half_life` as a useful
    /// visualization proxy (the point at which the inward multiplier
    /// has decayed roughly half-way back to rest).
    ///
    /// Returns `None` when no delta ever falls below the floor (e.g.,
    /// a staircase whose final step still exceeds the floor).
    pub fn refire_delta(&self, floor: f64) -> Option<TickDelta> {
        if floor <= 0.0 {
            return None;
        }
        match self {
            Curve::Exponential { half_life } => {
                // Semantic: smallest delta at which salience is strictly
                // less than floor. salience(d) = 0.5^(d/H) < floor iff
                // d > H · log2(1/floor). The smallest integer d > X is
                // floor(X) + 1 — which disagrees with ceil(X) only at
                // exact-integer X (e.g., floor=0.5 → X = H exactly), and
                // that disagreement is exactly where the old math was
                // off-by-one relative to the caller's strict `<` check.
                if floor >= 1.0 {
                    // salience starts at 1.0 and only decreases; it is
                    // never strictly less than a floor of 1.0 or more.
                    return None;
                }
                if *half_life == 0 {
                    return Some(1);
                }
                let ratio = (1.0 / floor).log2();
                Some((*half_life as f64 * ratio).floor() as TickDelta + 1)
            }
            Curve::Flat { suppression } => Some(*suppression),
            Curve::ProgressiveStaircase { steps } => {
                for (step_delta, step_salience) in steps {
                    if *step_salience < floor {
                        return Some(*step_delta);
                    }
                }
                None
            }
            Curve::ActionPotential {
                absolute_refractory,
                multiplier_half_life,
                ..
            } => Some(*absolute_refractory + *multiplier_half_life),
        }
    }

    /// Inward-gate refractory multiplier at the current tick given the
    /// fire history. "How much harder is it to fire right now than at
    /// rest?" 1.0 = normal, >1.0 = elevated, `f64::INFINITY` = absolute
    /// refractory.
    ///
    /// `delta` is the delta from the most recent fire to `current` — it
    /// is accepted as a parameter so callers that already computed it
    /// don't have to re-derive it, but the history is the source of
    /// truth for event-count burst detection.
    pub fn multiplier_at(
        &self,
        delta: TickDelta,
        history: &[(Tick, f64)],
        current: Tick,
    ) -> f64 {
        match self {
            Curve::ActionPotential {
                burst_threshold,
                peak_multiplier,
                absolute_refractory,
                multiplier_half_life,
            } => {
                if history.is_empty() {
                    return 1.0;
                }
                if delta < *absolute_refractory {
                    return f64::INFINITY;
                }
                // Event-count burst detection. A history entry "counts"
                // toward a burst if its own contribution to the multiplier
                // has not yet decayed past a small epsilon. On a chunky
                // axis this preserves the burst even when a single event
                // advances the tick by thousands.
                let live_events = count_live_events(history, current, *multiplier_half_life);
                if live_events < *burst_threshold {
                    return 1.0;
                }
                if *multiplier_half_life == 0 {
                    return *peak_multiplier;
                }
                let decay = 0.5_f64.powf(delta as f64 / *multiplier_half_life as f64);
                let multiplier = 1.0 + (*peak_multiplier - 1.0) * decay;
                multiplier.max(1.0)
            }
            // Non-refractory curves: inward gate is always open.
            Curve::Exponential { .. }
            | Curve::ProgressiveStaircase { .. }
            | Curve::Flat { .. } => 1.0,
        }
    }
}

/// Epsilon below which a history entry's multiplier contribution is
/// considered decayed out of burst consideration. See ADR-123 Open
/// Questions — this is a first pick and will want empirical tuning.
const BURST_LIVE_EPSILON: f64 = 0.01;

fn count_live_events(history: &[(Tick, f64)], current: Tick, half_life: TickDelta) -> usize {
    if half_life == 0 {
        // Zero half-life degenerates to "only the current tick counts."
        return history.iter().filter(|(t, _)| *t == current).count();
    }
    history
        .iter()
        .filter(|(t, _)| {
            let d = current.saturating_sub(*t);
            let contribution = 0.5_f64.powf(d as f64 / half_life as f64);
            contribution > BURST_LIVE_EPSILON
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_salience_endpoints() {
        let c = Curve::Exponential { half_life: 100 };
        assert!((c.salience_at(0) - 1.0).abs() < 1e-9);
        assert!((c.salience_at(100) - 0.5).abs() < 1e-9);
        assert!((c.salience_at(200) - 0.25).abs() < 1e-9);
        assert!(c.salience_at(10_000) < 1e-20);
    }

    #[test]
    fn exponential_zero_half_life_is_zero() {
        let c = Curve::Exponential { half_life: 0 };
        assert_eq!(c.salience_at(0), 0.0);
        assert_eq!(c.salience_at(100), 0.0);
    }

    #[test]
    fn action_potential_salience_is_unity() {
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        assert_eq!(c.salience_at(0), 1.0);
        assert_eq!(c.salience_at(9_999_999), 1.0);
    }

    #[test]
    fn staircase_salience_picks_latest_passed_step() {
        let c = Curve::ProgressiveStaircase {
            steps: vec![(0, 1.0), (15_000, 0.5), (40_000, 0.2)],
        };
        assert_eq!(c.salience_at(0), 1.0);
        assert_eq!(c.salience_at(14_999), 1.0);
        assert_eq!(c.salience_at(15_000), 0.5);
        assert_eq!(c.salience_at(39_999), 0.5);
        assert_eq!(c.salience_at(40_000), 0.2);
        assert_eq!(c.salience_at(1_000_000), 0.2);
    }

    #[test]
    fn refire_delta_matches_curve_shapes() {
        // Exponential: floor 0.5 → first delta where salience is strictly
        // less than 0.5. At delta = half_life, salience is exactly 0.5,
        // which is NOT strictly less than 0.5, so the first qualifying
        // delta is half_life + 1. Same +1 logic for floor 0.25 at 2×H.
        let c = Curve::Exponential { half_life: 100 };
        assert_eq!(c.refire_delta(0.5), Some(101));
        assert_eq!(c.refire_delta(0.25), Some(201));
        // Non-integer boundary — no off-by-one, ceil and floor+1 agree.
        // log2(1/0.3) ≈ 1.737, 100 × 1.737 ≈ 173.7, floor+1 = 174.
        assert_eq!(c.refire_delta(0.3), Some(174));
        // Floor ≥ 1.0 — salience never strictly less.
        assert_eq!(c.refire_delta(1.0), None);
        assert_eq!(c.refire_delta(1.5), None);

        // Flat: always suppression, regardless of floor.
        let c = Curve::Flat { suppression: 500 };
        assert_eq!(c.refire_delta(0.5), Some(500));
        assert_eq!(c.refire_delta(0.1), Some(500));

        // Staircase: smallest step whose salience drops below floor.
        let c = Curve::ProgressiveStaircase {
            steps: vec![(0, 1.0), (15_000, 0.5), (40_000, 0.2)],
        };
        assert_eq!(c.refire_delta(0.5), Some(40_000));
        assert_eq!(c.refire_delta(0.9), Some(15_000));
        // Floor below the lowest step → None.
        assert_eq!(c.refire_delta(0.1), None);

        // ActionPotential: visualization proxy = abs_refractory + multiplier_half_life.
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        assert_eq!(c.refire_delta(0.5), Some(660));
    }

    #[test]
    fn flat_salience_is_step() {
        // Salience = loudness: loud right after fire, faded past suppression.
        let c = Curve::Flat { suppression: 500 };
        assert_eq!(c.salience_at(0), 1.0);
        assert_eq!(c.salience_at(499), 1.0);
        assert_eq!(c.salience_at(500), 0.0);
        assert_eq!(c.salience_at(10_000), 0.0);
    }

    #[test]
    fn non_refractory_curves_have_unity_multiplier() {
        let history: Vec<(Tick, f64)> = vec![(0, 1.0), (10, 1.0), (20, 1.0)];
        let c = Curve::Exponential { half_life: 100 };
        assert_eq!(c.multiplier_at(5, &history, 25), 1.0);

        let c = Curve::Flat { suppression: 500 };
        assert_eq!(c.multiplier_at(5, &history, 25), 1.0);

        let c = Curve::ProgressiveStaircase {
            steps: vec![(0, 1.0), (15_000, 0.5)],
        };
        assert_eq!(c.multiplier_at(5, &history, 25), 1.0);
    }

    #[test]
    fn action_potential_single_fire_is_unity() {
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        let history: Vec<(Tick, f64)> = vec![(0, 1.0)];
        // One fire isn't a burst — threshold is 3.
        assert_eq!(c.multiplier_at(100, &history, 100), 1.0);
    }

    #[test]
    fn action_potential_burst_triggers_multiplier() {
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        // Three fires, all recent, last one at tick 200. Current = 300,
        // delta = 100 (past absolute_refractory = 60).
        let history: Vec<(Tick, f64)> = vec![(0, 1.0), (100, 1.0), (200, 1.0)];
        let m = c.multiplier_at(100, &history, 300);
        // decay = 0.5^(100/600) ≈ 0.891, so m ≈ 1 + (2 - 1) * 0.891 ≈ 1.891
        assert!(m > 1.8 && m < 2.0, "got multiplier {}", m);
    }

    #[test]
    fn action_potential_absolute_refractory_is_infinite() {
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        let history: Vec<(Tick, f64)> = vec![(0, 1.0), (100, 1.0), (200, 1.0)];
        // delta = 30, inside absolute refractory.
        let m = c.multiplier_at(30, &history, 230);
        assert!(m.is_infinite());
    }

    #[test]
    fn action_potential_multiplier_decays_toward_unity() {
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        let history: Vec<(Tick, f64)> = vec![(0, 1.0), (100, 1.0), (200, 1.0)];
        // Far past burst — all history entries have decayed past epsilon.
        let m = c.multiplier_at(100_000, &history, 100_200);
        assert!((m - 1.0).abs() < 1e-9, "got multiplier {}", m);
    }

    #[test]
    fn action_potential_chunky_axis_burst_survives_large_jump() {
        // Chunky-axis case: single event advances tick by a lot. Burst
        // threshold and multiplier_half_life are in ways-sized units.
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.0,
            absolute_refractory: 500,
            multiplier_half_life: 50_000,
        };
        // Three fires clustered at ticks 0, 8_000, 15_000 (all within
        // multiplier_half_life so they all stay "live"). Current jumps
        // to 22_000 — delta = 7_000 from last fire, past absolute
        // refractory of 500.
        let history: Vec<(Tick, f64)> = vec![(0, 1.0), (8_000, 1.0), (15_000, 1.0)];
        let m = c.multiplier_at(7_000, &history, 22_000);
        // Burst should still be detected; multiplier elevated above 1.0.
        assert!(m > 1.0, "got multiplier {}", m);
    }

    #[test]
    fn yaml_roundtrip_exponential() {
        let c = Curve::Exponential { half_life: 50_000 };
        let s = serde_yaml::to_string(&c).unwrap();
        let back: Curve = serde_yaml::from_str(&s).unwrap();
        assert!(matches!(back, Curve::Exponential { half_life: 50_000 }));
    }

    #[test]
    fn yaml_roundtrip_action_potential() {
        let c = Curve::ActionPotential {
            burst_threshold: 3,
            peak_multiplier: 2.25,
            absolute_refractory: 60,
            multiplier_half_life: 600,
        };
        let s = serde_yaml::to_string(&c).unwrap();
        let back: Curve = serde_yaml::from_str(&s).unwrap();
        match back {
            Curve::ActionPotential {
                burst_threshold,
                peak_multiplier,
                absolute_refractory,
                multiplier_half_life,
            } => {
                assert_eq!(burst_threshold, 3);
                assert!((peak_multiplier - 2.25).abs() < 1e-9);
                assert_eq!(absolute_refractory, 60);
                assert_eq!(multiplier_half_life, 600);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn yaml_roundtrip_staircase() {
        let c = Curve::ProgressiveStaircase {
            steps: vec![(0, 1.0), (15_000, 0.5), (40_000, 0.2)],
        };
        let s = serde_yaml::to_string(&c).unwrap();
        let back: Curve = serde_yaml::from_str(&s).unwrap();
        match back {
            Curve::ProgressiveStaircase { steps } => {
                assert_eq!(steps.len(), 3);
                assert_eq!(steps[0], (0, 1.0));
                assert_eq!(steps[1], (15_000, 0.5));
                assert_eq!(steps[2], (40_000, 0.2));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn yaml_roundtrip_flat() {
        let c = Curve::Flat { suppression: 15_000 };
        let s = serde_yaml::to_string(&c).unwrap();
        let back: Curve = serde_yaml::from_str(&s).unwrap();
        assert!(matches!(back, Curve::Flat { suppression: 15_000 }));
    }
}
