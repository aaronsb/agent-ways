//! Each channel's members under its row in the left column (ADR-405,
//! amendment of 2026-10-15): participating members drawn as their Agents
//! rows are, invited ones set back and marked `◌ invited`; a member row
//! opens its agent's menu; Add agent enrols and Invite agent asks. In its
//! own file, so its own fixture: three live claudes with sessions records,
//! #deploy with beta and the operator in it (beta invited there too), #infra
//! with beta in it and gamma invited, and #ops, which the one test that
//! writes changes.

mod support;

use agent_theme::ColorDepth;
use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use agent_tui::testkit;
use agent_tui::theme::{Palette, Shape};
use attend_chat::app::Chat;

const ALPHA_SID: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const BETA_SID: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const GAMMA_SID: &str = "cccccccc-0000-4000-8000-000000000003";
/// Invited to #infra, it exited without listening: no session record, no
/// heartbeat.
const GONE_SID: &str = "dddddddd-0000-4000-8000-000000000004";
const W: u16 = 120;
const H: u16 = 35;

fn chat(dry: bool) -> Chat {
    support::fixture("members", |base| {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        for (i, (sid, name)) in [(ALPHA_SID, "alpha"), (BETA_SID, "beta"), (GAMMA_SID, "gamma")].into_iter().enumerate() {
            support::pin_alive(sid);
            let work = home.join("work").join(name);
            std::fs::create_dir_all(&work).unwrap();
            let record = format!("{{\"pid\":1,\"sessionId\":\"{sid}\",\"cwd\":\"{}\"}}\n", work.display());
            std::fs::write(home.join(format!(".claude/sessions/{i}.json")), record).unwrap();
        }
        let none = attend_groups::Groups::new(base, "");
        for c in ["deploy", "infra", "ops"] {
            none.create(c, None).unwrap();
        }
        let beta = attend_groups::Groups::new(base, BETA_SID);
        beta.join("deploy", true).unwrap();
        beta.join("infra", true).unwrap();
        attend_groups::Groups::new(base, &attend_chat::signal::human_member_id()).join("deploy", true).unwrap();
        // gamma invited to #infra four minutes ago; beta invited to #deploy,
        // where it is already, which draws it as participating.
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 240;
        std::fs::write(base.join(format!("@infra/{GAMMA_SID}.invite")), format!("by: aaron\nat: {at}\n")).unwrap();
        std::fs::write(base.join(format!("@deploy/{BETA_SID}.invite")), format!("by: aaron\nat: {at}\n")).unwrap();
        std::fs::write(base.join(format!("@infra/{GONE_SID}.invite")), format!("by: aaron\nat: {at}\n")).unwrap();
    });
    Chat::new(None, Palette::default(), Shape::PLAIN).sidebar(true).heartbeat(false).enhanced(true).dry_run(dry)
}

/// The nickname the chat gives the claude working in `~/work/<project>`.
fn name(project: &str) -> String {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    agent_identity::Identity::for_cwd(&home.join("work").join(project).to_string_lossy(), ColorDepth::Ansi16).nickname.to_string()
}

fn rows(c: &mut Chat) -> Vec<String> {
    testkit::rows(&testkit::render_screen(c, W, H))
}

/// The left column's rows: each frame row up to the column's rule.
fn column(c: &mut Chat) -> Vec<String> {
    rows(c).into_iter().map(|r| r.split('│').next().unwrap_or("").to_string()).collect()
}

/// The rows under channel `channel`'s row, up to the next channel's.
fn under(c: &mut Chat, channel: &str) -> Vec<String> {
    let col = column(c);
    let at = col.iter().position(|r| r.contains(&format!("#{channel} ")) || r.trim_end().ends_with(&format!("#{channel}"))).unwrap_or_else(|| panic!("no #{channel} row: {col:#?}"));
    col[at + 1..].iter().take_while(|r| !r.contains('#') && !r.trim().is_empty()).cloned().collect()
}

