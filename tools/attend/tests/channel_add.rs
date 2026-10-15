//! Adding a member to a channel (ADR-404 D1): `attend channel add`, and the
//! briefing an added session reads, through the drain.
//!
//! The add enrolls the session with no answer from it, tells it by a direct
//! message, and hands it the channel's newest 50 messages to read, as a join
//! answering an invitation is. A session whose cold start has not run is
//! owed them as a briefing its cold start delivers.

mod common;
use common::{Fixture, MINUTE};
use std::time::Duration;

/// `n` messages in `#topic` from another session, each ten minutes and more
/// old, past the cold-start window; the first is the oldest.
fn history(f: &Fixture, n: usize) {
    for i in 0..n {
        f.put("@topic", &format!("other-brief-{i:02}"), &format!("briefing {i:02}"), 10 * MINUTE + Duration::from_secs((n - i) as u64));
    }
}

/// The fixture's session added to `#topic` by the person `aaron`, as
/// attend-chat's Add agent does it.
fn added_by_aaron(f: &Fixture) -> attend_membership::Added {
    let bus = attend_membership::Bus { signals: f.signals(), state: f.cache().join("state"), lock_wait: None };
    let actor = attend_membership::Actor {
        member_id: "aaron".into(),
        kind: "human",
        name: "aaron".into(),
        sender_id: "aaron-term".into(),
        from: "external:aaron-term".into(),
        project: "chat".into(),
        cwd: "/home/aaron".into(),
    };
    let target = attend_membership::Target { session_id: &f.sid, root: &f.origin };
    attend_membership::add(&bus, &actor, "topic", target).expect("added")
}

fn with_topic(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    attend_groups::Groups::new(&f.signals(), "aaron").create("topic", None).unwrap();
    f
}

#[test]
fn a_session_added_before_it_listens_reads_the_newest_history() {
    let f = with_topic("add-cold");
    history(&f, 60);
    f.put("_broadcast", "other-open", "old open chatter", 30 * MINUTE);
    let added = added_by_aaron(&f);
    assert_eq!((added.to_read, added.held), (50, 10));

    f.ok(&["listen", "on"]);
    let drained = f.drain("plain");
    assert!(drained.contains("aaron added you to #topic. Its recent history is yours to read."), "{drained}");
    assert!(drained.contains("briefing 59") && drained.contains("briefing 10"), "{drained}");
    assert!(!drained.contains("briefing 09"), "older than the newest 50: {drained}");
    assert!(!drained.contains("old open chatter"), "the rest of the cold start still holds: {drained}");
    assert!(drained.contains("51 message(s) drained"), "the 50 newest of the room and the add: {drained}");
}

#[test]
fn a_warm_session_added_reads_the_newest_history_too() {
    let f = with_topic("add-warm");
    f.ok(&["channel", "join", "newtopic"]);
    f.drain("plain");
    history(&f, 53);
    let added = added_by_aaron(&f);
    assert_eq!((added.to_read, added.held), (50, 3));
    let drained = f.drain("plain");
    assert!(drained.contains("briefing 52") && drained.contains("briefing 03"), "{drained}");
    assert!(!drained.contains("briefing 02"), "{drained}");
    assert!(drained.contains("aaron added you to #topic."), "{drained}");
}

/// Self-joining after an add changes nothing: the session is a member, and
/// what it has not read there is still owed to it.
#[test]
fn joining_a_channel_one_was_added_to_keeps_the_briefing() {
    let f = with_topic("add-then-join");
    history(&f, 3);
    added_by_aaron(&f);
    let joined = f.ok(&["channel", "join", "topic"]);
    assert!(!joined.contains("held back"), "{joined}");
    assert!(f.drain("plain").contains("briefing 00"));
}

// Naming a session to add reads the process scan, which the peers sensor
// carries.
#[cfg(feature = "sensor-peers")]
mod cli {
    use super::*;
    use common::Peer;

    const PEER: &str = "sess-add-peer";

    #[test]
    fn add_enrolls_a_live_session_spends_its_invitation_and_tells_it() {
        let f = with_topic("add-cli");
        let peer = Peer::start(&f, PEER, "other");
        f.put("@topic", "other-said", "earlier talk", 10 * MINUTE);
        attend_groups::Groups::new(&f.signals(), "aaron").invite("topic", PEER).unwrap();

        let out = f.ok(&["channel", "add", PEER, "#topic"]);
        assert!(out.contains(" to #topic (1 earlier message(s) to read)"), "{out}");
        let groups = attend_groups::Groups::new(&f.signals(), "x");
        assert_eq!(groups.members("topic").unwrap(), vec![PEER]);
        assert!(groups.invites("topic").is_empty(), "the invitation is spent");

        let told: Vec<String> = std::fs::read_dir(peer.tray(&f))
            .unwrap()
            .flatten()
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].contains(&format!("from_kind:agent|from_id:{}|to:{PEER}|", f.sid)), "{}", told[0]);
        assert!(told[0].contains(" added you to #topic. Its recent history is yours to read."), "{}", told[0]);

        let again = f.attend(&["channel", "add", PEER, "topic"]);
        assert!(!again.status.success());
        assert!(String::from_utf8_lossy(&again.stderr).contains("is already in #topic"));
    }

    #[test]
    fn add_refuses_open_and_unknown_channels_and_writes_nothing() {
        let f = with_topic("add-refuse");
        let peer = Peer::start(&f, PEER, "other");
        for (channel, says) in [("open", "#open is the base channel"), ("#open", "#open is the base channel"), ("ghost", "#ghost: unknown channel")] {
            let out = f.attend(&["channel", "add", PEER, channel]);
            assert!(!out.status.success(), "{channel}");
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(err.contains(says), "{channel}: {err}");
        }
        assert!(!peer.tray(&f).exists(), "a refused add tells no one");
        assert!(!f.signals().join("@ghost").exists());
        assert!(attend_groups::Groups::new(&f.signals(), "x").members("topic").unwrap().is_empty());
    }

    #[test]
    fn with_no_channel_named_add_takes_the_one_this_session_is_in() {
        let f = with_topic("add-default");
        let _peer = Peer::start(&f, PEER, "other");
        let none = f.attend(&["channel", "add", PEER]);
        assert!(String::from_utf8_lossy(&none.stderr).contains("name the channel: this session is in none"));
        f.ok(&["channel", "join", "topic"]);
        assert!(f.ok(&["channel", "add", PEER]).contains(" to #topic"));
    }
}
