//! `attend run` — the sensor loop, governor, and self-reload path.
//!
//! The hottest module in attend. `DisclosureGovernor` and
//! `ScheduledSensor` are private to this module because nothing else
//! touches them. `collect_snapshot` is the ser-side bridge from live
//! sensor slots to `state::StateSnapshot`; it too is private because
//! only the run loop checkpoints.

use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use crate::cmd::cleanup::run_cleanup;
use crate::sensors::Focus;
use crate::util::{own_session_id, signals_base};
use crate::{config, emit, groups, sensors, state};

// --- Disclosure governor ---

struct DisclosureGovernor {
    base_cooldown: Duration,
    last_disclosure: Option<Instant>,
    max_disclosures_per_window: u32,
    window_disclosures: u32,
    window_start: Instant,
    rate_window: Duration,
    total_events: u32,
    total_events_start: Instant,
}

impl DisclosureGovernor {
    fn new(base_cooldown: Duration, max_per_window: u32, rate_window: Duration) -> Self {
        let now = Instant::now();
        Self {
            base_cooldown,
            last_disclosure: None,
            max_disclosures_per_window: max_per_window,
            window_disclosures: 0,
            window_start: now,
            rate_window,
            total_events: 0,
            total_events_start: now,
        }
    }

    fn record_event(&mut self) {
        self.total_events += 1;
    }

    fn aggregate_rate(&self) -> f64 {
        let elapsed = self.total_events_start.elapsed().as_secs_f64();
        if elapsed < 0.001 {
            return 0.0;
        }
        self.total_events as f64 / elapsed
    }

    fn cooldown(&self) -> Duration {
        let rate = self.aggregate_rate();
        let multiplier = 1.0 + rate.sqrt() * 3.0;
        self.base_cooldown.mul_f64(multiplier)
    }

    fn can_disclose(&mut self) -> bool {
        if self.window_start.elapsed() >= self.rate_window {
            self.window_disclosures = 0;
            self.window_start = Instant::now();
        }

        if self.window_disclosures >= self.max_disclosures_per_window {
            return false;
        }

        if let Some(last) = self.last_disclosure {
            if last.elapsed() < self.cooldown() {
                return false;
            }
        }

        true
    }

    fn record_disclosure(&mut self) {
        self.last_disclosure = Some(Instant::now());
        self.window_disclosures += 1;
    }
}

// --- Priority queue entry ---

struct ScheduledSensor {
    fire_at: Instant,
    index: usize,
}

impl Eq for ScheduledSensor {}
impl PartialEq for ScheduledSensor {
    fn eq(&self, other: &Self) -> bool {
        self.fire_at == other.fire_at
    }
}
impl Ord for ScheduledSensor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.fire_at.cmp(&self.fire_at)
    }
}
impl PartialOrd for ScheduledSensor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) fn cmd_run_with_catchup(catchup: bool) {
    emit::log("starting attend");

    let focus = Focus::default_focus();
    emit::log(&format!("focus: {} ({})", focus.description, focus.working_dir));

    // Load config: user scope → project scope overlay
    let cfg = config::Config::load(&focus.working_dir);

