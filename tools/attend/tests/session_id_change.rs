//! A session id that changes under a running `attend run` (#725 review,
//! finding 4).
//!
//! Claude Code's `/clear` gives the running process a new session id and
//! rewrites its session record. `attend run` must follow: register the new
//! id in place of the old one (keeping its instance name), carry the
//! enrollment record and the seen-set across, so the drain under the new id
//! keeps delivering and does not deliver again what the old id saw.

mod common;
use common::Fixture;
use std::time::Duration;

const NEW: &str = "sess-after-clear";

#[test]
fn attend_run_follows_a_session_id_change() {
    let f = Fixture::new("idchange");
    let registry = f.cache().join("instances").join(format!("{}.yaml", f.project_tray()));
    let old = f.sid.clone();
    let run = f.run();
    run.wait_for("registration", Duration::from_secs(20), |_| {
        std::fs::read_to_string(&registry).is_ok_and(|r| r.contains(&old))
    });

    // A message the old id sees.
    f.put("_broadcast", "other-1-before", "seen before the clear", Duration::ZERO);
    run.wait_for("the old id's checkpoint", Duration::from_secs(20), |_| {
        std::fs::read_to_string(f.state_file(&old)).is_ok_and(|s| s.contains("other-1-before"))
    });
    let instance = std::fs::read_to_string(&registry).unwrap();

    f.set_session_id(NEW);
    run.wait_for("the move to the new id", Duration::from_secs(20), |_| {
        std::fs::read_to_string(&registry).is_ok_and(|r| r.contains(NEW) && !r.contains(&old))
    });
    let moved = std::fs::read_to_string(&registry).unwrap();
    assert_eq!(
        instance.lines().nth(1),
        moved.lines().nth(1),
        "the session keeps its instance name: {instance} vs {moved}"
    );
    assert!(f.marker(NEW).exists(), "enrollment carried to the new id");
    assert!(!f.marker(&old).exists(), "and dropped from the old one");
    let state = std::fs::read_to_string(f.state_file(NEW)).expect("seen-set carried");
    assert!(state.contains("other-1-before"), "{state}");
    drop(run);

    // The drain resolves the new id: still enrolled, nothing redelivered.
    assert!(f.drain("plain").contains("no pending messages"));
    f.put("_broadcast", "other-1-after", "after the clear", Duration::ZERO);
    assert!(f.drain("plain").contains("after the clear"));
}

/// #725 re-review, finding 1: a message the sensor has scanned but the
/// disclosure cooldown still holds must not be lost when the id changes.
#[test]
fn a_message_held_in_the_cooldown_survives_an_id_change() {
    let f = Fixture::new("idcooldown");
    let old = f.sid.clone();
    let run = f.run();
    f.put("_broadcast", "other-1-a", "first message", Duration::ZERO);
    run.wait_for("the first message", Duration::from_secs(20), |r| r.output().contains("first message"));
    // The message cooldown is 3 s from that disclosure.
    f.put("_broadcast", "other-1-b", "held in the cooldown", Duration::ZERO);
    run.wait_for("the sensor to scan the second", Duration::from_secs(5), |_| {
        std::fs::read_to_string(f.state_file(&old)).is_ok_and(|s| s.contains("other-1-b"))
    });
    f.set_session_id("sess-cooldown-new");
    run.wait_for("the second message, on the Monitor or the drain", Duration::from_secs(20), |r| {
        r.output().contains("held in the cooldown")
            || (f.marker("sess-cooldown-new").exists() && f.drain("plain").contains("held in the cooldown"))
    });
}

/// #725 re-review, finding 5: a session enrolled only by a join keeps its
/// enrollment, seen-set and channels when `/clear` changes its id, with no
/// `attend run` to move them.
#[test]
fn a_join_only_session_follows_an_id_change() {
    let f = Fixture::new("idjoin");
    f.ok(&["join", "side"]);
    f.put("_broadcast", "other-1-before", "before the clear", Duration::ZERO);
    assert!(f.drain("plain").contains("before the clear"));

    f.set_session_id("sess-join-new");
    f.put("_broadcast", "other-1-after", "open after", Duration::ZERO);
    f.put(&f.project_tray(), "other-1-to", "addressed after", Duration::ZERO);
    f.put("@side", "other-1-side", "channel after", Duration::ZERO);
    let out = f.drain("plain");
    for body in ["open after", "addressed after", "channel after"] {
        assert!(out.contains(body), "missing {body}: {out}");
    }
    assert!(!out.contains("before the clear"), "nothing seen before is delivered again: {out}");
    assert!(f.marker("sess-join-new").exists());
    assert!(!f.marker("sess-idjoin").exists());
    assert!(f.ok(&["channels", "--joined"]).contains("#side"));
}

