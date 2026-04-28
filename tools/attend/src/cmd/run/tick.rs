//! Per-tick body, plus the helpers it uses.
//!
//! Extracted from the outer `cmd_run_with_catchup` loop so the body
//! is testable against synthetic state without standing up the full
//! shell (sensor registration, instance registry, signal lock, etc).
//!
//! - `tick_iteration` — sensor poll, governor decision, periodic
//!   checkpoint, instance-registry touch, cleanup sweep.
//! - `build_engagement` — pure config → curve translation.
//! - `maybe_self_reload` — checkpoint + `exec()` on binary mtime change.
//! - `collect_snapshot` — sensor state → on-disk checkpoint shape.

use std::collections::BinaryHeap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use super::governor::{DisclosureGovernor, ScheduledSensor};
use crate::cmd::cleanup::run_cleanup;
use crate::sensors::{Focus, SensorSlot};
use crate::util::signals_base;
use crate::{config, emit, sensors, state};

pub(super) const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(30);
pub(super) const INSTANCE_TOUCH_INTERVAL: Duration = Duration::from_secs(60);

/// Per-tick mutable state, bundled so [`tick_iteration`] doesn't need
/// a 10-argument signature. The lifetime ties every borrow to a
/// single tick — once the tick returns, the references die before
/// `cmd_run_with_catchup` continues into the sleep/heartbeat path.
pub(super) struct TickState<'a> {
    pub(super) slots: &'a mut Vec<SensorSlot>,
    pub(super) queue: &'a mut BinaryHeap<ScheduledSensor>,
    pub(super) governor: &'a mut DisclosureGovernor,
    pub(super) last_checkpoint: &'a mut Instant,
    pub(super) last_instance_touch: &'a mut Instant,
    pub(super) last_cleanup: &'a mut Option<Instant>,
    pub(super) state_store: &'a state::StateStore,
    pub(super) instance_registry: &'a attend_instances::Registry,
    pub(super) focus: &'a Focus,
    pub(super) heartbeat_id: &'a str,
    pub(super) cfg: &'a config::Config,
}

/// Build the engagement curve from config (ADR-119 action potential,
/// ADR-123 progression-axis unification). All sensors share these
/// engagement parameters; per-sensor overrides can be added later
/// if the defaults turn out to be too coarse.
///
/// The attend tick is wall-clock seconds, so Curve parameters are in
/// seconds. The old linear `decay_per_minute` rate is converted to an
/// exponential half-life via `rate_per_min_to_half_life_secs` — see
/// ADR-123 Phase B worksheet for the caveat.
///
/// `peak_multiplier` is `1 + step_multiplier`, which reproduces the
/// old "peak at exactly burst_threshold" value (2.25 at defaults).
/// The old model's additional scaling for fires past threshold is
/// not preserved — that scaling rarely activated in practice and the
/// refactor opts for a flat ceiling.
pub(super) fn build_engagement(cfg: &config::Config) -> sensor_trait::Curve {
    let multiplier_half_life = sensor_trait::engagement::rate_per_min_to_half_life_secs(
        cfg.engagement.decay_per_minute,
    );
    // Surprise guard: very high decay_per_minute values produce sub-
    // minute half-lives that rarely match operator intent. Warn rather
    // than clamp so the operator keeps authority over their config,
    // but make the effective value visible instead of letting it
    // surprise them later.
    if cfg.engagement.decay_per_minute > 0.5 && multiplier_half_life < 60 {
        eprintln!(
            "[attend] note: engagement.decay_per_minute={:.3} → multiplier_half_life≈{}s (aggressive decay; adjust in attend config if unintended)",
            cfg.engagement.decay_per_minute, multiplier_half_life,
        );
    }
    sensor_trait::Curve::ActionPotential {
        burst_threshold: cfg.engagement.burst_threshold,
        peak_multiplier: 1.0 + cfg.engagement.step_multiplier,
        absolute_refractory: cfg.engagement.absolute_refractory.as_secs(),
        multiplier_half_life,
    }
}

