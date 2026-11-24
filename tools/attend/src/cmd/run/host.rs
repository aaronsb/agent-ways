// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor host --session <id> --cwd <dir> [--moved-from <old id>]
//! [--parent <pid>]`: one session's sensors, run for as long as the attend
//! mod keeps it.
//!
//! The host polls the sensors the files declare for the session's
//! directory through the tick loop: scheduling,
//! intervals and decay, the event lane's salience, refractory and governor,
//! the checkpoint, the stamp sweep and the re-exec on a changed binary or
//! sensor file. What the governor lets through becomes a system signal in
//! the session's project tray, addressed to the session, and each write
//! prints one line on stdout, `signal <id>`, which the mod takes as a cue
//! to poll at once. Logs go to stderr.
//!
//! It has no message poller, no wake gate, no banner and no rekey: the
//! mod's drain delivers what it writes, the mod spawns a new host after a
//! `/clear`, and it does not run the keepwarm sensor. The
//! peers sensor reports presence alone. With `--moved-from` the stores and
//! the session memory of the id the session had before `/clear` carry
//! over, so a trigger kept for the session that fired under the old id
//! stays spent.
//!
//! The host holds the session's instance slot, which sets the name peers
//! see it by (bare, or with a suffix when it joined another live session
//! in its directory), and refreshes it every minute. With `--moved-from` the
//! old id's slot moves to the new id before the host registers, so the
//! name stays the same across `/clear`.
//!
//! TERM, HUP or INT ends the host at its next look, within a second: it
//! returns, and the sensors it drops end their MCP servers. A host killed
//! with SIGKILL ends nothing, so a starting host first ends the servers a
//! killed process left running for its project.
//!
//! References:
//! - ADR-416 items 2 to 5: the host, its stdout, the store as transport,
//!   the system sender and what the host runs.
//! - ADR-416 item 8: stores keyed by session, carried by a session's move.
//! - ADR-129: the instance slot and its suffix.

use std::collections::BinaryHeap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::governor::{DisclosureGovernor, ScheduledSensor};
use super::sink::{Delivery, Disclosure, Sink};
use super::tick::{self, build_engagement, TickState};
use crate::cli::HostArgs;
use crate::sensors::{self, writer::Writer, Focus};
use crate::{config, emit, state};

const RELOAD_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// The longest the loop sleeps between looks at its parent, so a sensor
/// polled once a week does not hold it asleep for a week.
const WAKE_INTERVAL: Duration = Duration::from_secs(1);

/// Set by TERM, HUP or INT: the host ends at its next look.
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// Take TERM, HUP and INT as a request to end, so the host returns and
/// drops its sensors, which ends their servers, instead of dying with
/// them running.
fn end_on_signals() {
    #[cfg(unix)]
    for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        // SAFETY: the handler only stores to an atomic, which is
        // async-signal-safe; it is set before any thread starts.
        unsafe {
            libc::signal(sig, on_stop as *const () as libc::sighandler_t);
        }
    }
}

/// The host's sink: each line a system signal, and a cue on stdout. An
/// event-lane batch under the cut ([`crate::sensors::writer::EVENT_CUT`]) is
/// logged and not written.
struct Signals {
    writer: Writer,
    /// A cue met a closed stdout: the mod stopped reading, and the host
    /// exits once the tick that wrote it has checkpointed.
    closed: bool,
}

impl Sink for Signals {
    fn deliver(&mut self, batch: &[Disclosure]) -> Vec<Vec<Delivery>> {
        let mut out = std::io::stdout().lock();
        let verdicts = batch
            .iter()
            .map(|d| {
                let lane = if d.message_lane { sensor_spec::Lane::Message } else { sensor_spec::Lane::Event };
                if crate::sensors::writer::below_cut(lane, d.magnitude) {
                    for (line, _) in &d.lines {
                        emit::log(&format!("{}: under the event lane's cut ({:.1} of {:.1}), not written: {line}", d.sensor, d.magnitude, crate::sensors::writer::EVENT_CUT));
                    }
                    return vec![Delivery::Cut; d.lines.len()];
                }
                d.lines
                    .iter()
                    .map(|(line, lapse)| match self.writer.write(&d.sensor, lane, *lapse, line) {
                        Ok(id) => {
                            emit::log(&format!("{}: wrote {id}: {line}", d.sensor));
                            if let Err(e) = writeln!(out, "signal {id}") {
                                self.closed |= e.kind() == std::io::ErrorKind::BrokenPipe;
                            }
                            Delivery::Written
                        }
                        Err(why) => {
                            emit::log(&format!("{}: not written: {why}", d.sensor));
                            Delivery::Failed
                        }
                    })
                    .collect()
            })
            .collect();
        out.flush().ok();
        verdicts
    }
}