/// #725 re-review, finding 9: the moves happen in the re-executed run, so
/// a crash before the exec leaves everything on the old id. A run started
/// with `ATTEND_SESSION_MOVED_FROM` performs them.
#[test]
fn a_run_started_after_a_move_performs_it() {
    let f = Fixture::new("idmoved");
    let old = "sess-before-move";
    // The old id's state, as the previous process left it.
    let marker_dir = f.cache().join("enrolled");
    std::fs::create_dir_all(&marker_dir).unwrap();
    std::fs::write(marker_dir.join(old), "run\n").unwrap();
    f.put("_broadcast", "other-1-seen", "seen by the old id", Duration::ZERO);
    let state_dir = f.cache().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join(format!("{old}.state")),
        "seen_signal: other-1-seen.signal\nbaselined: true\nreply_hint_shown: true\n",
    )
    .unwrap();

    let run = f.run_with(&[("ATTEND_SESSION_MOVED_FROM", old)]);
    run.wait_for("the move", Duration::from_secs(20), |_| !f.marker(old).exists());
    let state = std::fs::read_to_string(f.state_file(&f.sid)).unwrap_or_default();
    assert!(state.contains("other-1-seen"), "seen-set moved: {state}");
    assert!(!f.state_file(old).exists());
}

/// #725 third review, finding 1: a run killed before it could hand over,
/// then started fresh under the new id (no ATTEND_SESSION_MOVED_FROM),
/// adopts the old id's state through the Claude process key.
#[test]
fn a_fresh_run_after_an_id_change_adopts_the_old_id() {
    let f = Fixture::new("idrestart");
    let old = f.sid.clone();
    let registry = f.cache().join("instances").join(format!("{}.yaml", f.project_tray()));
    f.ok(&["join", "side"]);
    {
        let run = f.run();
        f.put("_broadcast", "other-1-early", "EARLY-OPEN", Duration::ZERO);
        run.wait_for("the early message", Duration::from_secs(20), |r| r.output().contains("EARLY-OPEN"));
        run.wait_for("its checkpoint", Duration::from_secs(10), |_| {
            std::fs::read_to_string(f.state_file(&old)).is_ok_and(|s| s.contains("other-1-early"))
        });
    } // killed with SIGKILL: no hand-over

    f.set_session_id("sess-restart-new");
    let run = f.run();
    run.wait_for("the adoption", Duration::from_secs(20), |_| {
        f.marker("sess-restart-new").exists() && !f.marker(&old).exists()
    });
    let reg = std::fs::read_to_string(&registry).unwrap();
    assert!(reg.contains("sess-restart-new") && !reg.contains(&old), "{reg}");
    assert!(reg.contains("instance: alpha"), "the slot name carries over: {reg}");
    assert!(f.ok(&["channels", "--joined"]).contains("#side"));
    f.put("@side", "other-1-side", "SIDE-AFTER", Duration::ZERO);
    run.wait_for("the channel message", Duration::from_secs(20), |r| r.output().contains("SIDE-AFTER"));
    assert!(!run.output().contains("EARLY-OPEN"), "nothing seen is shown again: {}", run.output());
}

/// #725 third review, finding 2: when the hand-over exec fails, the run
/// says so on the Monitor and exits, instead of staying on the old id.
#[test]
fn a_failed_hand_over_exits_with_a_restart_line() {
    let f = Fixture::new("idexecfail");
    let bin = f.home.join("attend-copy");
    std::fs::copy(env!("CARGO_BIN_EXE_attend"), &bin).unwrap();
    let mut run = f.run_from(&bin, &[]);
    run.wait_for("registration", Duration::from_secs(20), |_| f.marker(&f.sid).exists());
    std::fs::remove_file(&bin).unwrap();
    f.set_session_id("sess-execfail-new");
    let start = std::time::Instant::now();
    while !run.exited() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(run.exited(), "the run must stop: {}", run.output());
    assert!(run.output().contains("restart attend"), "{}", run.output());
}
