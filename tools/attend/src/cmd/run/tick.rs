// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-FileCopyrightText: 2026 Jonathan Muse
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Per-tick body, plus the helpers it uses.
//!
//! Extracted from the sensor host's outer loop so the body is testable
//! against synthetic state without standing up the host (sensor
//! registration, instance registry, the signal writer).
//!
//! - `tick_iteration` — sensor poll, governor decision, periodic
//!   checkpoint, instance-registry touch, cleanup sweep. What the
//!   governor lets through goes to a [`Sink`], the signal store.
//! - `build_engagement` — pure config → curve translation.
//! - `binary_changed` and `reexec` — the re-exec on a changed binary.
//! - `collect_snapshot` — sensor state → on-disk checkpoint shape.

use std::collections::BinaryHeap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use super::governor::{DisclosureGovernor, ScheduledSensor};
use super::sink::{Delivery, Disclosure, Sink};
use crate::cmd::cleanup::run_cleanup;
use crate::sensors::{Focus, SensorSlot};
use crate::util::signals_base;
use crate::{config, emit, sensors, state};

pub(super) const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(30);
pub(super) const INSTANCE_TOUCH_INTERVAL: Duration = Duration::from_secs(60);

/// Per-tick mutable state, bundled so [`tick_iteration`] doesn't need
/// a 10-argument signature. The lifetime ties every borrow to a
/// single tick — once the tick returns, the references die before
/// the host's loop continues into its sleep.
pub(super) struct TickState<'a> {
    pub(super) slots: &'a mut Vec<SensorSlot>,
    pub(super) queue: &'a mut BinaryHeap<ScheduledSensor>,
    pub(super) governor: &'a mut DisclosureGovernor,
    /// Permissive governor for the message lane (a sensor whose file
    /// declares `lane: message`). The
    /// event lane keeps `governor`; messages must not be starved by it.
    pub(super) msg_governor: &'a mut DisclosureGovernor,
    pub(super) last_checkpoint: &'a mut Instant,
    /// Whether the peers sensor has checkpointed at least once this
    /// process — the first poll persists its cold-start baseline even
    /// when nothing changed (ADR-172; PR #385 review).
    pub(super) peers_checkpointed_once: &'a mut bool,
    pub(super) last_instance_touch: &'a mut Instant,
    pub(super) last_cleanup: &'a mut Option<Instant>,
    pub(super) state_store: &'a state::StateStore,
    pub(super) instance_registry: &'a attend_instances::Registry,
    pub(super) focus: &'a Focus,
    pub(super) heartbeat_id: &'a str,
    pub(super) cfg: &'a config::Config,
    /// Where disclosed lines go.
    pub(super) sink: &'a mut dyn Sink,
}

/// Build the engagement curve from config (ADR-123 action-potential
/// curve on the unified progression axis). All sensors share these
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

/// Hash a file's contents (FNV-1a, stable across Rust releases). Returns
/// `None` if the file can't be read. Used as the binary-identity check on
/// the self-reload path, only invoked when the mtime has already moved.
fn hash_file(path: &Path) -> Option<u64> {
    Some(agent_identity::identity::fnv1a_64(&std::fs::read(path).ok()?))
}

/// Capture the running binary's content hash at startup, paired with its
/// mtime, so the reload check can tell a real binary change from an
/// identical rebuild that only bumped the mtime. `None` if the exe path
/// or its bytes can't be read.
pub(super) fn initial_self_hash(self_exe: Option<&Path>) -> Option<u64> {
    self_exe.and_then(hash_file)
}

/// Whether the running binary's bytes changed since startup. An identical
/// rebuild, which moves the mtime and not the bytes, advances the baseline
/// mtime and counts as no change.
pub(super) fn binary_changed(self_exe: Option<&Path>, baseline_mtime: &mut Option<SystemTime>, baseline_hash: Option<u64>) -> bool {
    let Some(exe) = self_exe else {
        return false;
    };
    let Some(orig_mtime) = *baseline_mtime else {
        return false;
    };
    let Some(current_mtime) = std::fs::metadata(exe).ok().and_then(|m| m.modified().ok()) else {
        return false;
    };
    if current_mtime == orig_mtime {
        return false;
    }
    // mtime moved — confirm the bytes actually changed before reloading.
    // A matching hash means an identical rebuild: skip the pointless
    // exec() and the reheat noise, but advance the baseline mtime so we
    // don't rehash on every subsequent interval. A failed hash falls
    // through to reload (fail safe — behave as the pre-#140 mtime gate).
    let current_hash = hash_file(exe);
    if current_hash.is_some() && current_hash == baseline_hash {
        *baseline_mtime = Some(current_mtime);
        return false;
    }
    true
}