/// Compare the running binary's mtime against the captured startup
/// mtime. On change, checkpoint state and `exec()` self so the new
/// binary takes over with state preserved. Never returns on success;
/// `exec()` only returns on failure, which is logged and discarded so
/// the loop continues against the now-stale binary.
pub(super) fn maybe_self_reload(
    self_exe: Option<&Path>,
    initial_mtime: Option<&SystemTime>,
    slots: &[SensorSlot],
    state_store: &state::StateStore,
) {
    let (Some(exe), Some(orig_mtime)) = (self_exe, initial_mtime) else {
        return;
    };
    let Ok(meta) = std::fs::metadata(exe) else {
        return;
    };
    let Ok(current_mtime) = meta.modified() else {
        return;
    };
    if current_mtime == *orig_mtime {
        return;
    }
    emit::log("binary changed — checkpointing and reloading");
    let snapshot = collect_snapshot(slots);
    state_store.checkpoint(&snapshot);

    use std::io::Write;
    std::io::stdout().flush().ok();

    // Tag the new process with the version we are reloading from so
    // its startup banner can name it as a hot-swap, not a silent
    // restart. Any value works; format mirrors the banner string.
    let prev_version = format!(
        "v{} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("ATTEND_COMMIT")
    );

    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().collect();
    let err = std::process::Command::new(&args[0])
        .args(&args[1..])
        .env("ATTEND_RELOADED_FROM", &prev_version)
        .exec();
    // exec() only returns on failure
    emit::log(&format!("self-reload failed: {}", err));
}

