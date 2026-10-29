//! The module card as the detail pane heads a node with it.

use ratatui::style::Modifier;
use ratatui::text::Line;

use super::card::{card_lines, origin_style};
use super::theme;
use crate::tree::{Card, Origin, StateTone};

fn card(at: Option<&str>) -> Card {
    Card {
        title: "downloads".into(),
        summary: "a sensor of your own".into(),
        origin: Origin::User,
        at: at.map(str::to_string),
        kind: Some("script".into()),
        source: "~/.local/share/attend/sensors/dl.sh".into(),
        version: None,
        state: ("enabled".into(), StateTone::Ok),
        rows: vec![("interval".into(), "60s quiet, 15s busy".into())],
    }
}

fn text(lines: &[Line]) -> Vec<String> {
    lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
}

#[test]
fn the_card_lists_its_rows_in_order_under_one_label_column() {
    let lines = card_lines(&card(Some("~/.config/attend/config.yaml")), 60);
    assert_eq!(
        text(&lines),
        [
            "downloads",
            "a sensor of your own",
            "",
            "origin    user · ~/.config/attend/config.yaml",
            "kind      script",
            "source    ~/.local/share/attend/sensors/dl.sh",
            "state     enabled",
            "interval  60s quiet, 15s busy",
        ]
    );
    // Only the origin word takes the origin's role; the location is plain.
    let origin = &lines[3].spans;
    assert_eq!((origin[1].content.as_ref(), origin[1].style), ("user", origin_style(Origin::User)));
    assert_eq!(origin[2].content.as_ref(), " · ~/.config/attend/config.yaml");
    assert!(lines[0].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn builtin_and_user_take_distinct_roles() {
    assert_ne!(origin_style(Origin::Builtin), origin_style(Origin::User));
    assert_eq!(origin_style(Origin::Builtin), theme::info());
}

#[test]
fn a_card_with_no_version_or_kind_leaves_their_rows_out() {
    let c = Card { kind: None, version: None, ..card(None) };
    let lines = text(&card_lines(&c, 60));
    assert_eq!(lines[3], "origin    user");
    assert!(!lines.iter().any(|l| l.starts_with("kind") || l.starts_with("version")), "{lines:?}");
    let c = Card { version: Some("0.6.0".into()), ..card(None) };
    assert!(text(&card_lines(&c, 60)).contains(&"version   0.6.0".to_string()));
}

#[test]
fn a_long_origin_wraps_under_the_value_column() {
    let lines = text(&card_lines(&card(Some("~/a/very/long/path/to/config.yaml")), 20));
    assert_eq!(lines[3], "origin    user · ~/a/very/long");
    assert_eq!(lines[4], "          /path/to/config.yaml");
}
