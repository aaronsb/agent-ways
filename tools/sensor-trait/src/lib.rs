//! Sensor trait and runtime types for attend.
//!
//! This crate defines the contract between the attend orchestrator and any sensor
//! implementation. To add a new sensor: implement the `Sensor` trait, publish as
//! a workspace crate, and wire it into attend via a feature flag.

use std::time::{Duration, Instant};

pub mod curve;
pub mod engagement;
pub use curve::Curve;
pub use engagement::EngagementState;

// ── Progression axis (ADR-123) ──────────────────────────────────

/// Monotonic progression-axis value supplied by the caller. The engine
/// treats this as an opaque strictly-increasing `u64` — attend interprets
/// it as wall-clock seconds, ways as token position, future callers as
/// whatever monotonic matches their cadence. See ADR-123.
pub type Tick = u64;

/// Delta between two ticks on the same progression axis. Same unit as
/// the ticks it was computed from; the engine does not know what that
/// unit is.
pub type TickDelta = u64;

// ── Metadata macro ──────────────────────────────────────────────

/// Default `description()` and `source()` impls for built-in sensor crates.
/// Invoke inside an `impl Sensor for ...` block to get:
///   - `description()` returning `env!("CARGO_PKG_DESCRIPTION")` (so the
///     user-facing text lives in `Cargo.toml`, single source of truth).
///   - `source()` returning `format!("{name}@{version}")` from the crate's
///     own `CARGO_PKG_NAME` / `CARGO_PKG_VERSION`.
///
/// `env!()` resolves at the macro-expansion site (i.e. the calling crate),
/// so each sensor crate gets its own metadata without further wiring.
///
/// External (non-built-in) sensor implementors who don't want to publish
/// their crate via Cargo can simply implement `description()` / `source()`
/// directly instead.
#[macro_export]
macro_rules! sensor_metadata {
    () => {
        /// Sourced from this crate's `Cargo.toml` `description` field — edit
        /// there to change what `attend sensors` prints.
        fn description(&self) -> &str {
            env!("CARGO_PKG_DESCRIPTION")
        }

        /// `{crate}@{version}` from this crate's package metadata.
        fn source(&self) -> String {
            format!(
                "{}@{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            )
        }
    };
}

// ── Sensor trait ────────────────────────────────────────────────

/// A sensor observes some aspect of the environment or Claude's state.
///
/// Sensors are polled by the tick loop on adaptive schedules.
/// They return observations that feed into delta accumulators.
///
/// The `Send` bound prepares for the daemon model where sensors may
/// run on separate threads.
pub trait Sensor: Send {
    /// Unique name for this sensor (used in output format and state keys).
    fn name(&self) -> &str;

    /// One-line description of what this sensor observes. Surfaced by
    /// `attend sensors` for discovery. Default empty so the trait stays
    /// backward compatible; concrete sensors should override.
    fn description(&self) -> &str {
        ""
    }

    /// Sensor source identifier — typically `crate@version` for built-in
    /// crate sensors, or a script path for `ScriptSensor`. Surfaced by
    /// `attend sensors`. Default empty; concrete sensors should override.
    fn source(&self) -> String {
        String::new()
    }

    /// Poll the sensor's data source. Returns a list of observations.
    /// Each observation is a (delta_magnitude, description) pair.
    /// Empty vec = no change detected this tick.
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;

    /// Per-sensor emission threshold — accumulated magnitude needed before
    /// this sensor is a candidate for disclosure.
    fn emission_threshold(&self) -> f64;

    /// Base polling interval (maximum, used when quiet).
    fn base_interval(&self) -> Duration;

    /// Minimum polling interval (fastest, used during active changes).
    fn min_interval(&self) -> Duration;

    /// Number of quiet ticks before interval decays back toward base.
    fn decay_threshold(&self) -> u32;

    /// Export state for checkpointing. Default: no state.
    fn export_state(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Import state from checkpoint. Default: no-op.
    fn import_state(&mut self, _state: &[(String, String)]) {}
}

// ── Focus ───────────────────────────────────────────────────────

/// What Claude is currently focused on. Shapes sensor relevance and thresholds.
#[derive(Clone, Debug)]
pub struct Focus {
    /// Short description of current work ("debugging auth module", "writing tests").
    pub description: String,
    /// Working directory — sensors can use this to scope observations.
    pub working_dir: String,
    /// Keywords that increase sensor relevance when matched.
    pub keywords: Vec<String>,
}

impl Focus {
    pub fn default_focus() -> Self {
        Self {
            description: "general session".to_string(),
            working_dir: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            keywords: Vec::new(),
        }
    }
}

// ── Delta accumulator ───────────────────────────────────────────

/// Accumulates real state changes across sensor ticks.
/// Magnitude grows monotonically until disclosed or decayed.
pub struct DeltaAccumulator {
    pub magnitude: f64,
    pub event_count: u32,
    pub window_start: Instant,
    pub events: Vec<String>,
}

impl DeltaAccumulator {
    pub fn new() -> Self {
        Self {
            magnitude: 0.0,
            event_count: 0,
            window_start: Instant::now(),
            events: Vec::new(),
        }
    }

    pub fn accumulate(&mut self, delta: f64, description: String) {
        self.magnitude += delta;
        self.event_count += 1;
        self.events.push(description);
    }

    pub fn reset(&mut self) {
        self.magnitude = 0.0;
        self.event_count = 0;
        self.window_start = Instant::now();
        self.events.clear();
    }

    /// Single-line summary (legacy, for non-paged contexts).
    pub fn summary(&self) -> String {
        if self.events.is_empty() {
            return String::new();
        }
        let unique = self.unique_events();
        if unique.len() == 1 {
            unique[0].clone()
        } else {
            format!(
                "{} observations: {}",
                unique.len(),
                unique.join("; ")
            )
        }
    }

    /// Drain events as individual lines for paged emission.
    /// Each event becomes its own notification line via Monitor.
    pub fn drain_events(&self) -> Vec<String> {
        self.unique_events()
    }

    fn unique_events(&self) -> Vec<String> {
        let mut unique: Vec<String> = Vec::new();
        for event in &self.events {
            if !unique.contains(event) {
                unique.push(event.clone());
            }
        }
        unique
    }
}

impl Default for DeltaAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Adaptive interval ───────────────────────────────────────────

/// Per-sensor adaptive polling interval.
/// Ramps up fast on change, decays slowly on quiet (hysteresis).
pub struct AdaptiveInterval {
    pub base: Duration,
    pub current: Duration,
    pub min: Duration,
    pub decay_threshold: u32,
    pub ramp_cooldown: u32,
}

impl AdaptiveInterval {
    pub fn new(base: Duration, min: Duration, decay_threshold: u32) -> Self {
        Self {
            base,
            current: base,
            min,
            decay_threshold,
            ramp_cooldown: 0,
        }
    }

    pub fn on_change(&mut self) {
        self.current = self.current.div_f64(2.0).max(self.min);
        self.ramp_cooldown = 0;
    }

    pub fn on_quiet(&mut self) {
        self.ramp_cooldown += 1;
        if self.ramp_cooldown >= self.decay_threshold {
            self.current = self.current.mul_f64(1.5).min(self.base);
        }
    }
}

// ── Sensor slot ─────────────────────────────────────────────────

/// Wall-clock tick source for attend — seconds since the UNIX epoch.
///
/// Attend interprets the progression axis as wall-clock seconds, so this
/// is the canonical tick source for `SensorSlot` and anything else in
/// attend that touches the firing engine. Lives in `sensor-trait` so
/// `SensorSlot::poll` can use it without a circular dependency.
pub fn epoch_secs() -> Tick {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Default `Curve::ActionPotential` for sensor slots. Mirrors the defaults
/// that the old `EngagementState::new()` shipped with, so sensors booted
/// without config overrides preserve ADR-119 behavior.
fn default_sensor_curve() -> Curve {
    Curve::ActionPotential {
        burst_threshold: 3,
        // step_multiplier = 1.25 at burst_threshold = 3 → peak of 2.25
        // under the old math (`1 + steps * step_multiplier` with steps=1).
        peak_multiplier: 2.25,
        absolute_refractory: 60,
        // Old `decay_per_minute = 0.1` (linear) ≈ exponential half-life
        // 395 s per the conversion formula in ADR-123 / the Phase B
        // worksheet.
        multiplier_half_life: 395,
    }
}

/// A sensor with its runtime state (interval, accumulator, scheduling).
pub struct SensorSlot {
    pub sensor: Box<dyn Sensor>,
    pub interval: AdaptiveInterval,
    pub accumulator: DeltaAccumulator,
    pub engagement: EngagementState,
    pub next_fire: Instant,
}

impl SensorSlot {
    pub fn new(sensor: Box<dyn Sensor>) -> Self {
        let interval = AdaptiveInterval::new(
            sensor.base_interval(),
            sensor.min_interval(),
            sensor.decay_threshold(),
        );
        Self {
            accumulator: DeltaAccumulator::new(),
            engagement: EngagementState::new(default_sensor_curve()),
            next_fire: Instant::now(),
            interval,
            sensor,
        }
    }

    /// Create a SensorSlot with config-overridden intervals.
    pub fn new_with_config(
        sensor: Box<dyn Sensor>,
        base: Duration,
        min: Duration,
        decay_threshold: u32,
    ) -> Self {
        let interval = AdaptiveInterval::new(base, min, decay_threshold);
        Self {
            accumulator: DeltaAccumulator::new(),
            engagement: EngagementState::new(default_sensor_curve()),
            next_fire: Instant::now(),
            interval,
            sensor,
        }
    }

    pub fn poll(&mut self, focus: &Focus) -> bool {
        let observations = self.sensor.poll(focus);
        if observations.is_empty() {
            self.interval.on_quiet();
            return false;
        }

        let tick = epoch_secs();

        // Absolute refractory: drop all observations silently. The agent
        // needs a beat to process what just arrived.
        if self.engagement.in_absolute_refractory(tick) {
            self.interval.on_change();
            return true;
        }

        // Relative refractory: per-event gating. Each observation must
        // individually clear the elevated threshold to be accumulated.
        // At rest (multiplier = 1.0), no filtering — all events accumulate
        // normally and the aggregate check in ready_to_disclose applies.
        // During refractory, sub-threshold events are silently dropped
        // (true disengagement, not delayed firing).
        let base = self.sensor.emission_threshold();
        let multiplier = self.engagement.current_multiplier(tick);
        let per_event_gate = if multiplier > 1.0 { base * multiplier } else { 0.0 };

        let mut accepted = 0;
        for (magnitude, description) in observations {
            if magnitude >= per_event_gate {
                self.accumulator.accumulate(magnitude, description);
                accepted += 1;
            }
        }

        if accepted > 0 {
            self.interval.on_change();
            true
        } else {
            // All events filtered — count as quiet for adaptive interval
            self.interval.on_quiet();
            false
        }
    }

    pub fn ready_to_disclose(&self) -> bool {
        match self
            .engagement
            .effective_threshold(self.sensor.emission_threshold(), epoch_secs())
        {
            None => false, // absolute refractory — hard block
            Some(threshold) => self.accumulator.magnitude >= threshold,
        }
    }

    /// Returns the current effective threshold (base * refractory multiplier),
    /// or None if in absolute refractory. For diagnostics/logging.
    pub fn effective_threshold(&self) -> Option<f64> {
        self.engagement
            .effective_threshold(self.sensor.emission_threshold(), epoch_secs())
    }

    pub fn schedule_next(&mut self) {
        self.next_fire = Instant::now() + self.interval.current;
    }

    pub fn name(&self) -> &str {
        self.sensor.name()
    }

    pub fn description(&self) -> &str {
        self.sensor.description()
    }

    pub fn source(&self) -> String {
        self.sensor.source()
    }

    pub fn export_state(&self) -> Vec<(String, String)> {
        self.sensor.export_state()
    }

    pub fn import_state(&mut self, state: &[(String, String)]) {
        self.sensor.import_state(state);
    }
}

// ── Shared parsing utilities ────────────────────────────────────

/// Quick-and-dirty JSON u64 extraction without pulling serde into the
/// sensor crates. Finds `"key":value` (whitespace-tolerant after the colon)
/// and parses the leading digit run as a u64. Returns `None` if the key is
/// absent, the value is non-numeric, or the run is empty.
///
/// Lives here so `sensor-context`, `sensor-peers`, and `sensor-disclosure`
/// don't each carry their own copy. Good enough for the `ways context --json`
/// and signal-file formats the sensors consume.
pub fn extract_json_u64(text: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = text.find(&pattern)? + pattern.len();
    let rest = text[start..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse().ok()
}

#[cfg(test)]
mod util_tests {
    use super::*;

    #[test]
    fn extract_json_u64_basic() {
        assert_eq!(extract_json_u64(r#"{"tokens_used":12345}"#, "tokens_used"), Some(12345));
        assert_eq!(extract_json_u64(r#"{"tokens_used": 12345}"#, "tokens_used"), Some(12345));
        assert_eq!(extract_json_u64(r#"{"tokens_used":0}"#, "tokens_used"), Some(0));
    }

    #[test]
    fn extract_json_u64_missing_key() {
        assert_eq!(extract_json_u64(r#"{"other":1}"#, "tokens_used"), None);
    }

    #[test]
    fn extract_json_u64_non_numeric() {
        assert_eq!(extract_json_u64(r#"{"tokens_used":"str"}"#, "tokens_used"), None);
    }
}
