//! Messages that arrived before a session enrolled (#725 review, finding 2).
//!
//! One cold-start rule for both conduits: a message addressed to the
//! project (`--to`) is delivered on enrollment whatever its age, while an
//! old `#open` message is baselined and announced, never consumed silently.
//! Each enrollment path is covered: `attend join` (the drain runs first) and
//! `/attend` (the peers sensor under `attend run` runs first).

mod common;
use common::{Fixture, MINUTE};
use std::time::Duration;

const ADDRESSED: &str = "addressed ten minutes ago";
const OLD_OPEN: &str = "open ten minutes ago";
const NOTE: &str = "1 earlier message not shown; attend inbox";

fn with_backlog(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.put(&f.project_tray(), "other-1-addressed", ADDRESSED, 10 * MINUTE);
    f.put("_broadcast", "other-1-open", OLD_OPEN, 10 * MINUTE);
    f
}

#[test]
fn join_then_drain_delivers_addressed_and_announces_the_rest() {
    let f = with_backlog("backlog-join");
    f.ok(&["join", "side"]);

    let first = f.drain("plain");
    assert!(first.contains(ADDRESSED), "addressed mail must reach the session: {first}");
    assert!(!first.contains(OLD_OPEN), "old #open backlog is not shown: {first}");
    assert!(first.contains(NOTE), "the baseline must be announced: {first}");

    let hook_again = f.drain("hook");
    assert_eq!(hook_again, "", "nothing is delivered twice");
    // The backlog is still there to read on demand.
    assert!(f.ok(&["inbox"]).contains(OLD_OPEN));
}

#[test]
fn attend_run_delivers_addressed_and_announces_the_rest() {
    let f = with_backlog("backlog-run");
    {
        let run = f.run();
        run.wait_for("the addressed message and the note", Duration::from_secs(30), |r| {
            let out = r.output();
            out.contains(ADDRESSED) && out.contains(NOTE)
        });
        assert!(!run.output().contains(OLD_OPEN), "old #open backlog is not shown: {}", run.output());
        // The sensor applied the rule, and its checkpoint says so.
        run.wait_for("the checkpoint", Duration::from_secs(10), |_| {
            std::fs::read_to_string(f.state_file(&f.sid))
                .is_ok_and(|s| s.contains("other-1-addressed") && s.contains("baselined: true"))
        });
    }
    let drained = f.drain("plain");
    assert!(!drained.contains(ADDRESSED) && !drained.contains(OLD_OPEN), "{drained}");
}

/// #725 re-review, finding 2: under `/attend` the turn usually ends a few
/// seconds after `attend run` starts, before the peers sensor's first
/// message scan (its first poll only baselines peer presence, yet
/// checkpoints). A drain in that window must still apply the rule: the
/// 50 cap on addressed mail, old `#open` held back, and the note.
#[test]
fn a_drain_before_the_sensor_scans_applies_the_rule() {
    let f = Fixture::with_peers_interval("backlog-window", 30);
    let tray = f.project_tray();
    for i in 0..55 {
        f.put(&tray, &format!("other-1-to-{i:02}"), &format!("addressed {i:02}"), 10 * MINUTE + Duration::from_secs(i));
    }
    for i in 0..3 {
        f.put("_broadcast", &format!("other-1-open-{i}"), &format!("old open {i}"), 60 * MINUTE);
    }
    let run = f.run();
    run.wait_for("the sensor's first checkpoint", Duration::from_secs(10), |_| f.state_file(&f.sid).exists());
    let hook = f.drain_hook_with("{}");
    assert!(hook.contains("50 peer message(s)"), "the 50 cap: {hook}");
    assert!(hook.contains("8 earlier messages not shown; attend inbox"), "the note: {hook}");
    assert!(!hook.contains("old open"), "old #open is held back: {hook}");
}
