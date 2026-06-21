//! `attend run` — sensor loop, governor, self-reload path.
//!
//! The hottest module in attend. Split into:
//! - `governor` — `DisclosureGovernor` rate-limit state machine plus
//!   the priority-queue entry type. Pure CPU; no IO.
//! - `tick` — per-tick body, engagement-curve builder, self-reload
//!   helper, and the `TickState` bundle that ties them to a single
//!   iteration. Reachable from tests without standing up the full
//!   `cmd_run_with_catchup` shell.
//!
//! This file owns the setup-then-loop skeleton: process-level locking,
//! state restoration, sensor registration, the startup banner, and
//! the outer loop that calls `tick::tick_iteration` once per beat.

mod governor;
mod tick;

use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use governor::{DisclosureGovernor, ScheduledSensor};
use tick::{build_engagement, maybe_self_reload, tick_iteration, TickState};

use crate::sensors::Focus;
use crate::util::{own_session_id, signals_base};
use crate::{config, emit, groups, sensors, state};

const RELOAD_CHECK_INTERVAL: Duration = Duration::from_secs(10);

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

    let engagement_curve = build_engagement(&cfg);
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

    print_startup_banner(&enabled_names, &focus_desc);

    let mut governor = DisclosureGovernor::new(
        cfg.governor.base_cooldown,
        cfg.governor.max_per_window,
        cfg.governor.rate_window,
    );
    // Permissive governor for the message lane (ADR-136 Decision 1):
    // a flat 3s cooldown and a generous 30/window cap so authored
    // messages flow at normal cadence and a coalesced burst discloses
    // promptly, instead of being starved by the event-lane back-off.
    let mut msg_governor = DisclosureGovernor::new_permissive(
        Duration::from_secs(3),
        30,
        Duration::from_secs(60),
    );

    let mut queue: BinaryHeap<ScheduledSensor> = BinaryHeap::new();
    for (i, slot) in slots.iter().enumerate() {
        queue.push(ScheduledSensor {
            fire_at: slot.next_fire,
            index: i,
        });
    }

    let mut last_checkpoint = Instant::now();
    let mut last_instance_touch = Instant::now();
    // Cleanup sweep — fire a first sweep on startup so long-running
    // instances don't wait a full interval before the first prune.
    let mut last_cleanup: Option<Instant> = None;

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

    // Self-reload: track own binary mtime as the cheap trigger, plus a
    // content hash so an identical rebuild (mtime bump, same bytes) is
    // distinguished from a real binary change (issue #140).
    let self_exe = std::env::current_exe().ok();
    let mut initial_mtime = self_exe
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let initial_hash = tick::initial_self_hash(self_exe.as_deref());
    let mut last_reload_check = Instant::now();

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

        if last_reload_check.elapsed() >= RELOAD_CHECK_INTERVAL {
            maybe_self_reload(
                self_exe.as_deref(),
                &mut initial_mtime,
                initial_hash,
                &slots,
                &state_store,
            );
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

        let mut tick_state = TickState {
            slots: &mut slots,
            queue: &mut queue,
            governor: &mut governor,
            msg_governor: &mut msg_governor,
            last_checkpoint: &mut last_checkpoint,
            last_instance_touch: &mut last_instance_touch,
            last_cleanup: &mut last_cleanup,
            state_store: &state_store,
            instance_registry: &instance_registry,
            focus: &focus,
            heartbeat_id: &heartbeat_id,
            cfg: &cfg,
        };
        tick_iteration(&mut tick_state);
    }
}

/// Print the startup banner. Suppresses repeated unchanged-config
/// banners, names hot-swap reloads explicitly, and writes a
/// fingerprint stamp so the next startup can compare.
fn print_startup_banner(enabled_names: &[String], focus_desc: &str) {
    let sensor_list = enabled_names.join(", ");
    let banner_fingerprint = format!(
        "v{}:{}:{}:{}",
        env!("CARGO_PKG_VERSION"),
        env!("ATTEND_COMMIT"),
        sensor_list,
        focus_desc
    );

    // The `ATTEND_RELOADED_FROM` env var is set by the self-reload
    // exec() path; when present, this process is the post-exec child
    // of a binary-change reload, so the banner explicitly names it
    // as a hot-swap rather than a silent "unchanged" restart. The
    // env var is consumed (removed) to keep it from leaking into any
    // subprocesses attend itself spawns.
    let reloaded_from = std::env::var("ATTEND_RELOADED_FROM").ok();
    std::env::remove_var("ATTEND_RELOADED_FROM");
    let stamp_path = signals_base().join("_last_banner");
    let prev_fingerprint = std::fs::read_to_string(&stamp_path).unwrap_or_default();
    if let Some(prev_version) = reloaded_from {
        // Always emit on a binary hot-swap — the operator needs to
        // know the running code changed under them, even when
        // sensors/focus are identical.
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
}
