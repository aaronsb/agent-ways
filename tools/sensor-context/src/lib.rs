use sensor_trait::{extract_json_u64, Focus, Sensor};
use std::process::Command;
use std::time::{Duration, Instant};

/// Interoceptive sensor: watches Claude's own context window usage.
/// Polls `ways context --json` for token counts, tracks velocity,
/// and projects when compaction will hit.
///
/// This is the canonical first sensor from ADR-113 — the one that
/// prevents Claude from silently running off the context cliff.
pub struct ContextSensor {
    /// Previous reading
    prior: Option<ContextReading>,
    /// History of readings for velocity calculation
    history: Vec<ContextReading>,
    /// Thresholds already disclosed (to avoid repeating)
    disclosed_thresholds: Vec<u8>,
    /// First poll establishes baseline
    baseline_established: bool,
}

#[derive(Clone, Debug)]
struct ContextReading {
    pct_used: f64,
    tokens_used: u64,
    tokens_total: u64,
    taken_at: Instant,
}

impl ContextSensor {
    pub fn new() -> Self {
        Self {
            prior: None,
            history: Vec::new(),
            disclosed_thresholds: Vec::new(),
            baseline_established: false,
        }
    }

    fn read_context(&self, focus: &Focus) -> Option<ContextReading> {
        // Run ways context --json from the working directory
        let output = Command::new("ways")
            .args(["context", "--json"])
            .current_dir(&focus.working_dir)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse JSON fields without serde
        let tokens_used = extract_json_u64(&stdout, "tokens_used")?;
        let tokens_total = extract_json_u64(&stdout, "tokens_total")?;
        let pct_used = extract_f64(&stdout, "pct_used")?;

        Some(ContextReading {
            pct_used,
            tokens_used,
            tokens_total,
            taken_at: Instant::now(),
        })
    }

    /// Calculate velocity: percent per minute based on recent history
    fn velocity(&self) -> Option<f64> {
        if self.history.len() < 2 {
            return None;
        }

        // Use oldest and newest readings for smoother velocity
        let oldest = self.history.first()?;
        let newest = self.history.last()?;
        let elapsed_mins = newest.taken_at.duration_since(oldest.taken_at).as_secs_f64() / 60.0;

        if elapsed_mins < 0.5 {
            return None; // Not enough time elapsed for meaningful velocity
        }

        let delta_pct = newest.pct_used - oldest.pct_used;
        Some(delta_pct / elapsed_mins)
    }

    /// Project how many minutes until a given threshold, based on velocity
    fn project_minutes_to(&self, target_pct: f64, current_pct: f64) -> Option<f64> {
        let vel = self.velocity()?;
        if vel <= 0.0 {
            return None; // Not growing
        }
        let remaining_pct = target_pct - current_pct;
        if remaining_pct <= 0.0 {
            return Some(0.0); // Already past
        }
        Some(remaining_pct / vel)
    }
}

impl Default for ContextSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensor for ContextSensor {
    fn name(&self) -> &str {
        "context"
    }

    sensor_trait::sensor_metadata!();

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = match self.read_context(focus) {
            Some(r) => r,
            None => return Vec::new(),
        };

        // First poll: establish baseline
        if !self.baseline_established {
            eprintln!(
                "[attend] context: baseline — {:.0}% used ({:.0}k / {:.0}k tokens)",
                current.pct_used,
                current.tokens_used as f64 / 1000.0,
                current.tokens_total as f64 / 1000.0,
            );
            self.history.push(current.clone());
            self.prior = Some(current);
            self.baseline_established = true;
            return Vec::new();
        }

        // Track history (keep last 20 readings for velocity smoothing)
        self.history.push(current.clone());
        if self.history.len() > 20 {
            self.history.remove(0);
        }

        let mut observations = Vec::new();

