//! Disclosure sensor: reheats affordance instructions (messaging, scenes, focus, ...)
//! on token-distance drift from the previous disclosure.
//!
//! Shape: an ordinary `impl Sensor` whose signal is token distance rather than
//! environmental change. Each `poll` shells out to `ways context --json` once,
//! reads `tokens_used`, and checks each registered component:
//!
//!   - No marker yet → emit the component's disclosure text, stamp baseline.
//!   - Marker present, distance ≥ 25% of context window → emit, re-stamp.
//!   - Otherwise → no observation for that component.
//!
//! State is `HashMap<Component, u64>` on the sensor struct — in-process memory
//! only, clean-restart semantics by construction.

use sensor_trait::{extract_json_u64, Focus, Sensor};
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

/// Reheat threshold — a component re-discloses when token distance from its
/// last disclosure exceeds this percentage of the current model's context
/// window. Matches `ways-cli`'s `REDISCLOSE_PCT` so one dial governs both.
const REDISCLOSE_PCT: u64 = 25;

/// Fixed magnitude for a disclosure observation. Sized to clear the default
/// emission threshold on a single poll — "urgency building up" is conceptual
/// (progress toward the token-distance gate), not a gradual accumulation.
const DISCLOSURE_MAGNITUDE: f64 = 5.0;

// ── Component registry ─────────────────────────────────────────

/// Registered disclosure components. Add a variant + a matching markdown file
/// under `src/disclosures/` to introduce a new reheatable affordance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Component {
    Messaging,
}

impl Component {
    fn id(self) -> &'static str {
        match self {
            Component::Messaging => "messaging",
        }
    }

    fn body(self) -> &'static str {
        match self {
            // Messaging guidance lives in three synchronized sources; an
            // edit to `disclosures/messaging.md` must be mirrored to the
            // other two or agents will receive inconsistent guidance at
            // different points in a session:
            //   - skills/attend/SKILL.md                               (primer read at /attend invocation)
            //   - tools/sensor-disclosure/src/disclosures/messaging.md (this file — runtime reheat)
            //   - hooks/ways/softwaredev/environment/attend/attend.md  (just-in-time way via commands: attend)
            // Note: the .md file is embedded via include_str! and
            // becomes the literal reheat payload, so no HTML comments
            // can live inside the file itself — this is the closest
            // anchoring point that doesn't leak into the notification.
            Component::Messaging => include_str!("disclosures/messaging.md"),
        }
    }

    fn all() -> &'static [Component] {
        &[Component::Messaging]
    }
}

// ── The sensor ─────────────────────────────────────────────────

pub struct DisclosureSensor {
    /// Last `tokens_used` value at which each component was disclosed.
    /// Missing key = never disclosed this process → fire unconditionally.
    ledger: HashMap<Component, u64>,
}

impl DisclosureSensor {
    pub fn new() -> Self {
        Self {
            ledger: HashMap::new(),
        }
    }

    /// Shell out to `ways context --json` and return the `tokens_used` field.
    /// Matches `sensor-context`'s integration pattern.
    fn read_tokens_used(&self, focus: &Focus) -> Option<(u64, u64)> {
        let output = Command::new("ways")
            .args(["context", "--json"])
            .current_dir(&focus.working_dir)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let tokens_used = extract_json_u64(&stdout, "tokens_used")?;
        let tokens_total = extract_json_u64(&stdout, "tokens_total")?;
        Some((tokens_used, tokens_total))
    }

    /// Format the observation text for a component. The body is prefixed with
    /// a single tagged line so Claude can recognize this as an affordance
    /// reheat regardless of which component fired.
    fn format_observation(&self, component: Component) -> String {
        format!(
            "reheat: attend affordances — {}\n{}",
            component.id(),
            component.body().trim_end()
        )
    }
}

impl Default for DisclosureSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl DisclosureSensor {
    /// Pure ledger-transition logic, extracted from `poll` so it can be
    /// unit-tested without shelling out to `ways context --json`.
    ///
    /// For each registered component, decides whether it should fire this
    /// tick (missing marker OR distance ≥ threshold) and, if so, appends
    /// an observation and stamps the new baseline into the ledger.
    fn check_and_stamp(
        &mut self,
        tokens_used: u64,
        tokens_total: u64,
    ) -> Vec<(f64, String)> {
        if tokens_total == 0 {
            return Vec::new();
        }

        let threshold_tokens = tokens_total * REDISCLOSE_PCT / 100;
        let mut observations = Vec::new();

        for &component in Component::all() {
            let should_fire = match self.ledger.get(&component) {
                None => true,
                Some(&last) => tokens_used.saturating_sub(last) >= threshold_tokens,
            };
            if should_fire {
                observations.push((DISCLOSURE_MAGNITUDE, self.format_observation(component)));
                self.ledger.insert(component, tokens_used);
            }
        }

        observations
    }
}

impl Sensor for DisclosureSensor {
    fn name(&self) -> &str {
        "disclosure"
    }

