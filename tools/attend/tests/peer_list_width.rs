//! `attend peer list` on a terminal narrower than its table (#1117). The
//! Agent column is pinned: its labels were already shortened to fit, so the
//! table narrows the other columns and leaves every label whole.

mod common;
use common::Fixture;
use serde_json::Value;
use std::ffi::OsStr;

/// A channel name long enough that the table cannot fit a narrow terminal.
const CHANNEL: &str = "a-channel-name-long-enough-to-crowd-the-table";

/// `attend peer list` as a terminal `cols` wide would show it, uncoloured.
fn peer_list_at(f: &Fixture, cols: usize) -> String {
    let cols = cols.to_string();
    let out = f.attend_with(&["peer", "list"], &[("COLUMNS", OsStr::new(&cols)), ("NO_COLOR", OsStr::new("1"))]);
    assert!(out.status.success(), "attend peer list: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The table row whose Agent cell is `(you)`: the channel this session joined.
fn channel_row(listing: &str) -> &str {
    listing.lines().find(|l| l.contains(" (you)") && !l.trim_start().starts_with('#')).unwrap_or_else(|| panic!("no channel row:\n{listing}"))
}

/// The self row, whose Focus cell is `(self)`.
fn self_row(listing: &str) -> &str {
    listing.lines().find(|l| l.trim_start().starts_with("(self)")).unwrap_or_else(|| panic!("no self row:\n{listing}"))
}

#[test]
fn a_narrow_terminal_truncates_focus_and_keeps_the_agent_label_whole() {
    let f = Fixture::new("peer-list-narrow");
    f.ok(&["channel", "join", CHANNEL]);
    let me: Value = serde_json::from_str(&f.ok(&["peer", "list", "--json"])).unwrap();
    let label = format!("{} (proj)", me["self"]["name"].as_str().unwrap());

    // Control: a wide terminal shows both cells whole.
    let wide = peer_list_at(&f, 200);
    assert!(channel_row(&wide).contains(CHANNEL), "{wide}");
    assert!(self_row(&wide).contains(&label), "{wide}");

    // Below the natural width: indent, the four columns at their widest
    // cells (Status and Context at least their headers), and the gaps.
    let cols = 60;
    let natural = 2 + CHANNEL.len() + label.chars().count() + "Status".len() + "Context".len() + 3;
    assert!(natural > cols, "the table must not fit: natural {natural}, terminal {cols}");

    let narrow = peer_list_at(&f, cols);
    let row = channel_row(&narrow);
    assert!(!row.contains(CHANNEL) && row.contains('…'), "Focus should truncate:\n{narrow}");
    assert!(self_row(&narrow).contains(&label), "the Agent label should stay whole:\n{narrow}");
    assert!(row.chars().count() <= cols, "the row should fit {cols} columns:\n{narrow}");
}