        // Attend handles trajectory awareness. Ways handles threshold
        // actions (todos@75%, memory@80%, checkpoint@95%).
        // Attend's thresholds are early warnings *before* ways fires,
        // plus velocity context that ways can't provide.
        let thresholds: &[(u8, f64, &str)] = &[
            (40, 1.5, "approaching midpoint — plan wrap-up scope"),
            (50, 2.0, "midpoint — wrap-up window opening"),
            (65, 3.0, "ways will fire todos checkpoint at 75%"),
            (85, 4.0, "ways fired memory save at 80% — verify it happened"),
            (92, 5.0, "compaction checkpoint at 95% — finish current task"),
        ];

        for &(pct, magnitude, label) in thresholds {
            if current.pct_used >= pct as f64
                && !self.disclosed_thresholds.contains(&pct)
            {
                let mut msg = format!(
                    "context at {:.0}% — {}", current.pct_used, label
                );

                // Add velocity and projection if available
                if let Some(vel) = self.velocity() {
                    if vel > 0.1 {
                        msg.push_str(&format!(" (burning {:.1}%/min", vel));
                        if pct < 95 {
                            if let Some(mins) = self.project_minutes_to(95.0, current.pct_used) {
                                if mins < 60.0 {
                                    msg.push_str(&format!(", ~{:.0} min to critical", mins));
                                }
                            }
                        }
                        msg.push(')');
                    }
                }

                // High-salience thresholds get an affordance (ADR-114)
                if pct >= 85 {
                    msg.push_str(". Use `ways show attend context-pressure --session $CLAUDE_SESSION_ID` for reflection guidance");
                }

                observations.push((magnitude, msg));
                self.disclosed_thresholds.push(pct);
            }
        }

        // Velocity change detection (even between thresholds)
        if let Some(prior) = &self.prior {
            if let Some(vel) = self.velocity() {
                // Sudden acceleration: velocity doubled
                let prior_elapsed = prior.taken_at.elapsed().as_secs_f64() / 60.0;
                if prior_elapsed > 1.0 && vel > 2.0 {
                    let pct_change = current.pct_used - prior.pct_used;
                    if pct_change > 5.0 {
                        observations.push((2.0, format!(
                            "context velocity spike: {:.0}% in last {:.0} min ({:.1}%/min)",
                            pct_change, prior_elapsed, vel
                        )));
                    }
                }
            }
        }

        self.prior = Some(current);
        observations
    }

    fn emission_threshold(&self) -> f64 {
        1.5 // Lower than other sensors — context warnings are high priority
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(60) // Check every 60s at rest
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(20) // Ramp up to 20s when context is moving fast
    }

    fn decay_threshold(&self) -> u32 {
        3 // Decay quickly back to base when context is stable
    }

    fn export_state(&self) -> Vec<(String, String)> {
        let mut state = Vec::new();
        for t in &self.disclosed_thresholds {
            state.push(("disclosed_threshold".to_string(), t.to_string()));
        }
        if let Some(ref prior) = self.prior {
            state.push(("context_pct".to_string(), format!("{:.1}", prior.pct_used)));
        }
        state
    }

    fn import_state(&mut self, state: &[(String, String)]) {
        for (key, value) in state {
            if key.as_str() == "disclosed_threshold" {
                if let Ok(t) = value.parse::<u8>() {
                    if !self.disclosed_thresholds.contains(&t) {
                        self.disclosed_thresholds.push(t);
                    }
                }
            }
        }
        if !self.disclosed_thresholds.is_empty() {
            eprintln!("[attend] context: restored {} disclosed thresholds from checkpoint",
                self.disclosed_thresholds.len());
        }
    }
}

/// Extract an f64 from JSON-like text: "key":value
fn extract_f64(text: &str, key: &str) -> Option<f64> {
    let pattern = format!("\"{}\":", key);
    let start = text.find(&pattern)? + pattern.len();
    let rest = text[start..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
    if end == 0 { return None; }
    rest[..end].parse().ok()
}