    sensor_trait::sensor_metadata!();

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let (tokens_used, tokens_total) = match self.read_tokens_used(focus) {
            Some(pair) => pair,
            None => return Vec::new(),
        };
        self.check_and_stamp(tokens_used, tokens_total)
    }

    fn emission_threshold(&self) -> f64 {
        // Matches `sensor-context` — disclosures are high-priority by
        // nature (they carry instructional payload, not telemetry).
        1.5
    }

    fn base_interval(&self) -> Duration {
        // Poll at the same rest cadence as `sensor-context`. The subprocess
        // call is the same operation against the same file.
        Duration::from_secs(60)
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(20)
    }

    fn decay_threshold(&self) -> u32 {
        3
    }

    // State is intentionally ephemeral — no export/import. A restarted attend
    // process starts with an empty ledger and re-teaches on first poll, which
    // is the designed clean-restart behavior.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_registry_has_messaging_body() {
        let body = Component::Messaging.body();
        assert!(body.contains("attend send"));
        // Post-ADR-124: default scope is `#open` (the base channel),
        // previously labelled `broadcast`. The assertion follows the
        // display-layer rename so any future regression in the
        // disclosure copy gets caught.
        assert!(body.contains("#open"));
        assert!(!body.is_empty());
    }

    #[test]
    fn format_observation_contains_tag_and_body() {
        let sensor = DisclosureSensor::new();
        let obs = sensor.format_observation(Component::Messaging);
        assert!(obs.starts_with("reheat: attend affordances — messaging"));
        assert!(obs.contains("attend send"));
    }

    // ── Ledger transition tests (exercise check_and_stamp directly
    //    so no `ways context --json` subprocess is needed). ──

    const TOKENS_TOTAL: u64 = 1_000_000; // Opus 1M window — threshold at 250k.
    const THRESHOLD: u64 = TOKENS_TOTAL * REDISCLOSE_PCT / 100;

    #[test]
    fn first_check_fires_every_component_and_stamps_ledger() {
        let mut sensor = DisclosureSensor::new();
        assert!(sensor.ledger.is_empty());

        let obs = sensor.check_and_stamp(100_000, TOKENS_TOTAL);

        assert_eq!(obs.len(), Component::all().len());
        assert_eq!(sensor.ledger.len(), Component::all().len());
        for &c in Component::all() {
            assert_eq!(sensor.ledger.get(&c), Some(&100_000));
        }
    }

    #[test]
    fn second_check_below_threshold_is_silent() {
        let mut sensor = DisclosureSensor::new();
        sensor.check_and_stamp(100_000, TOKENS_TOTAL); // baseline
        let obs = sensor.check_and_stamp(200_000, TOKENS_TOTAL); // +100k < 250k
        assert!(obs.is_empty(), "expected no fire at sub-threshold drift, got {:?}", obs);
        // Ledger is unchanged (still stamped at baseline).
        for &c in Component::all() {
            assert_eq!(sensor.ledger.get(&c), Some(&100_000));
        }
    }

    #[test]
    fn second_check_at_threshold_refires_and_restamps() {
        let mut sensor = DisclosureSensor::new();
        sensor.check_and_stamp(100_000, TOKENS_TOTAL); // baseline
        let obs = sensor.check_and_stamp(100_000 + THRESHOLD, TOKENS_TOTAL); // exactly at threshold
        assert_eq!(obs.len(), Component::all().len());
        for &c in Component::all() {
            assert_eq!(sensor.ledger.get(&c), Some(&(100_000 + THRESHOLD)));
        }
    }

    #[test]
    fn second_check_above_threshold_refires_and_restamps() {
        let mut sensor = DisclosureSensor::new();
        sensor.check_and_stamp(100_000, TOKENS_TOTAL);
        let obs = sensor.check_and_stamp(500_000, TOKENS_TOTAL); // +400k > 250k
        assert_eq!(obs.len(), Component::all().len());
        for &c in Component::all() {
            assert_eq!(sensor.ledger.get(&c), Some(&500_000));
        }
    }

    #[test]
    fn saturating_sub_guards_against_token_regression() {
        // If tokens_used regresses (compaction, transcript rewrite), distance
        // saturates to 0 rather than underflowing. A component below threshold
        // stays silent rather than firing spuriously.
        let mut sensor = DisclosureSensor::new();
        sensor.check_and_stamp(500_000, TOKENS_TOTAL); // baseline at 500k
        let obs = sensor.check_and_stamp(100_000, TOKENS_TOTAL); // regressed to 100k
        assert!(obs.is_empty(), "regression must not refire, got {:?}", obs);
        // Ledger still carries the prior (higher) baseline.
        for &c in Component::all() {
            assert_eq!(sensor.ledger.get(&c), Some(&500_000));
        }
    }

    #[test]
    fn zero_tokens_total_is_silent_safeguard() {
        // If `ways context --json` ever returns 0 for the window (shouldn't
        // happen, but guard exists), the sensor emits nothing rather than
        // dividing by zero or computing a nonsense threshold.
        let mut sensor = DisclosureSensor::new();
        let obs = sensor.check_and_stamp(100_000, 0);
        assert!(obs.is_empty());
        assert!(sensor.ledger.is_empty());
    }
}