/// Re-execute this process with `var=value` set, so the new process can
/// say why it started. Replaces the process on Unix; spawns and exits
/// on Windows. Returns only on failure, which is logged.
pub(super) fn reexec(var: &str, value: &str) {
    // An exec runs no destructor, so the MCP servers end here.
    crate::sensors::end_mcp_servers();
    let args: Vec<String> = std::env::args().collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&args[0]).args(&args[1..]).env(var, value).exec();
        // exec() only returns on failure
        emit::log(&format!("re-exec failed: {}", err));
    }
    #[cfg(not(unix))]
    match std::process::Command::new(&args[0]).args(&args[1..]).env(var, value).spawn() {
        Ok(_) => std::process::exit(0),
        Err(err) => emit::log(&format!("re-exec failed: {}", err)),
    }
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

        // ADR-172 Decision 3: the drain is a second writer to the shared
        // seen-set. Re-import drain-recorded consumption from disk before
        // the peers sensor scans, so a message already delivered at a
        // turn boundary is not re-emitted by the poller. The remaining
        // race (a drain landing between this read and the scan) costs at
        // worst a duplicate notification — at-least-once, never loss.
        if s.slots[i].is_crate("peers") {
            if let Some(snap) = s.state_store.load() {
                let marks: Vec<(String, String)> = snap
                    .seen_signals
                    .into_iter()
                    .map(|k| ("seen_signal".to_string(), k))
                    .chain(snap.baselined.then(|| ("baselined".to_string(), "true".to_string())))
                    .collect();
                if !marks.is_empty() {
                    s.slots[i].import_state(&marks);
                }
            }
        }

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

        // The message lane bypasses the action-potential
        // refractory (ADR-136 Decision 1): an authored message is not
        // observation noise, so it must surface whenever there is
        // anything accumulated, never held by a refractory built for
        // throttling git/process churn. It still rides the permissive
        // message governor downstream. The event lane keeps the
        // refractory gate unchanged.
        let is_message_lane = s.slots[i].message_lane;

        // ADR-172 Decision 3, the other direction: the sensor marks
        // scanned signals seen in memory, but the drain reads the FILE
        // at the next turn boundary — an interval-deferred checkpoint
        // would let the drain re-deliver what the poller just surfaced.
        // Checkpoint immediately on any peers change so on-disk
        // consumption keeps pace with both writers. Union semantics
        // make the extra write safe; message traffic makes it rare.
        // The first peers poll checkpoints even when quiet: it may have
        // just baselined the cold-start backlog in memory, and until
        // that reaches disk the drain would treat the session as cold
        // and baseline again (PR #385 review, blocking finding).
        if is_message_lane && (changed || !*s.peers_checkpointed_once) {
            let snapshot = collect_snapshot(s.slots);
            s.state_store.checkpoint(&snapshot);
            *s.last_checkpoint = Instant::now();
            *s.peers_checkpointed_once = true;
        }

        if (is_message_lane && s.slots[i].accumulator.magnitude > 0.0)
            || s.slots[i].ready_to_disclose()
        {
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

    // Batch disclosure, split by lane (ADR-136 Decision 1). The event
    // lane (git/process) keeps the strict, rate-ballooning governor that
    // suppresses observation noise. The message lane, which a sensor's
    // file declares (ADR-403 §3a), rides a permissive governor: authored
    // messages flow at normal cadence and a burst — already coalesced into
    // one digest by sensor-peers — discloses promptly instead of being
    // starved.
    // KNOWN LIMITATION (#139): the lane is chosen per
    // *sensor*, but the `peers` sensor emits BOTH authored messages and
    // peer-presence *events* (peer started/exited, status). Today all of
    // it rides the message lane, so peer-presence skips the refractory and
    // neuron-decay the event lane gives git/process. No message loss —
    // only presence-noise control is relaxed. The clean fix is to split
    // message scanning into its own sensor so the lane is chosen per
    // observation (also the seam a future Slack event-sensor wants).
    let (msg_ready, evt_ready): (Vec<usize>, Vec<usize>) = ready_indices
        .iter()
        .partition(|&&i| s.slots[i].message_lane);
    let mut spent = try_disclose(s.slots, &evt_ready, s.governor, true, s.sink);
    // Message lane: do not record engagement, so it never builds the
    // action-potential refractory that would later hold conversation.
    spent |= try_disclose(s.slots, &msg_ready, s.msg_governor, false, s.sink);

    // A trigger kept for the session is spent once its lines were written or
    // cut. The checkpoint follows at once, so a restart does not fire it
    // again; a kill between the write and this checkpoint can, and a
    // duplicate notice is the lesser loss.
    if spent {
        s.state_store.checkpoint(&collect_snapshot(s.slots));
        *s.last_checkpoint = Instant::now();
        emit::log("a session trigger is spent; checkpointed");
    }

    // Periodic checkpoint
    if s.last_checkpoint.elapsed() >= CHECKPOINT_INTERVAL {
        let snapshot = collect_snapshot(s.slots);
        s.state_store.checkpoint(&snapshot);
        *s.last_checkpoint = Instant::now();
    }

    // Periodic instance-registry refresh — `last_seen` moves so the GC
    // clock cannot expire an active session, and a missing entry is
    // allocated again (issue #378), so the roster heals within one
    // interval. An alias row stays an alias: after `/clear` the new host
    // renamed this id's slot, and this host, still running until the mod
    // ends it, must not take the old id a fresh slot.
    if s.last_instance_touch.elapsed() >= INSTANCE_TOUCH_INTERVAL {
        s.instance_registry.keep(&s.focus.working_dir, s.heartbeat_id).ok();
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
            let stats = run_cleanup(&base, false, false);
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

/// Disclose a batch of ready sensors through `governor`, or log a hold if
/// the governor is closed. Each line that went out is reported to its
/// sensor ([`sensor_trait::Sensor::delivered`]); returns whether that spent
/// a trigger kept for the session. Shared by the event lane and the (permissive)
/// message lane so both get identical batching, priority, and engagement
/// bookkeeping — only the governing policy differs.
fn try_disclose(
    slots: &mut [SensorSlot],
    ready: &[usize],
    governor: &mut DisclosureGovernor,
    record_engagement: bool,
    sink: &mut dyn Sink,
) -> bool {
    if ready.is_empty() {
        return false;
    }
    if !governor.can_disclose() {
        emit::log(&format!(
            "{} sensors ready but governor holding ({}/{} in window)",
            ready.len(),
            governor.window_disclosures,
            governor.max_disclosures_per_window,
        ));
        return false;
    }

    let mut batch = Vec::new();
    for &i in ready {
        let slot = &slots[i];
        let lines = slot.accumulator.drain_events().into_iter().map(|l| {
            let lapse = slot.accumulator.lapse_of(&l);
            (l, lapse)
        });
        batch.push(Disclosure {
            sensor: slot.name().to_string(),
            magnitude: slot.accumulator.magnitude,
            message_lane: slot.message_lane,
            lines: lines.collect(),
        });
    }

    emit::log(&format!(
        "disclosing batch of {} sensors (cooldown was {:.1}s)",
        batch.len(),
        governor.cooldown().as_secs_f64(),
    ));
    let verdicts = sink.deliver(&batch);
    let mut spent = false;
    for ((&i, d), verdicts) in ready.iter().zip(&batch).zip(&verdicts) {
        // A failed line releases its trigger first, so a trigger with one
        // line written and another failed is not spent.
        for ((line, _), v) in d.lines.iter().zip(verdicts) {
            if *v == Delivery::Failed {
                slots[i].sensor.dropped(line);
            }
        }
        for ((line, _), v) in d.lines.iter().zip(verdicts) {
            if *v != Delivery::Failed {
                slots[i].sensor.delivered(line);
            }
        }
        spent |= slots[i].sensor.take_spent();
    }
    if verdicts.iter().flatten().any(|v| *v == Delivery::Written) {
        governor.record_disclosure();
        // Record engagement only for the event lane (refractory is its
        // noise-control). The message lane skips it so conversation never
        // builds a refractory that would later hold it.
        if record_engagement {
            let tick = sensor_trait::epoch_secs();
            for &i in ready {
                if slots[i].accumulator.magnitude >= 3.0 {
                    slots[i].engagement.record_fire(tick, 1.0);
                }
            }
        }
    }

    for &i in ready {
        slots[i].accumulator.reset();
    }
    spent
}

/// The state a restarted run hands the sensor named `name`, running the
/// compiled sensor `crate_id` when it is one, from the session's
/// checkpoint: the reverse of [`collect_snapshot`]. The peers sensor gets
/// its seen-set and cold-start mark, the context sensor its disclosed
/// thresholds, and a file sensor its session memory, so a trigger that
/// fired before a re-exec stays spent for the rest of the session.
pub(super) fn restored_state(name: &str, crate_id: Option<&str>, snapshot: &state::StateSnapshot) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = match crate_id.unwrap_or_default() {
        "peers" => snapshot
            .seen_signals
            .iter()
            .map(|s| ("seen_signal".to_string(), s.clone()))
            .chain(std::iter::once(("reply_hint_shown".to_string(), snapshot.reply_hint_shown.to_string())))
            .chain(snapshot.baselined.then(|| ("baselined".to_string(), "true".to_string())))
            .collect(),
        "context" => snapshot.disclosed_thresholds.iter().map(|t| ("disclosed_threshold".to_string(), t.to_string())).collect(),
        _ => Vec::new(),
    };
    if let Some(memo) = snapshot.sensor_memory.get(name) {
        out.push((sensors::hosted::SESSION_MEMORY.to_string(), memo.clone()));
    }
    out
}

pub(super) fn collect_snapshot(slots: &[sensors::SensorSlot]) -> state::StateSnapshot {
    let mut snapshot = state::StateSnapshot::default();
    for slot in slots {
        let peers = slot.is_crate("peers");
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
                crate::sensors::hosted::SESSION_MEMORY => {
                    snapshot.sensor_memory.insert(slot.name().to_string(), value);
                }
                // The message lane's cold-start mark: only the peers sensor
                // applies the rule.
                "baselined" if peers => {
                    snapshot.baselined |= value == "true";
                }
                _ => {}
            }
        }
    }
    snapshot
}

#[cfg(test)]
mod tests;