pub(crate) fn cmd_host(a: &HostArgs) {
    // The parent is read first, so a parent that leaves during startup is
    // still seen to leave. One that left before this line, between the
    // spawn and attend's main, is seen only through --parent: the host was
    // adopted already, and its parent reads as the adopter's.
    let parent = parent_id();
    if is_gone(a.parent) {
        emit::log("the process that started the sensor host is gone; it exits");
        return;
    }
    // attend's main restores SIGPIPE's default (agent_fmt::
    // exit_quietly_on_closed_pipe), under which a cue written to a closed
    // pipe kills the host before the tick that wrote it checkpoints. The
    // host ignores it again, so the write returns BrokenPipe to handle. A
    // child the host runs, and the host re-executed, get the default back.
    #[cfg(unix)]
    // SAFETY: setting a signal disposition before any thread starts.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    end_on_signals();
    std::env::remove_var("ATTEND_RELOADED_FROM");
    let refuse = |why: String| -> ! { crate::cmd::refuse_usage(&format!("attend sensor host: {why}"), &why, false) };
    if !std::path::Path::new(&a.cwd).is_dir() {
        refuse(format!("{} is not a directory", a.cwd));
    }
    let writer = Writer::new(&a.session, &a.cwd).unwrap_or_else(|why| refuse(why));
    let session = writer.session().to_string();
    let mut focus = Focus::default_focus();
    focus.working_dir = writer.cwd().to_string();
    emit::log(&format!("sensor host for session {session} in {}", focus.working_dir));

    let moved_from = a.moved_from.as_deref().filter(|old| !old.is_empty() && *old != session);
    if let Some(old) = moved_from {
        let moved = sensors::hosted::move_stores(old, &session);
        emit::log(&format!("moved {moved} sensor store(s) from {old}"));
    }
    let registry = attend_instances::Registry::new();
    register(&registry, &focus.working_dir, &session, moved_from);

    let swept = sensors::sweep_mcp_servers(&focus.working_dir);
    if swept > 0 {
        emit::log(&format!("ended {swept} MCP server(s) a killed process left running"));
    }

    let cfg = config::Config::load(&focus.working_dir);
    let (mut slots, names) = sensors::register_host_sensors(&cfg, &focus, &session);
    let curve = build_engagement(&cfg);
    for slot in &mut slots {
        slot.engagement = sensor_trait::EngagementState::new(curve.clone());
    }
    let state_store = state::StateStore::new(Some(session.clone()));
    if let Some(snapshot) = restored(&state_store, moved_from) {
        for slot in &mut slots {
            let saved = tick::restored_state(slot.name(), slot.crate_id.as_deref(), &snapshot);
            if !saved.is_empty() {
                slot.import_state(&saved);
            }
        }
    }
    emit::log(&format!("running {} sensor(s): {}", slots.len(), names.join(", ")));

    let live = super::stamps::live_sessions();
    let swept = super::stamps::sweep_stale(
        &attend_presence::cache::state_dir(),
        |key| super::stamps::is_live(key, &live),
        std::time::SystemTime::now(),
        super::stamps::STAMP_MAX_AGE,
        super::stamps::SWEEP_MAX_CANDIDATES,
    );
    if swept > 0 {
        emit::log(&format!("removed {swept} stamp file(s) of sessions gone for over a week"));
    }

    let mut governor = DisclosureGovernor::new(cfg.governor.base_cooldown, cfg.governor.max_per_window, cfg.governor.rate_window);
    let mut msg_governor = DisclosureGovernor::new_permissive(Duration::from_secs(3), 30, Duration::from_secs(60));
    let mut queue: BinaryHeap<ScheduledSensor> = slots.iter().enumerate().map(|(index, s)| ScheduledSensor { fire_at: s.next_fire, index }).collect();
    let mut last_checkpoint = Instant::now();
    let mut last_instance_touch = Instant::now();
    let mut last_cleanup: Option<Instant> = None;
    let mut sink = Signals { writer, closed: false };

    let self_exe = std::env::current_exe().ok();
    let mut exe_mtime = self_exe.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
    let exe_hash = tick::initial_self_hash(self_exe.as_deref());
    let sensor_files = sensors::files_stamp(&focus.working_dir);
    let mut last_reload_check = Instant::now();

    loop {
        if parent_id() != parent || is_gone(a.parent) {
            emit::log("the process that started the sensor host is gone; it exits");
            return;
        }
        if STOP.load(Ordering::Relaxed) {
            emit::log("asked to end by a signal; the sensor host exits");
            return;
        }
        if last_reload_check.elapsed() >= RELOAD_CHECK_INTERVAL {
            let why = match () {
                _ if tick::binary_changed(self_exe.as_deref(), &mut exe_mtime, exe_hash) => Some("binary changed"),
                _ if sensors::files_stamp(&focus.working_dir) != sensor_files => Some("sensor files changed"),
                _ => None,
            };
            if let Some(why) = why {
                reexec(&slots, &state_store, why);
            }
            last_reload_check = Instant::now();
        }
        let now = Instant::now();
        let next = queue.peek().map_or(now + WAKE_INTERVAL, |s| s.fire_at).min(now + WAKE_INTERVAL);
        if next > now {
            std::thread::sleep(next - now);
        }
        // A signal that came during the sleep ends the host before it polls
        // again, so it writes nothing more for a session that moved on.
        if STOP.load(Ordering::Relaxed) {
            emit::log("asked to end by a signal; the sensor host exits");
            return;
        }
        tick::tick_iteration(&mut TickState {
            slots: &mut slots,
            queue: &mut queue,
            governor: &mut governor,
            msg_governor: &mut msg_governor,
            last_checkpoint: &mut last_checkpoint,
            last_instance_touch: &mut last_instance_touch,
            last_cleanup: &mut last_cleanup,
            state_store: &state_store,
            instance_registry: &registry,
            focus: &focus,
            heartbeat_id: &session,
            cfg: &cfg,
            sink: &mut sink,
        });
        if sink.closed {
            emit::log("stdout is closed, so no one reads the cues; the sensor host exits");
            return;
        }
    }
}

