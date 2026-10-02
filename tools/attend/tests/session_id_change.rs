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