/// One iteration of the sensor loop: drain ready sensors, decide
/// disclosures, run periodic checkpoints / instance touch / cleanup
/// sweep. The outer loop owns the sleep, heartbeat touch, and self-
/// reload check; everything bounded by sensor scheduling lives here.
pub(super) fn tick_iteration(s: &mut TickState) {
    let mut ready_indices = Vec::new();

    while let Some(scheduled) = s.queue.peek() {
        if scheduled.fire_at > Instant::now() {
            break;
        }
        let scheduled = s.queue.pop().unwrap();
        let i = scheduled.index;

        let changed = s.slots[i].poll(s.focus);

        // Only log when something changed — quiet polls are silent.
        if changed {
            s.governor.record_event();
            let refractory = s.slots[i]
                .effective_threshold()
                .map(|t| format!("threshold={:.1}", t))
                .unwrap_or_else(|| "ABSOLUTE REFRACTORY".to_string());
            emit::log(&format!(
                "{}: change detected (interval={:.1}s, accum={:.1}, events={}, {})",
                s.slots[i].name(),
                s.slots[i].interval.current.as_secs_f64(),
                s.slots[i].accumulator.magnitude,
                s.slots[i].accumulator.event_count,
                refractory,
            ));
        }

        if s.slots[i].ready_to_disclose() {
            ready_indices.push(i);
        } else if s.slots[i].accumulator.magnitude > 0.0 && changed {
            // Accumulated but blocked by refractory — log it so we
            // can see when action potential is holding the line.
            if s.slots[i].effective_threshold().is_none() {
                emit::log(&format!(
                    "{}: held in absolute refractory (magnitude={:.1})",
                    s.slots[i].name(),
                    s.slots[i].accumulator.magnitude,
                ));
            }
        }

        s.slots[i].schedule_next();
        s.queue.push(ScheduledSensor {
            fire_at: s.slots[i].next_fire,
            index: i,
        });
    }

    // Batch disclosure
    if !ready_indices.is_empty() && s.governor.can_disclose() {
        let mut batch = Vec::new();
        for &i in &ready_indices {
            let slot = &s.slots[i];
            let priority = if slot.accumulator.magnitude >= 5.0 {
                "high"
            } else if slot.accumulator.magnitude >= 3.0 {
                "medium"
            } else {
                "low"
            };

            batch.push((
                slot.name().to_string(),
                priority.to_string(),
                slot.accumulator.drain_events(),
            ));
        }

        emit::log(&format!(
            "disclosing batch of {} sensors (cooldown was {:.1}s)",
            batch.len(),
            s.governor.cooldown().as_secs_f64(),
        ));
        let emitted = emit::emit_batch(&batch);
        if emitted {
            s.governor.record_disclosure();
            // Record engagement only for sensors whose events actually
            // fired (not the quiet ones that got suppressed). Action
            // potential refractory is per-sensor.
            let tick = sensor_trait::epoch_secs();
            for &i in &ready_indices {
                let was_actionable = s.slots[i].accumulator.magnitude >= 3.0;
                if was_actionable {
                    s.slots[i].engagement.record_fire(tick, 1.0);
                }
            }
        }

        for &i in &ready_indices {
            s.slots[i].accumulator.reset();
        }
    } else if !ready_indices.is_empty() {
        emit::log(&format!(
            "{} sensors ready but governor holding ({}/{} in window)",
            ready_indices.len(),
            s.governor.window_disclosures,
            s.governor.max_disclosures_per_window,
        ));
    }

    // Periodic checkpoint
    if s.last_checkpoint.elapsed() >= CHECKPOINT_INTERVAL {
        let snapshot = collect_snapshot(s.slots);
        s.state_store.checkpoint(&snapshot);
        *s.last_checkpoint = Instant::now();
    }

    // Periodic instance-registry touch — refresh `last_seen` so the
    // GC clock cannot expire an active session. Cheap when we already
    // know we have an entry; no-op when registration failed at startup.
    if s.last_instance_touch.elapsed() >= INSTANCE_TOUCH_INTERVAL {
        s.instance_registry
            .touch(&s.focus.working_dir, s.heartbeat_id)
            .ok();
        *s.last_instance_touch = Instant::now();
    }

    // Periodic cleanup sweep — remove stale signal files and empty
    // project subdirs from the signals base. Scoped strictly to
    // attend's own data (~/.cache/attend/signals/); never touches
    // ways data or anything else.
    if s.cfg.cleanup.enabled {
        let due = match *s.last_cleanup {
            None => true,
            Some(t) => t.elapsed() >= s.cfg.cleanup.interval,
        };
        if due {
            let base = signals_base();
            let stats = run_cleanup(&base, s.cfg.cleanup.retention, false, false);
            if stats.removed > 0 || stats.dirs_removed > 0 {
                emit::log(&format!(
                    "cleanup: removed {} signal(s) ({} bytes), {} empty project dir(s)",
                    stats.removed, stats.bytes, stats.dirs_removed,
                ));
            }
            *s.last_cleanup = Some(Instant::now());
        }
    }
}

pub(super) fn collect_snapshot(slots: &[sensors::SensorSlot]) -> state::StateSnapshot {
    let mut snapshot = state::StateSnapshot::default();
    for slot in slots {
        for (key, value) in slot.export_state() {
            match key.as_str() {
                "seen_signal" => {
                    snapshot.seen_signals.insert(value);
                }
                "disclosed_threshold" => {
                    if let Ok(t) = value.parse::<u8>() {
                        snapshot.disclosed_thresholds.push(t);
                    }
                }
                "reply_hint_shown" => {
                    snapshot.reply_hint_shown = value == "true";
                }
                "context_pct" => {
                    snapshot.context_pct = value.parse().ok();
                }
                _ => {}
            }
        }
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_engagement_returns_action_potential_curve() {
        // Reachable without standing up a full cmd_run shell, which
        // is the testability claim from issue #78.
        let cfg = config::Config::default();
        let curve = build_engagement(&cfg);
        match curve {
            sensor_trait::Curve::ActionPotential { peak_multiplier, .. } => {
                assert!(peak_multiplier > 1.0);
            }
            _ => panic!("expected ActionPotential curve"),
        }
    }
}