fn click(c: &mut Chat, (x, y): (u16, u16), button: MouseButton) {
    testkit::render_screen(c, W, H);
    c.mouse(MouseEvent { kind: MouseEventKind::Down(button), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// The cell of the row under `channel` that names `who`.
fn member_cell(c: &mut Chat, channel: &str, who: &str) -> (u16, u16) {
    let col = column(c);
    let at = col.iter().position(|r| r.contains(&format!("#{channel}"))).unwrap();
    let (y, r) = col.iter().enumerate().skip(at + 1).find(|(_, r)| r.contains(who)).unwrap_or_else(|| panic!("no {who} under #{channel}: {col:#?}"));
    (r[..r.find(who).unwrap()].chars().count() as u16, y as u16)
}

/// `line` typed into the compose box, then Enter.
fn enter(c: &mut Chat, line: &str) {
    testkit::render_screen(c, W, H);
    let mut keys: Vec<KeyEvent> = line.chars().map(|ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)).collect();
    keys.push(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    testkit::drive(c, &keys);
}

fn press(c: &mut Chat, code: KeyCode) {
    testkit::render_screen(c, W, H);
    testkit::drive(c, &[KeyEvent::new(code, KeyModifiers::NONE)]);
}

/// The items of the open menu titled `pick one: <title>`.
fn picker(c: &mut Chat, title: &str) -> Vec<String> {
    let shown = rows(c);
    let head = format!("pick one: {title}");
    let (y, r) = shown.iter().enumerate().find(|(_, r)| r.contains(&head)).unwrap_or_else(|| panic!("no {head:?}: {shown:#?}"));
    let x = r[..r.find(&head).unwrap()].chars().count().saturating_sub(1);
    shown[y + 1..].iter().take_while(|r| r.chars().nth(x) != Some('└')).map(|r| r.chars().skip(x + 1).take_while(|ch| *ch != '│').collect::<String>().trim().to_string()).collect()
}

/// The channel menu of `channel`, by a right click on its row.
fn channel_menu(c: &mut Chat, channel: &str) {
    let col = column(c);
    let (y, r) = col.iter().enumerate().find(|(_, r)| r.contains(&format!("#{channel}"))).unwrap();
    let x = r[..r.find('#').unwrap()].chars().count() as u16;
    click(c, (x, y as u16), MouseButton::Right);
}

/// A click on `item` in the menu open: the first `item` below the title
/// of the newest menu drawn, right of its left edge, where a row of the
/// column beside it can name the same agent.
fn pick(c: &mut Chat, item: &str) {
    let shown = rows(c);
    let (top, left) = shown.iter().enumerate().rev().find_map(|(y, r)| r.find("pick one:").map(|x| (y, r[..x].chars().count()))).unwrap_or_else(|| panic!("no menu open: {shown:#?}"));
    let hit = shown.iter().enumerate().skip(top + 1).find_map(|(y, r)| {
        let line: String = r.chars().skip(left).take_while(|ch| *ch != '│').collect();
        line.find(item).map(|x| ((left + line[..x].chars().count()) as u16, y as u16))
    });
    click(c, hit.unwrap_or_else(|| panic!("no {item:?} in the menu: {shown:#?}")), MouseButton::Left);
}

#[test]
fn every_channel_lists_its_members_under_its_row_invited_ones_marked_apart() {
    let mut c = chat(true);
    let (beta, gamma) = (name("beta"), name("gamma"));
    let deploy = under(&mut c, "deploy");
    assert!(deploy.iter().any(|r| r.contains(&beta) && r.contains('●') && !r.contains("invited")), "beta participates in #deploy: {deploy:#?}");
    assert!(deploy.iter().any(|r| r.contains("aaron")), "the operator, a human member, is listed: {deploy:#?}");
    assert_eq!(deploy.iter().filter(|r| r.contains(&beta)).count(), 1, "beta invited where it already is: listed once, participating: {deploy:#?}");
    let infra = under(&mut c, "infra");
    assert!(infra.iter().any(|r| r.contains(&beta) && !r.contains("invited")), "beta is in two channels and listed under both: {infra:#?}");
    let invited = infra.iter().find(|r| r.contains(&gamma)).unwrap_or_else(|| panic!("gamma under #infra: {infra:#?}"));
    assert!(invited.contains("◌ invited") && !invited.contains('●'), "invited, marked apart: {invited}");
    assert!(invited.contains("4m"), "with the invitation's age: {invited}");
    assert!(under(&mut c, "ops").is_empty(), "an empty channel has no rows under it");
    let agents = column(&mut c);
    assert!(agents.iter().any(|r| r.contains(&name("alpha"))), "an agent in no channel keeps its Agents row");
}

#[test]
fn a_member_row_opens_its_agents_menu_by_a_click_or_enter() {
    let mut c = chat(true);
    let gamma = name("gamma");
    let at = member_cell(&mut c, "infra", &gamma);
    click(&mut c, at, MouseButton::Left);
    let shown = rows(&mut c);
    assert!(shown.iter().any(|r| r.contains(&format!("pick one: @{gamma}"))), "the click opens gamma's menu: {shown:#?}");
    let title = shown.iter().position(|r| r.contains("pick one:")).unwrap();
    assert_eq!(title as u16, at.1 + 1, "docked on the line under the member row: {shown:#?}");
    press(&mut c, KeyCode::Esc);
    // The keyboard is on the row clicked; Enter opens the same menu.
    press(&mut c, KeyCode::Enter);
    assert!(rows(&mut c).iter().any(|r| r.contains(&format!("pick one: @{gamma}"))), "{:#?}", rows(&mut c));
    pick(&mut c, "Who is this");
    assert!(c.status().contains("invited to #infra by aaron"), "Who is this names the inviter: {}", c.status());
}

/// The column's refresh prunes the invitation of a session that has gone,
/// so it is not listed as invited.
#[test]
fn an_invitation_to_a_session_that_has_gone_is_pruned_and_not_listed() {
    let mut c = chat(true);
    let infra = under(&mut c, "infra");
    assert!(!infra.iter().any(|r| r.contains("dddddddd")), "{infra:#?}");
    let invite = attend_chat::signal::signals_base().join(format!("@infra/{GONE_SID}.invite"));
    assert!(!invite.exists(), "the invitation is pruned");
}

#[test]
fn add_offers_invited_agents_and_invite_leaves_them_out() {
    let mut c = chat(true);
    let (alpha, beta, gamma) = (name("alpha"), name("beta"), name("gamma"));
    channel_menu(&mut c, "infra");
    let items = picker(&mut c, "#infra");
    assert!(items.iter().any(|i| i == "Add agent ▸") && items.iter().any(|i| i == "Invite agent ▸"), "{items:#?}");
    pick(&mut c, "Invite agent ▸");
    let invite = picker(&mut c, "invite to #infra");
    assert!(invite.iter().any(|i| i.starts_with(&alpha)), "{invite:#?}");
    assert!(!invite.iter().any(|i| i.starts_with(&gamma) || i.starts_with(&beta)), "neither the invited agent nor a member: {invite:#?}");
    press(&mut c, KeyCode::Esc);
    press(&mut c, KeyCode::Esc);
    channel_menu(&mut c, "infra");
    pick(&mut c, "Add agent ▸");
    let add = picker(&mut c, "add to #infra");
    assert!(add.iter().any(|i| i.starts_with(&alpha)) && add.iter().any(|i| i.starts_with(&gamma)), "adding an invited agent spends its invitation: {add:#?}");
    assert!(!add.iter().any(|i| i.starts_with(&beta)), "a member is not offered: {add:#?}");
    pick(&mut c, &format!("{gamma} "));
    assert_eq!(c.status(), "dry run: nothing added", "the pick adds; a dry run writes nothing");
}

/// The live paths, in one test since they change the fixture: Invite agent
/// writes an invitation the column then shows as invited, and Add agent to
/// chat enrols the agent, which the column then shows as participating.
#[test]
fn invite_then_add_for_real() {
    let mut c = chat(false);
    let alpha = name("alpha");
    channel_menu(&mut c, "ops");
    pick(&mut c, "Invite agent ▸");
    pick(&mut c, &alpha);
    assert_eq!(c.status(), format!("invited @{alpha} to #ops — they join themselves"));
    c.refresh();
    let ops = under(&mut c, "ops");
    assert!(ops.iter().any(|r| r.contains(&alpha) && r.contains("◌ invited")), "{ops:#?}");
    // Inviting it again renews the invitation, which is no error.
    enter(&mut c, &format!("/invite @{alpha} #ops"));
    assert_eq!(c.status(), format!("@{alpha} was invited to #ops already; the invitation now dates from now"));
    assert!(rows(&mut c).iter().all(|r| !r.contains("/invite")), "the command is done, the compose box cleared");
    // From alpha's own menu: Add agent to chat ▸, then #ops.
    let (x, y) = member_cell(&mut c, "ops", &alpha);
    click(&mut c, (x, y), MouseButton::Left);
    pick(&mut c, "Add agent to chat ▸");
    let title = rows(&mut c).iter().position(|r| r.contains("pick one: add to channel")).unwrap();
    assert_eq!(title as u16, y + 1, "the picker docks under the row its menu came from: {:#?}", rows(&mut c));
    let channels = picker(&mut c, "add to channel");
    assert!(channels.iter().any(|i| i == "#ops"), "invited, not a member: #ops is offered: {channels:#?}");
    pick(&mut c, "#ops");
    assert_eq!(c.status(), format!("added @{alpha} to #ops"));
    let yaml = std::fs::read_to_string(attend_chat::signal::signals_base().join("_groups.yaml")).unwrap();
    let groups: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    assert!(groups["ops"]["members"].as_sequence().unwrap().iter().any(|m| m.as_str() == Some(ALPHA_SID)), "{yaml}");
    c.refresh();
    let ops = under(&mut c, "ops");
    assert!(ops.iter().any(|r| r.contains(&alpha) && r.contains('●') && !r.contains("invited")), "participating now: {ops:#?}");
    channel_menu(&mut c, "ops");
    pick(&mut c, "Add agent ▸");
    let add = picker(&mut c, "add to #ops");
    assert!(!add.iter().any(|i| i.starts_with(&alpha)), "a member is not offered again: {add:#?}");
}
