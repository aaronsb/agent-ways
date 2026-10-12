//! A member can see who else is in a channel before anyone posts (the
//! attend mod's clean-slate test, finding 5): `attend channels` and `attend
//! peers` name each channel's members, through the CLI the attend_peers
//! tool runs.

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
    let expected = format!("{my_name} (you), aaron");

    let listing = f.ok(&["channels"]);
    assert!(listing.contains(&format!("#room: {expected}")), "{listing}");
    let channels: Value = serde_json::from_str(&f.ok(&["channels", "--json"])).unwrap();
    let room = channels.as_array().unwrap().iter().find(|c| c["name"] == "room").unwrap();
    assert_eq!(room["member_names"], serde_json::json!([format!("{my_name} (you)"), "aaron"]));

    let peers = f.ok(&["peers"]);
    assert!(peers.contains(&format!("#room: {expected}")), "{peers}");
    assert_eq!(me["channels"][0]["members"], serde_json::json!([format!("{my_name} (you)"), "aaron"]));
}