    // Initialize rooms for signal routing (ADR-118)
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));

    // Duplicate-attend guard (ADR-129). Acquire an exclusive flock on
    // our heartbeat file so a second attend process cannot start for
    // the same session. The lock is released by the kernel on process
    // exit, so a panicking or killed attend does not need a janitor.
    //
    // Self-reload exec() keeps file descriptors and their flocks open,
    // so the post-exec process *should* already hold the lock through
    // FD inheritance. Re-acquiring with a fresh FD on that path will
    // therefore return EWOULDBLOCK — we treat that as success on the
    // reload path (gated on `ATTEND_RELOADED_FROM`).
    //
    // We still attempt the acquire on the reload path so the
    // bootstrap migration works: when an older binary that did not
    // take a lock execs into a new binary that does, the new process
    // has no inherited lock, and the attempt below cleanly grabs one.
    //
    // The lock value lives on the stack until `cmd_run` returns; the
    // sensor loop never returns under normal operation, so the lock
    // effectively lives for the life of the process.
    let reloaded = std::env::var("ATTEND_RELOADED_FROM").is_ok();
    let _session_lock = match attend_heartbeat::try_acquire_session_lock(&session_id) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) if reloaded => {
            // The old binary's lock is still held through the inherited
            // FD — that is the running attend, which is now us. No new
            // lock object to track; the pre-exec FD continues to hold
            // the kernel state until process exit.
            None
        }
        Ok(None) => {
            eprintln!(
                "[attend] another attend is already running for session {session_id}; exiting cleanly"
            );
            return;
        }
        Err(e) => {
            // Best-effort: log and proceed without a lock. A failed
            // open is rare (permissions, missing dir), and exiting
            // attend over a transient FS error would be a worse
            // failure mode than running without the duplicate guard
            // for one session.
            emit::log(&format!(
                "session lock unavailable ({e}); proceeding without duplicate-attend guard"
            ));
            None
        }
    };

    // Instance registry (ADR-129). Register or reclaim our slot in
    // this cwd. Logged at startup so the operator can correlate a
    // running attend with its discriminator suffix without reading
    // the on-disk yaml. Registration is read-modify-write under
    // flock — if another attend is racing for the same slot, CAS
    // resolves it; we always end up with a deterministic instance.
    //
    // Failure here is non-fatal — the registry file is on the same
    // ~/.cache path as everything else; if writes fail the rest of
    // attend will fail similarly. Log and continue with no
    // discriminator so renders fall back to the bare nickname.
    let instance_registry = attend_instances::Registry::new();
    let my_instance = match instance_registry.register(&focus.working_dir, &session_id) {
        Ok(s) => {
            emit::log(&format!("instance: {s} (cwd: {})", focus.working_dir));
            Some(s)
        }
        Err(e) => {
            emit::log(&format!(
                "instance registry unavailable ({e}); rendering without suffix"
            ));
            None
        }
    };
    let _ = &my_instance; // consumed by render layer once the suffix is wired in

    let group_mgr = groups::Groups::new(&signals_base(), &session_id);

    // ADR-124 one-shot: fold any lingering `@open/` group into the
    // `_broadcast/` base. Idempotent — a no-op once there's nothing
    // left to move, which is the common case after the first
    // post-upgrade startup.
    if let Some(moved) = groups::migrate_legacy_open_group(&signals_base(), &group_mgr) {
        emit::log(&format!(
            "migrated legacy @open/ → _broadcast/ ({moved} signal(s) moved)"
        ));
    }

    // Self-documenting startup
    let my_groups = group_mgr.my_groups();
    let focus_desc = if my_groups.is_empty() {
        "project only".to_string()
    } else {
        let names: Vec<&str> = my_groups.iter().map(|(n, _)| n.as_str()).collect();
        format!("project + {}", names.join(", "))
    };

    // Register sensors from config + feature flags

    let (mut slots, enabled_names) = sensors::register_sensors(&cfg, &focus, catchup, &group_mgr);

    // Apply engagement config (ADR-119 action potential, ADR-123
    // progression-axis unification) to every slot. All sensors share the
    // same engagement parameters; per-sensor overrides can be added later
    // if the defaults turn out to be too coarse.
    //
    // The attend tick is wall-clock seconds (sensor_trait::epoch_secs),
    // so Curve parameters are in seconds. The old linear
    // `decay_per_minute` rate is converted to an exponential half-life
    // via `rate_per_min_to_half_life_secs` — see ADR-123 Phase B
    // worksheet for the caveat.
    //
    // `peak_multiplier` is `1 + step_multiplier`, which reproduces the
    // old "peak at exactly burst_threshold" value (2.25 at defaults).
    // The old model's additional scaling for fires past threshold is
    // not preserved — that scaling rarely activated in practice and the
    // refactor opts for a flat ceiling.
    let multiplier_half_life = sensor_trait::engagement::rate_per_min_to_half_life_secs(
        cfg.engagement.decay_per_minute,
    );
    // Surprise guard: very high decay_per_minute values produce sub-minute
    // half-lives that rarely match operator intent. Warn rather than clamp
    // so the operator keeps authority over their config, but make the
    // effective value visible instead of letting it surprise them later.
    if cfg.engagement.decay_per_minute > 0.5 && multiplier_half_life < 60 {
        eprintln!(
            "[attend] note: engagement.decay_per_minute={:.3} → multiplier_half_life≈{}s (aggressive decay; adjust in attend config if unintended)",
            cfg.engagement.decay_per_minute, multiplier_half_life,
        );
    }
    let engagement_curve = sensor_trait::Curve::ActionPotential {
        burst_threshold: cfg.engagement.burst_threshold,
        peak_multiplier: 1.0 + cfg.engagement.step_multiplier,
        absolute_refractory: cfg.engagement.absolute_refractory.as_secs(),
        multiplier_half_life,
    };
    for slot in &mut slots {
        slot.engagement = sensor_trait::EngagementState::new(engagement_curve.clone());
    }

    // State persistence
    let session_id = own_session_id();
    let state_store = state::StateStore::new(session_id);

    // Try to restore state from previous run
    if let Some(snapshot) = state_store.restore() {
        // Distribute state to matching sensors
        for slot in &mut slots {
            let sensor_state: Vec<(String, String)> = match slot.name() {
                "peers" => snapshot
                    .seen_signals
                    .iter()
                    .map(|s| ("seen_signal".to_string(), s.clone()))
                    .chain(std::iter::once((
                        "reply_hint_shown".to_string(),
                        snapshot.reply_hint_shown.to_string(),
                    )))
                    .collect(),
                "context" => snapshot
                    .disclosed_thresholds
                    .iter()
                    .map(|t| ("disclosed_threshold".to_string(), t.to_string()))
                    .collect(),
                _ => Vec::new(),
            };
            if !sensor_state.is_empty() {
                slot.import_state(&sensor_state);
            }
        }
    }

    let sensor_list = enabled_names.join(", ");
    let banner_fingerprint = format!(
        "v{}:{}:{}:{}",
        env!("CARGO_PKG_VERSION"),
        env!("ATTEND_COMMIT"),
        sensor_list,
        focus_desc
    );

    // Suppress repeated startup banners — only emit full banner when config
    // changes. The `ATTEND_RELOADED_FROM` env var is set by the self-reload
    // exec() path below; when present, this process is the post-exec child
    // of a binary-change reload, so the banner explicitly names it as a
    // hot-swap rather than a silent "unchanged" restart. The env var is
    // consumed (removed) to keep it from leaking into any subprocesses
    // attend itself spawns.
    let reloaded_from = std::env::var("ATTEND_RELOADED_FROM").ok();
    std::env::remove_var("ATTEND_RELOADED_FROM");
    let stamp_path = signals_base().join("_last_banner");
    let prev_fingerprint = std::fs::read_to_string(&stamp_path).unwrap_or_default();
    if let Some(prev_version) = reloaded_from {
        // Always emit on a binary hot-swap — the operator needs to know
        // the running code changed under them, even when sensors/focus
        // are identical.
        println!(
            "[attend] reloaded {} → v{} ({}) — binary updated, state preserved across exec()",
            prev_version,
            env!("CARGO_PKG_VERSION"),
            env!("ATTEND_COMMIT")
        );
        std::fs::write(&stamp_path, &banner_fingerprint).ok();
    } else if banner_fingerprint == prev_fingerprint.trim() {
        println!("[attend] restarted (unchanged)");
    } else {
        println!(
            "[attend] v{} ({}) — sensors: {} | focus: {} | send: attend send <msg> (#open)",
            env!("CARGO_PKG_VERSION"),
            env!("ATTEND_COMMIT"),
            sensor_list,
            focus_desc
        );
        std::fs::write(&stamp_path, &banner_fingerprint).ok();
    }

    let mut governor = DisclosureGovernor::new(
        cfg.governor.base_cooldown,
        cfg.governor.max_per_window,
        cfg.governor.rate_window,
    );

    let mut queue: BinaryHeap<ScheduledSensor> = BinaryHeap::new();
    for (i, slot) in slots.iter().enumerate() {
        queue.push(ScheduledSensor {
            fire_at: slot.next_fire,
            index: i,
        });
    }

    // Checkpoint timer — save state every 30s
    let mut last_checkpoint = Instant::now();
    let checkpoint_interval = Duration::from_secs(30);

    // Instance registry refresh (ADR-129). Touches our row's
    // `last_seen` so the 7-day GC clock cannot expire an active
    // session. Touch is a small flock-protected read-modify-write,
    // and last_seen is only used for GC, so once-per-minute is more
    // than enough headroom against the day-scale grace window.
    let mut last_instance_touch = Instant::now();
    let instance_touch_interval = Duration::from_secs(60);

    // Auto-cleanup timer — prune stale signal files and empty project dirs.
    // Default 30-day retention + 10-minute sweep interval (see CleanupConfig).
    // Fire a first sweep on startup so long-running instances don't wait
    // a full interval before the first prune.
    let mut last_cleanup: Option<Instant> = None;
    let cleanup_enabled = cfg.cleanup.enabled;
    let cleanup_interval = cfg.cleanup.interval;
    let cleanup_retention = cfg.cleanup.retention;

    emit::log(&format!(
        "tick loop running — {} sensors registered",
        slots.len()
    ));
    for slot in &slots {
        emit::log(&format!(
            "  {} (base={:.0}s, min={:.0}s, threshold={:.1})",
            slot.name(),
            slot.sensor.base_interval().as_secs_f64(),
            slot.sensor.min_interval().as_secs_f64(),
            slot.sensor.emission_threshold(),
        ));
    }

    // Self-reload: track own binary mtime
    let self_exe = std::env::current_exe().ok();
    let initial_mtime = self_exe
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let mut last_reload_check = Instant::now();
    let reload_check_interval = Duration::from_secs(10);

    // Heartbeat identifier (ADR-129). Real session id when resolvable,
    // otherwise a pid-based fallback so a heartbeat exists even when
    // session resolution is racing claude's startup. Re-derived here
    // because the canonical `session_id` was shadowed by `state_store`'s
    // Option<String> form.
    let heartbeat_id =
        own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));

    loop {
        // Heartbeat — touched at the top of every tick so a single
        // skipped poll cannot evict this session from peer liveness
        // checks (groups::session_alive, attend-chat known_identities
        // filter). Best-effort: a missing write is recoverable on the
        // next iteration.
        attend_heartbeat::touch(&heartbeat_id).ok();

        // Check for binary change
        if last_reload_check.elapsed() >= reload_check_interval {
            if let (Some(ref exe), Some(ref orig_mtime)) = (&self_exe, &initial_mtime) {
                if let Ok(meta) = std::fs::metadata(exe) {
                    if let Ok(current_mtime) = meta.modified() {
                        if current_mtime != *orig_mtime {
                            // Binary changed — checkpoint and exec self
                            emit::log("binary changed — checkpointing and reloading");
                            let snapshot = collect_snapshot(&slots);
                            state_store.checkpoint(&snapshot);

                            // Flush stdout before exec to avoid losing buffered output
                            use std::io::Write;
                            std::io::stdout().flush().ok();

                            // Tag the new process with the version we are
                            // reloading from so its startup banner can name
                            // it as a hot-swap, not a silent restart. Any
                            // value works; format mirrors the banner string.
                            let prev_version = format!(
                                "v{} ({})",
                                env!("CARGO_PKG_VERSION"),
                                env!("ATTEND_COMMIT")
                            );

                            // exec self via std::os::unix
                            use std::os::unix::process::CommandExt;
                            let args: Vec<String> = std::env::args().collect();
                            let err = std::process::Command::new(&args[0])
                                .args(&args[1..])
                                .env("ATTEND_RELOADED_FROM", &prev_version)
                                .exec();
                            // exec() only returns on failure
                            emit::log(&format!("self-reload failed: {}", err));
                        }
                    }
                }
            }
            last_reload_check = Instant::now();
        }

        let next = match queue.peek() {
            Some(s) => s.fire_at,
            None => break,
        };

        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }

        let mut ready_indices = Vec::new();

        while let Some(scheduled) = queue.peek() {
            if scheduled.fire_at > Instant::now() {
                break;
            }
            let scheduled = queue.pop().unwrap();
            let i = scheduled.index;

            let changed = slots[i].poll(&focus);

            if changed {
                governor.record_event();
            }

            // Only log when something changed — quiet polls are silent
            if changed {
                let refractory = slots[i]
                    .effective_threshold()
                    .map(|t| format!("threshold={:.1}", t))
                    .unwrap_or_else(|| "ABSOLUTE REFRACTORY".to_string());
                emit::log(&format!(
                    "{}: change detected (interval={:.1}s, accum={:.1}, events={}, {})",
                    slots[i].name(),
                    slots[i].interval.current.as_secs_f64(),
                    slots[i].accumulator.magnitude,
                    slots[i].accumulator.event_count,
                    refractory,
                ));
            }

            if slots[i].ready_to_disclose() {
                ready_indices.push(i);
            } else if slots[i].accumulator.magnitude > 0.0 && changed {
                // Accumulated but blocked by refractory — log it so we can
                // see when action potential is holding the line.
                if slots[i].effective_threshold().is_none() {
                    emit::log(&format!(
                        "{}: held in absolute refractory (magnitude={:.1})",
                        slots[i].name(),
                        slots[i].accumulator.magnitude,
                    ));
                }
            }

            slots[i].schedule_next();
            queue.push(ScheduledSensor {
                fire_at: slots[i].next_fire,
                index: i,
            });
        }

        // Batch disclosure
        if !ready_indices.is_empty() && governor.can_disclose() {
            let mut batch = Vec::new();

            for &i in &ready_indices {
                let slot = &slots[i];
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
                governor.cooldown().as_secs_f64(),
            ));
            let emitted = emit::emit_batch(&batch);
            if emitted {
                governor.record_disclosure();
                // Record engagement only for sensors whose events actually
                // fired (not the quiet ones that got suppressed). Action
                // potential refractory is per-sensor.
                let tick = sensor_trait::epoch_secs();
                for &i in &ready_indices {
                    let slot = &slots[i];
                    let was_actionable = slot.accumulator.magnitude >= 3.0;
                    if was_actionable {
                        slots[i].engagement.record_fire(tick, 1.0);
                    }
                }
            }

            for &i in &ready_indices {
                slots[i].accumulator.reset();
            }
        } else if !ready_indices.is_empty() {
            emit::log(&format!(
                "{} sensors ready but governor holding ({}/{} in window)",
                ready_indices.len(),
                governor.window_disclosures,
                governor.max_disclosures_per_window,
            ));
        }

        // Periodic checkpoint
        if last_checkpoint.elapsed() >= checkpoint_interval {
            let snapshot = collect_snapshot(&slots);
            state_store.checkpoint(&snapshot);
            last_checkpoint = Instant::now();
        }

        // Periodic instance-registry touch — refresh last_seen so the
        // GC clock cannot expire an active session. Cheap when we
        // already know we have an entry; no-op when registration
        // failed at startup. Use heartbeat_id (the String form
        // resolved at startup, before `session_id` was shadowed by
        // the state_store's Option<String> form).
        if last_instance_touch.elapsed() >= instance_touch_interval {
            instance_registry
                .touch(&focus.working_dir, &heartbeat_id)
                .ok();
            last_instance_touch = Instant::now();
        }

        // Periodic cleanup sweep — remove stale signal files and empty
        // project subdirs from the signals base. Scoped strictly to
        // attend's own data (~/.cache/attend/signals/); never touches
        // ways data or anything else.
        if cleanup_enabled {
            let due = match last_cleanup {
                None => true,
                Some(t) => t.elapsed() >= cleanup_interval,
            };
            if due {
                let base = signals_base();
                let stats = run_cleanup(&base, cleanup_retention, false, false);
                if stats.removed > 0 || stats.dirs_removed > 0 {
                    emit::log(&format!(
                        "cleanup: removed {} signal(s) ({} bytes), {} empty project dir(s)",
                        stats.removed, stats.bytes, stats.dirs_removed,
                    ));
                }
                last_cleanup = Some(Instant::now());
            }
        }
    }
}

fn collect_snapshot(slots: &[sensors::SensorSlot]) -> state::StateSnapshot {
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
