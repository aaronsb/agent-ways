// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use sensor_trait::{Focus, Sensor};
use std::time::{Duration, Instant};
use ways_core::context_fill;

/// Interoceptive sensor: watches Claude's own context window usage.
/// Reads the session's fill through `ways_core::context_fill`, the
/// measurement `ways context` prints, tracks velocity, and projects when
/// compaction will hit.
///
/// This is the canonical first sensor from ADR-113 — the one that
/// prevents Claude from silently running off the context cliff.
pub struct ContextSensor {
    /// The session whose transcript is measured. With none there is no
    /// reading: the newest transcript in the project can be another
    /// session's, and its fill would be reported as this one's (#1361).
    session: Option<String>,
    /// Where the session's transcript is looked for.
    claude: claude_sessions::ClaudeDir,
    /// Previous reading
    prior: Option<ContextReading>,
    /// History of readings for velocity calculation
    history: Vec<ContextReading>,
    /// Bands already disclosed (to avoid repeating)
    disclosed_thresholds: Vec<u8>,
    /// First poll establishes baseline
    baseline_established: bool,
}

#[derive(Clone, Debug)]
struct ContextReading {
    /// Unrounded percent, for velocity.
    pct_used: f64,
    /// The whole percent `ways context` prints.
    pct_shown: u64,
    tokens_used: u64,
    tokens_total: u64,
    taken_at: Instant,
}

/// One fill band: the percent it starts at, its magnitude, and what it says.
struct Band {
    at: u8,
    magnitude: f64,
    says: &'static str,
}

/// Attend handles trajectory awareness. Ways handles threshold actions
/// (todos@75%, memory@80%, checkpoint@85%). Attend's bands say where the
/// fill stands against those actions, plus velocity context that ways can't
/// provide. A band runs from its `at` up to the next band's, and its wording
/// is true at every percent in that range: each ways action gets a band
/// boundary, so no band calls an action ahead that a reading in it has
/// already passed.
const BANDS: &[Band] = &[
    Band { at: 50, magnitude: 1.5, says: "over half the context window is used — if the work's on track, keep going; if you've barely started, this is a calm moment to scope the next stages" },
    Band { at: 65, magnitude: 3.0, says: "agent-ways will capture todos (75%) and memory (80%) shortly" },
    Band { at: 75, magnitude: 3.0, says: "past the todos capture at 75%; agent-ways captures memory at 80%, and the compaction checkpoint follows at 85%" },
    Band { at: 80, magnitude: 4.0, says: "past the memory capture at 80%; the compaction checkpoint fires at 85% — finish the current task and sync" },
    Band { at: 85, magnitude: 4.0, says: "past the compaction checkpoint at 85% — finish the current task and sync" },
    Band { at: 90, magnitude: 5.0, says: "stop or compact right now — quality degrades past here, and auto-compact is off" },
];

/// How far under a spent band's start the fill must fall before the band
/// can sound again. A compaction falls far past it; a fill that wavers a
/// point around a boundary does not repeat the band.
const REARM_MARGIN: u64 = 5;

/// The band `pct` falls in: the highest one it has reached.
fn band_for(pct: u64) -> Option<&'static Band> {
    BANDS.iter().rev().find(|b| pct >= u64::from(b.at))
}

impl ContextSensor {
    /// The sensor for `session`, reading transcripts where `ways context`
    /// reads them.
    pub fn new(session: Option<&str>) -> Self {
        Self::in_dir(session, ways_core::paths::claude_dir())
    }

    fn in_dir(session: Option<&str>, claude: claude_sessions::ClaudeDir) -> Self {
        Self {
            session: session.filter(|s| !s.is_empty()).map(str::to_string),
            claude,
            prior: None,
            history: Vec::new(),
            disclosed_thresholds: Vec::new(),
            baseline_established: false,
        }
    }

