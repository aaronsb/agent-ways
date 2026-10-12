//! A member can see who else is in a channel before anyone posts (the
//! attend mod's clean-slate test, finding 5): `attend channels` and `attend
//! peers` name each channel's members, through the CLI the attend_peers
//! tool runs. A person in attend-chat is a member like an agent, marked
//! `(attend-chat)`, and a live one is a listener a send or reply reaches.

mod common;
use common::Fixture;
use serde_json::Value;

/// A fresh heartbeat for `member`, which keeps it in its channels when a
/// listing drops members whose sessions are gone.
fn alive(f: &Fixture, member: &str) {
    let hb = f.heartbeat(member);
    std::fs::create_dir_all(hb.parent().unwrap()).unwrap();
    std::fs::write(hb, "").unwrap();
}

/// `aaron`, a person in attend-chat, joins `room`.
fn human_in(f: &Fixture, room: &str) {
    attend_groups::Groups::new(&f.signals(), "aaron").join(room, false).unwrap();
    alive(f, "aaron");
}

#[test]
fn channels_and_peers_name_the_members() {
    let f = Fixture::new("channel-members");
    f.ok(&["join", "room"]);
    alive(&f, &f.sid);
    human_in(&f, "room");
    let me: Value = serde_json::from_str(&f.ok(&["peers", "--json"])).unwrap();
    let my_name = me["self"]["name"].as_str().unwrap().to_string();
    let expected = format!("{my_name} (you), aaron (attend-chat)");

    let listing = f.ok(&["channels"]);
    assert!(listing.contains(&format!("#room: {expected}")), "{listing}");
    let channels: Value = serde_json::from_str(&f.ok(&["channels", "--json"])).unwrap();
    let room = channels.as_array().unwrap().iter().find(|c| c["name"] == "room").unwrap();
    assert_eq!(room["member_names"], serde_json::json!([format!("{my_name} (you)"), "aaron (attend-chat)"]));

    let peers = f.ok(&["peers"]);
    assert!(peers.contains(&format!("#room: {expected}")), "{peers}");
    assert_eq!(me["channels"][0]["members"], serde_json::json!([format!("{my_name} (you)"), "aaron (attend-chat)"]));
}

/// A channel this session is not in shows its count, not its members' names
/// (review of #927, finding 4).
#[test]
fn a_channel_this_session_is_not_in_shows_only_its_count() {
    let f = Fixture::new("channel-members-other");
    f.ok(&["join", "room"]);
    alive(&f, &f.sid);
    human_in(&f, "elsewhere");
    let listing = f.ok(&["channels"]);
    assert!(!listing.contains("#elsewhere:"), "{listing}");
    assert!(!listing.contains("aaron"), "{listing}");
    let channels: Value = serde_json::from_str(&f.ok(&["channels", "--json"])).unwrap();
    let other = channels.as_array().unwrap().iter().find(|c| c["name"] == "elsewhere").unwrap();
    assert_eq!(other["members"], 1);
    assert_eq!(other["member_names"], serde_json::json!([]));
}

/// `attend peers` reads without pruning, and leaves out a member whose
/// session is gone, as `attend channels` does after its cleanup (review of
/// #927, finding 3).
#[test]
fn peers_leaves_out_a_member_whose_session_is_gone() {
    let f = Fixture::new("channel-members-stale");
    f.ok(&["join", "room"]);
    alive(&f, &f.sid);
    human_in(&f, "room");
    // A member that joined and exited without leaving: no heartbeat.
    attend_groups::Groups::new(&f.signals(), "11111111-2222-3333-4444-555555555555").join("room", false).unwrap();
    let peers = f.ok(&["peers"]);
    let line = peers.lines().find(|l| l.contains("#room:")).unwrap_or_else(|| panic!("{peers}"));
    assert!(!line.contains("session 11111111"), "{line}");
    assert!(line.contains("aaron (attend-chat)"), "{line}");
    let groups = std::fs::read_to_string(f.signals().join("_groups.yaml")).unwrap();
    assert!(groups.contains("11111111-2222-3333-4444-555555555555"), "peers wrote nothing: {groups}");
}

/// A message from `aaron` in attend-chat, said in `room`.
#[cfg(feature = "sensor-peers")]
fn human_says(f: &Fixture, room: &str, id: &str, body: &str) {
    let dir = f.signals().join(format!("@{room}"));
    std::fs::create_dir_all(&dir).unwrap();
    let line = agent_identity::signal::format_signal("external:aaron@kitty", "aaron", "/home/aaron", None, body);
    std::fs::write(dir.join(format!("{id}.signal")), line).unwrap();
}

/// The agent is the only Claude session in the channel and the operator is
/// in it from attend-chat: the agent's send reaches the operator, and
/// `attend channels` names the operator as a person in attend-chat.
#[test]
fn a_send_to_a_channel_whose_other_member_is_a_live_human_goes_out() {
    let f = Fixture::new("channel-members-human-send");
    f.ok(&["join", "room"]);
    alive(&f, &f.sid);
    human_in(&f, "room");
    let out = f.attend(&["send", "--channel", "room", "--json", "--", "for the operator"]);
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let listing = f.ok(&["channels"]);
    assert!(listing.contains("aaron (attend-chat)"), "{listing}");
}

/// The observed case (attend 0.21.0): the operator posts in the channel from
/// attend-chat, and the agent's `attend reply` to it is said in the channel.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_reply_to_a_human_in_the_channel_goes_out() {
    let f = Fixture::new("channel-members-human-reply");
    f.ok(&["join", "room"]);
    alive(&f, &f.sid);
    f.drain("plain");
    human_in(&f, "room");
    human_says(&f, "room", "aaron-kitty-1-1", "can you see me");
    assert!(f.drain("plain").contains("can you see me"));
    let out = f.attend(&["reply", "--json", "yes"]);
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let sent: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sent["scope"], "channel");
    assert_eq!(sent["re"], "aaron-kitty-1-1");
    let id = sent["id"].as_str().unwrap();
    assert!(f.signals().join("@room").join(format!("{id}.signal")).is_file(), "said in the channel");
    let listing = f.ok(&["channels"]);
    assert!(listing.contains("aaron (attend-chat)"), "{listing}");
}

/// With nobody else in the channel the refusal stands: a person whose chat
/// is closed, so not a live member, is no listener.
#[cfg(feature = "sensor-peers")]
#[test]
fn a_reply_into_a_channel_with_nobody_else_is_refused() {
    let f = Fixture::new("channel-members-alone");
    f.ok(&["join", "room"]);
    alive(&f, &f.sid);
    f.drain("plain");
    human_says(&f, "room", "aaron-kitty-1-1", "anyone here");
    assert!(f.drain("plain").contains("anyone here"));
    let out = f.attend(&["reply", "--json", "only me"]);
    assert!(!out.status.success());
    let err: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(err["error"]["kind"], "no_live_peers", "{err}");
    assert_eq!(err["error"]["only_self"], true, "{err}");
}
