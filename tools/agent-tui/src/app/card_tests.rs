//! The item card as the detail pane heads a node with it.

use agent_settings::item::{Family, How, Item, ItemKind, Origin, Override, SensorState, ThemeState};
use ratatui::style::Modifier;
use ratatui::text::Line;

use super::card::{card_lines, origin_style};
use super::theme;
use crate::tree::Card;

fn card(path: Option<&str>) -> Card {
    Card {
        title: "downloads".into(),
        summary: "a sensor of your own".into(),
        item: Item {
            kind: Some(ItemKind::Command),
            path: path.map(Into::into),
            source: Some("~/bin/dl.sh".into()),
            state: Some(SensorState::Active.into()),
            ..Item::new(Family::Sensor, "downloads", Origin::User)
        },
        fold_state: false,
        rows: vec![("interval".into(), "60s quiet, 15s busy".into())],
    }
}

fn text(lines: &[Line]) -> Vec<String> {
    lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
}

#[test]
fn the_card_lists_its_rows_in_order_under_one_label_column() {
    let mut c = card(Some("~/.config/attend/sensors/downloads.yaml"));
    c.item.version = Some("1.2".into());
    c.item.detail = Some("polled 3 s ago".into());
    let lines = card_lines(&c, 60);
    assert_eq!(
        text(&lines),
        [
            "downloads",
            "a sensor of your own",
            "",
            "origin    user · ~/.config/attend/sensors/downloads.yaml",
            "kind      command",
            "source    ~/bin/dl.sh",
            "version   1.2",
            "state     active · polled 3 s ago",
            "interval  60s quiet, 15s busy",
        ]
    );
    // Only the origin word takes the origin's role; the location is plain.
    let origin = &lines[3].spans;
    assert_eq!((origin[1].content.as_ref(), origin[1].style), ("user", origin_style(Origin::User)));
    assert_eq!(origin[2].content.as_ref(), " · ~/.config/attend/sensors/downloads.yaml");
    // The state word takes its family's tone; the detail is plain.
    let state = &lines[7].spans;
    assert_eq!((state[1].content.as_ref(), state[1].style), ("active", theme::ok()));
    assert_eq!(state[2].content.as_ref(), " · polled 3 s ago");
    assert!(lines[0].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn builtin_and_what_was_added_take_distinct_roles() {
    assert_ne!(origin_style(Origin::Builtin), origin_style(Origin::User));
    assert_eq!(origin_style(Origin::User), origin_style(Origin::Project));
    assert_eq!(origin_style(Origin::Builtin), theme::info());
    assert!(origin_style(Origin::Project).add_modifier.contains(Modifier::BOLD));
}

#[test]
fn each_override_follows_the_origin_on_a_row_of_its_own() {
    let mut c = card(None);
    c.item.origin = Origin::Builtin;
    c.item.overrides = vec![
        Override { origin: Origin::User, how: How::Extends, path: "~/.config/attend/sensors/git.yaml".into() },
        Override { origin: Origin::Project, how: How::Replaces, path: "~/proj/.claude/attend/sensors/git.yaml".into() },
    ];
    let lines = card_lines(&c, 60);
    let t = text(&lines);
    assert_eq!(&t[3..7], ["origin    builtin", "override  user extends · ~/.config/attend/sensors/git.yaml", "override  project replaces · ~/proj/.claude/attend/sensors/git.yaml", "kind      command"]);
    assert_eq!((lines[3].spans[1].content.as_ref(), lines[3].spans[1].style), ("builtin", theme::info()));
    assert_eq!((lines[4].spans[1].content.as_ref(), lines[4].spans[1].style), ("user", origin_style(Origin::User)));
}

#[test]
fn a_null_field_draws_no_row() {
    let mut c = card(None);
    c.item.kind = None;
    c.item.source = None;
    c.item.state = None;
    let lines = text(&card_lines(&c, 60));
    assert_eq!(lines[3], "origin    user");
    assert!(!lines.iter().any(|l| ["kind", "source", "version", "state", "override"].iter().any(|r| l.starts_with(r))), "{lines:?}");
    // A state not yet known shows what is known of it.
    c.item.detail = Some("reading…".into());
    assert!(text(&card_lines(&c, 60)).contains(&"state     reading…".to_string()));
}

#[test]
fn a_folded_state_takes_its_own_row_once_an_override_follows_the_origin() {
    let mut c = card(None);
    c.fold_state = true;
    c.item = Item::new(Family::Theme, "nord", Origin::Builtin).with_state(ThemeState::Active);
    let lines = card_lines(&c, 60);
    assert_eq!(text(&lines)[3], "origin    builtin · active");
    c.item.overrides = vec![Override { origin: Origin::User, how: How::Replaces, path: "~/t/nord.theme".into() }];
    let lines = card_lines(&c, 60);
    let t = text(&lines);
    assert_eq!(&t[3..6], ["origin    builtin", "override  user replaces · ~/t/nord.theme", "state     active"]);
    let state = &lines[5].spans[1];
    assert_eq!((state.content.as_ref(), state.style), ("active", theme::ok()));
}

#[test]
fn a_folded_state_that_would_overflow_takes_its_own_row() {
    let mut c = card(None);
    c.fold_state = true;
    c.item.kind = None;
    c.item.detail = Some("Enter uses it".into());
    let lines = text(&card_lines(&c, 20));
    assert_eq!(lines[3], "origin    user");
    let at = lines.iter().position(|l| l.starts_with("state")).expect("a state row");
    assert_eq!(&lines[at..at + 2], ["state     active · Enter uses ", "          it"]);
}

#[test]
fn a_long_origin_wraps_under_the_value_column() {
    let lines = text(&card_lines(&card(Some("~/a/very/long/path/to/config.yaml")), 20));
    assert_eq!(lines[3], "origin    user · ~/a/very/long");
    assert_eq!(lines[4], "          /path/to/config.yaml");
}