/// The parent process's id. A change means the process that started the
/// host, the attend mod's Claude Code, is gone and the host was adopted.
/// A re-exec keeps the parent.
fn parent_id() -> u32 {
    #[cfg(unix)]
    return std::os::unix::process::parent_id();
    #[cfg(not(unix))]
    return attend_presence::process::parent_pid(std::process::id()).unwrap_or(0);
}

/// Whether the process `--parent` names has exited. Without the flag the
/// parent's id alone answers.
fn is_gone(parent: Option<u32>) -> bool {
    parent.is_some_and(|pid| !attend_presence::process::is_running(pid))
}

/// Take or keep session `session`'s instance slot in `cwd`: the bare name
/// (`Chaucer`) for the first live session there, a suffix (`Chaucer-alpha`)
/// for one that joins it. After `/clear` the slot of the id the session had
/// before moves to the new id first, so the name does not change;
/// registering first would allocate the new id a slot of its own.
/// The tick loop refreshes it every minute ([`attend_instances::Registry::keep`]),
/// which takes a new slot should it have been collected and leaves an
/// alias row alone.
fn register(registry: &attend_instances::Registry, cwd: &str, session: &str, moved_from: Option<&str>) {
    if let Some(old) = moved_from {
        if let Err(e) = registry.rename(cwd, old, session) {
            emit::log(&format!("instance slot of {old} not moved ({e})"));
        }
    }
    match registry.register(cwd, session) {
        Ok(instance) if instance == attend_instances::BARE => emit::log("instance: bare"),
        Ok(instance) => emit::log(&format!("instance: {instance}")),
        Err(e) => emit::log(&format!("instance registry unavailable ({e}); the session's name has no suffix")),
    }
}

/// The session's checkpoint, with the session memory of the id it had
/// before `/clear` for each sensor that has none under the new id. The
/// mod's move of the old checkpoint may come before or after the host
/// starts, so the old one is read whichever way it went.
fn restored(store: &state::StateStore, moved_from: Option<&str>) -> Option<state::StateSnapshot> {
    let mut snapshot = store.restore();
    let old = moved_from.and_then(|old| state::StateStore::new(Some(old.to_string())).load());
    if let Some(old) = old.filter(|o| !o.sensor_memory.is_empty()) {
        let into = snapshot.get_or_insert_with(state::StateSnapshot::default);
        for (name, memo) in old.sensor_memory {
            into.sensor_memory.entry(name).or_insert(memo);
        }
    }
    snapshot
}

/// Checkpoint and re-execute in place because `why`, keeping the pid and
/// the pipes, so the mod reading this process does not notice. Returns
/// only when the exec failed, which is logged.
fn reexec(slots: &[sensors::SensorSlot], store: &state::StateStore, why: &str) {
    emit::log(&format!("{why}; checkpointing and re-executing"));
    store.checkpoint(&tick::collect_snapshot(slots));
    std::io::stdout().flush().ok();
    tick::reexec("ATTEND_RELOADED_FROM", &format!("v{} ({})", env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT")));
}
