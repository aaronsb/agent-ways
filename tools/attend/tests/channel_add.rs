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
    added_by_aaron_as(f, &f.sid)
}

/// The fixture's bus.
fn bus(f: &Fixture) -> attend_membership::Bus {
    attend_membership::Bus {
        signals: f.signals(),
        state: f.cache().join("state"),
        enrolled: f.cache().join("enrolled"),
        instances: f.cache().join("instances"),
        heartbeats: f.cache().join("heartbeat"),
        lock_wait: None,
    }
}

/// Session `sid`, working at the fixture's origin, added to `#topic` by
/// `aaron`.
fn added_by_aaron_as(f: &Fixture, sid: &str) -> attend_membership::Added {
    let bus = bus(f);
    let actor = attend_membership::Actor {
        member_id: "aaron".into(),
        kind: "human",
        name: "aaron".into(),
        sender_id: "aaron-term".into(),
        from: "external:aaron-term".into(),
        project: "chat".into(),
        cwd: "/home/aaron".into(),
    };
    let target = attend_membership::Target { session_id: sid, root: &f.origin };
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
    assert!(
        drained.contains("aaron added you to #topic. Its recent history is yours to read. Leave with attend channel leave topic."),
        "{drained}"
    );
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

/// A session that listened and then turned listening off stays baselined
/// but is no longer enrolled. Added, it starts listening again: its cold
/// start reopens, so it reads the channel's newest history and is not
/// handed every `#open` message since it stopped listening (ADR-404 D1).
#[test]
fn a_session_added_after_it_stopped_listening_starts_cold() {
    let f = with_topic("add-after-off");
    f.ok(&["listen", "on"]);
    f.drain("plain");
    f.ok(&["listen", "off"]);
    f.put("_broadcast", "other-open", "open chatter while away", 30 * MINUTE);
    history(&f, 60);
    let added = added_by_aaron(&f);
    assert_eq!((added.to_read, added.held), (50, 10));
    let drained = f.drain("plain");
    assert!(!drained.contains("open chatter while away"), "the #open backlog is held back: {drained}");
    assert!(drained.contains("briefing 59") && drained.contains("briefing 10"), "{drained}");
    assert!(!drained.contains("briefing 09"), "{drained}");
    assert!(drained.contains("aaron added you to #topic."), "{drained}");
}

/// An added session that leaves its only channel stops listening: the add
/// was all that enrolled it, so the drain delivers it nothing more.
#[test]
fn an_added_session_that_leaves_its_only_channel_stops_listening() {
    let f = with_topic("add-then-leave");
    added_by_aaron(&f);
    f.drain("plain");
    f.ok(&["channel", "leave", "topic"]);
    f.put(&f.project_tray(), "other-project", "project mail after leaving", MINUTE);
    f.put("_broadcast", "other-open", "open post after leaving", MINUTE);
    let drained = f.drain("plain");
    assert!(!drained.contains("after leaving"), "{drained}");
}

/// The same for an added session removed from its only channel.
#[test]
fn an_added_session_removed_from_its_only_channel_stops_listening() {
    let f = with_topic("add-then-remove");
    added_by_aaron(&f);
    f.drain("plain");
    attend_membership::remove(&bus(&f), "aaron", "topic", &f.sid).unwrap();
    f.put(&f.project_tray(), "other-project", "project mail after removal", MINUTE);
    f.put("_broadcast", "other-open", "open post after removal", MINUTE);
    let drained = f.drain("plain");
    assert!(!drained.contains("after removal"), "{drained}");
}

/// An added session the sweep drops from its only channel, its process
/// gone, loses the add's enrollment with it.
#[test]
fn the_sweep_withdraws_the_add_of_a_member_it_drops_from_every_channel() {
    let f = with_topic("add-then-sweep");
    let gone = "2f2632d7-3f59-4c24-9aae-00000000dead";
    added_by_aaron_as(&f, gone);
    assert!(f.cache().join("enrolled").join(gone).exists());
    f.ok(&["channel", "list"]);
    assert!(attend_groups::Groups::new(&f.signals(), "x").members("topic").unwrap().is_empty());
    assert!(!f.cache().join("enrolled").join(gone).exists(), "the add's enrollment is withdrawn");
}

/// An invitee that exits without ever listening leaves no session record
/// here, since Claude Code deletes it at exit, and no heartbeat: the sweep
/// prunes its invitation.
#[test]
fn the_sweep_prunes_an_invitation_to_a_session_that_has_gone() {
    let f = with_topic("invite-gone");
    let groups = attend_groups::Groups::new(&f.signals(), "aaron");
    groups.invite("topic", "2f2632d7-3f59-4c24-9aae-00000000dead").unwrap();
    f.ok(&["channel", "list"]);
    assert!(groups.invites("topic").is_empty(), "the invitation is pruned");
}

/// An invitee that listened, turned listening off and then exited has no
/// session record here and a heartbeat younger than the hold. Its
/// enrollment record keeps its Claude Code process through the opt-out, so
/// the sweep sees that process has exited and prunes its invitation.
#[test]
fn the_sweep_prunes_an_invitation_to_a_session_that_stopped_listening_and_exited() {
    let f = with_topic("invite-off-gone");
    let gone = "sess-off-gone";
    f.attend_in_process(gone, Some(common::DAY), &["listen", "on"]);
    f.attend_in_process(gone, Some(common::DAY), &["message", "inbox", "--drain", "--format", "plain"]);
    f.attend_in_process(gone, Some(common::DAY), &["listen", "off"]);
    common::age_file(&f.heartbeat(gone), 2 * MINUTE);
    let groups = attend_groups::Groups::new(&f.signals(), "aaron");
    groups.invite("topic", gone).unwrap();
    f.ok(&["channel", "list"]);
    assert!(groups.invites("topic").is_empty(), "the invitation is pruned");
}

/// An added session whose only channel is dissolved stops listening.
#[test]
fn an_added_session_whose_only_channel_is_dissolved_stops_listening() {
    let f = with_topic("add-then-dissolve");
    added_by_aaron(&f);
    f.drain("plain");
    f.ok(&["channel", "dissolve", "topic"]);
    f.put(&f.project_tray(), "other-project", "project mail after dissolving", MINUTE);
    f.put("_broadcast", "other-open", "open post after dissolving", MINUTE);
    let drained = f.drain("plain");
    assert!(!drained.contains("after dissolving"), "{drained}");
}

/// A session that joined its only channel itself and is removed from it
/// stops listening, as one that leaves does.
#[test]
fn a_session_removed_from_the_only_channel_it_joined_stops_listening() {
    let f = with_topic("join-then-remove");
    f.ok(&["channel", "join", "topic"]);
    f.drain("plain");
    attend_membership::remove(&bus(&f), "aaron", "topic", &f.sid).unwrap();
    f.put(&f.project_tray(), "other-project", "project mail after removal", MINUTE);
    f.put("_broadcast", "other-open", "open post after removal", MINUTE);
    let drained = f.drain("plain");
    assert!(!drained.contains("after removal"), "{drained}");
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
        assert!(out.contains(" to #topic: ") && out.trim_end().ends_with(" has 1 earlier message to read"), "{out}");
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

    /// An added session that never listened is enrolled by the add (ADR-404
    /// D1), so the membership sweep keeps it while its Claude Code process
    /// runs, and the drain delivers the add's message to it.
    #[test]
    fn an_added_session_that_is_not_listening_stays_a_member_and_is_told() {
        let f = with_topic("add-dormant");
        let _peer = Peer::start(&f, PEER, "proj");
        f.ok(&["channel", "add", PEER, "topic"]);
        // The listing sweeps members that are not alive.
        f.ok(&["channel", "list"]);
        assert_eq!(attend_groups::Groups::new(&f.signals(), "x").members("topic").unwrap(), vec![PEER]);
        let drained = f.attend_in_process(PEER, Some(common::DAY), &["message", "inbox", "--drain", "--format", "plain"]);
        assert!(drained.contains(" added you to #topic."), "{drained}");
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

    /// A project path names every session working there, so an add, which
    /// takes one session, refuses it, even where one session works there.
    #[test]
    fn add_refuses_a_project_path() {
        let f = with_topic("add-path");
        let peer = Peer::start(&f, PEER, "other");
        let out = f.attend(&["channel", "add", &peer.cwd, "topic"]);
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("is a project path"), "{err}");
        assert!(attend_groups::Groups::new(&f.signals(), "x").members("topic").unwrap().is_empty());
    }

    /// An add that lands between a session's `/clear` and its next drain
    /// moves the earlier id's state to the new id first, as the drain's
    /// adoption would have: its channels, its seen-set, its enrollment and
    /// the mail addressed to the earlier id. The add records the session's
    /// Claude Code process, so the earlier id must be found before the add's
    /// enrollment names the new id under that process.
    #[test]
    fn an_add_between_a_clear_and_the_next_drain_carries_the_earlier_id() {
        const OLD: &str = "sess-before-clear";
        const NEW: &str = "sess-after-clear";
        let f = with_topic("add-after-clear");
        let mut claude = common::Claude::start(&f, OLD);
        claude.attend(&["channel", "join", "other"]);
        claude.attend(&["message", "inbox", "--drain", "--format", "plain"]);
        f.put(&f.project_tray(), "other-read", "READ-BEFORE-CLEAR", Duration::ZERO);
        assert!(claude.attend(&["message", "inbox", "--drain", "--format", "plain"]).contains("READ-BEFORE-CLEAR"));
        std::fs::write(
            f.signals().join(f.project_tray()).join("other-to-old.signal"),
            format!("claude:other-session|elsewhere|/tmp/elsewhere|to:{OLD}|TO-THE-OLD-ID\n"),
        )
        .unwrap();

        claude.clear(&f, NEW);
        f.ok(&["channel", "add", NEW, "topic"]);
        f.put("@other", "other-after", "OTHER-AFTER-CLEAR", Duration::ZERO);
        let drained = claude.attend(&["message", "inbox", "--drain", "--format", "plain"]);
        for body in ["OTHER-AFTER-CLEAR", "TO-THE-OLD-ID", " added you to #topic."] {
            assert_eq!(drained.matches(body).count(), 1, "{body} once: {drained}");
        }
        assert!(!drained.contains("READ-BEFORE-CLEAR"), "nothing read before the clear is delivered again: {drained}");
        assert!(claude.attend(&["message", "inbox", "--drain", "--format", "plain"]).contains("no pending messages"));

        let groups = attend_groups::load_groups(&f.signals());
        for (name, entry) in &groups {
            assert!(!entry.members.iter().any(|m| m == OLD), "#{name} still lists the earlier id: {:?}", entry.members);
        }
        assert_eq!(groups["other"].members, vec![NEW]);
        assert_eq!(groups["topic"].members, vec![NEW]);
        assert!(!f.marker(OLD).exists(), "the earlier id's enrollment is carried");
        let record = std::fs::read_to_string(f.marker(NEW)).unwrap();
        assert!(record.lines().any(|l| l == "join") && record.lines().any(|l| l == "add"), "{record}");
        assert!(record.lines().any(|l| l == format!("previous {OLD}")), "{record}");
        assert!(!f.state_file(OLD).exists(), "the earlier id's seen-set is carried");
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