    fn read_context(&self, focus: &Focus) -> Option<ContextReading> {
        let session = self.session.as_deref()?;
        let fill = context_fill::measure_session(&self.claude, Some(&focus.working_dir), session)?;
        Some(ContextReading {
            pct_used: fill.percent(),
            pct_shown: fill.pct_used(),
            tokens_used: fill.tokens_used,
            tokens_total: fill.window.tokens,
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

impl Sensor for ContextSensor {
    fn name(&self) -> &str {
        "context"
    }

    fn description(&self) -> &str {
        "tracks context window usage and projects compaction"
    }

    fn source(&self) -> String {
        concat!("attend@", env!("CARGO_PKG_VERSION")).to_string()
    }

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = match self.read_context(focus) {
            Some(r) => r,
            None => return Vec::new(),
        };

        // `/compact` keeps the session id, and a restarted host restores the
        // spent bands, so a fill that has fallen well under a spent band
        // re-arms it: the band sounds again when the fill climbs back.
        self.disclosed_thresholds.retain(|&t| current.pct_shown + REARM_MARGIN > u64::from(t));

        // First poll: establish baseline
        if !self.baseline_established {
            eprintln!(
                "[attend] context: baseline — {}% used ({:.0}k / {:.0}k tokens)",
                current.pct_shown,
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

        // Only the band the fill is in speaks, and reaching it spends the
        // bands below: a reading that lands past two bands says the higher
        // one alone, never a lower band's wording beside a higher percent.
        if let Some(band) = band_for(current.pct_shown) {
            if !self.disclosed_thresholds.contains(&band.at) {
                let mut msg = format!("context at {}% — {}", current.pct_shown, band.says);

                // Add velocity and projection if available
                if let Some(vel) = self.velocity() {
                    if vel > 0.1 {
                        msg.push_str(&format!(" (burning {:.1}%/min", vel));
                        if band.at < 90 {
                            if let Some(mins) = self.project_minutes_to(90.0, current.pct_used) {
                                if mins < 60.0 {
                                    msg.push_str(&format!(", ~{:.0} min to critical", mins));
                                }
                            }
                        }
                        msg.push(')');
                    }
                }

                // High-salience bands name the way to read for reflection.
                //
                // References:
                // - ADR-114: a high-salience observation carries an affordance.
                if band.at >= 85 {
                    msg.push_str(". Use `ways show attend context-pressure --session $CLAUDE_SESSION_ID` for reflection guidance");
                }

                observations.push((band.magnitude, msg));
            }
            for b in BANDS.iter().take_while(|b| b.at <= band.at) {
                if !self.disclosed_thresholds.contains(&b.at) {
                    self.disclosed_thresholds.push(b.at);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(model: &str, tokens: u64) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"model":"{model}","usage":{{"input_tokens":0,"cache_read_input_tokens":{tokens},"cache_creation_input_tokens":0}}}}}}"#
        )
    }

    /// A Claude config directory holding one project with the given
    /// `(session, transcript text)` pairs, written oldest first.
    fn claude_dir(name: &str, project: &str, sessions: &[(&str, String)]) -> claude_sessions::ClaudeDir {
        let root = std::env::temp_dir().join(format!("attend-context-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("projects").join(claude_sessions::project_slug(project));
        std::fs::create_dir_all(&dir).unwrap();
        for (sid, text) in sessions {
            std::fs::write(dir.join(format!("{sid}.jsonl")), text).unwrap();
        }
        claude_sessions::ClaudeDir::at(root)
    }

    fn focus(project: &str) -> Focus {
        Focus { description: String::new(), working_dir: project.to_string(), keywords: Vec::new() }
    }

    /// #1361: a claude-opus-5-5 session at 173,884 tokens is 17% of its 1M
    /// window, as `ways context` reads it. A newer transcript in the same
    /// project, 138k on a model with no table entry (200k default), is
    /// another session's and must not be reported as this one's 69%.
    #[test]
    fn a_1m_session_at_17_percent_does_not_read_69() {
        let project = "/srv/ctx-1361";
        let claude = claude_dir(
            "1361",
            project,
            &[("own", turn("claude-opus-5-5", 173_884)), ("other", turn("claude-unlisted-1", 138_000))],
        );
        let sensor = ContextSensor::in_dir(Some("own"), claude.clone());
        let r = sensor.read_context(&focus(project)).unwrap();
        assert_eq!((r.tokens_used, r.tokens_total, r.pct_shown), (173_884, 1_000_000, 17));
        let _ = std::fs::remove_dir_all(claude.root());
    }

    /// Without a session id there is nothing to measure: the newest
    /// transcript in the project may be any session's.
    #[test]
    fn no_session_no_reading() {
        let project = "/srv/ctx-nosession";
        let claude = claude_dir("nosession", project, &[("other", turn("claude-opus-5-5", 600_000))]);
        assert!(ContextSensor::in_dir(None, claude.clone()).read_context(&focus(project)).is_none());
        let _ = std::fs::remove_dir_all(claude.root());
    }

    /// Rewrite session `sid`'s transcript so it reads `tokens` of a 1M window.
    fn set_fill(claude: &claude_sessions::ClaudeDir, project: &str, sid: &str, tokens: u64) {
        let dir = claude.projects_dir().join(claude_sessions::project_slug(project));
        std::fs::write(dir.join(format!("{sid}.jsonl")), turn("claude-opus-5-5", tokens)).unwrap();
    }

    #[test]
    fn the_band_wording_fits_the_percent() {
        assert!(band_for(17).is_none());
        assert!(band_for(49).is_none());
        assert_eq!(band_for(50).unwrap().at, 50);
        assert_eq!(band_for(69).unwrap().at, 65);
        assert_eq!(band_for(97).unwrap().at, 90);
        assert!(!band_for(69).unwrap().says.contains("half"));
    }

    /// A host that starts late, at 78%, is past the todos capture at 75%:
    /// its band must not announce that capture as coming.
    #[test]
    fn at_78_percent_the_todos_capture_is_not_ahead() {
        let says = band_for(78).unwrap().says;
        assert!(!says.contains("will capture todos"), "{says}");
        assert!(!says.contains("shortly"), "{says}");
    }

    /// At 87% the compaction checkpoint at 85% has fired: the band must not
    /// say it fires at 85%.
    #[test]
    fn at_87_percent_the_checkpoint_is_not_ahead() {
        let says = band_for(87).unwrap().says;
        assert!(!says.contains("fires at 85%"), "{says}");
    }

    /// `/compact` keeps the session id. After a compaction the bands sound
    /// again as the fill climbs back: 84%, compact to 12%, refill to 84%.
    #[test]
    fn the_bands_rearm_after_a_compaction() {
        let project = "/srv/ctx-rearm";
        let claude = claude_dir("rearm", project, &[("own", turn("claude-opus-5-5", 840_000))]);
        let mut sensor = ContextSensor::in_dir(Some("own"), claude.clone());
        assert!(sensor.poll(&focus(project)).is_empty(), "baseline");
        assert_eq!(sensor.poll(&focus(project)).len(), 1);
        set_fill(&claude, project, "own", 120_000);
        assert!(sensor.poll(&focus(project)).is_empty());
        set_fill(&claude, project, "own", 840_000);
        let said = sensor.poll(&focus(project));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].1.starts_with("context at 84%"), "{}", said[0].1);
        let _ = std::fs::remove_dir_all(claude.root());
    }

    /// A restarted host restores its spent bands; a session compacted while
    /// the host was down gets them back on its first reading.
    #[test]
    fn restored_bands_above_the_fill_rearm() {
        let project = "/srv/ctx-restore";
        let claude = claude_dir("restore", project, &[("own", turn("claude-opus-5-5", 120_000))]);
        let mut sensor = ContextSensor::in_dir(Some("own"), claude.clone());
        let spent: Vec<(String, String)> =
            ["50", "65", "75", "80"].iter().map(|t| ("disclosed_threshold".to_string(), t.to_string())).collect();
        sensor.import_state(&spent);
        assert!(sensor.poll(&focus(project)).is_empty(), "baseline");
        set_fill(&claude, project, "own", 660_000);
        let said = sensor.poll(&focus(project));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].1.starts_with("context at 66%"), "{}", said[0].1);
        let _ = std::fs::remove_dir_all(claude.root());
    }

    /// A fill that wavers a point under a band's start does not say the
    /// band again.
    #[test]
    fn a_small_dip_does_not_rearm_a_band() {
        let project = "/srv/ctx-dip";
        let claude = claude_dir("dip", project, &[("own", turn("claude-opus-5-5", 660_000))]);
        let mut sensor = ContextSensor::in_dir(Some("own"), claude.clone());
        assert!(sensor.poll(&focus(project)).is_empty(), "baseline");
        assert_eq!(sensor.poll(&focus(project)).len(), 1);
        set_fill(&claude, project, "own", 640_000);
        assert!(sensor.poll(&focus(project)).is_empty());
        set_fill(&claude, project, "own", 660_000);
        assert!(sensor.poll(&focus(project)).is_empty());
        let _ = std::fs::remove_dir_all(claude.root());
    }

    /// A first report at 69% says the 65% band alone, and the 50% band
    /// never follows it.
    #[test]
    fn a_reading_past_two_bands_says_the_higher_one_only() {
        let project = "/srv/ctx-bands";
        let claude = claude_dir("bands", project, &[("own", turn("claude-opus-5-5", 690_000))]);
        let mut sensor = ContextSensor::in_dir(Some("own"), claude.clone());
        assert!(sensor.poll(&focus(project)).is_empty(), "the first poll is the baseline");
        let said = sensor.poll(&focus(project));
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].1.starts_with("context at 69% — agent-ways will capture todos"), "{}", said[0].1);
        assert!(sensor.poll(&focus(project)).is_empty());
        let _ = std::fs::remove_dir_all(claude.root());
    }
}
