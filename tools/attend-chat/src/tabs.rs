//! Tab model for the channel strip (#393, ADR-173 Decision 6).
//!
//! Channels are tabs: `[merged] [#open] [#g] …` with *merged* pinned
//! at slot zero. Per-channel tabs show a filtered transcript (the
//! strict IRC prior); merged keeps today's full cross-channel stream
//! (ambient awareness). Slash commands with no channel argument scope
//! to the foreground tab — the `/part`-in-the-current-window
//! convention — while explicit arguments work from anywhere.
//!
//! Everything except the strip render is a pure function of the tab +
//! the ordered channel-name list, so cycling, dissolve-advance, and
//! scope resolution are testable without an iocraft `App`.

use agent_identity::TermCaps;
use iocraft::prelude::*;

use crate::chip::color_for;
use crate::groups::{KnownGroup, BASE_CHANNEL_NAME};
use crate::signal::Channel;

/// The foreground tab. `Merged` is not a channel — it has no send
/// target of its own (typing there defaults to `#open`) and cannot be
/// closed; channel lifecycle only ever touches `Channel` tabs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Merged,
    Channel(String),
}

/// Channel names in strip order, taken from the discovered channel
/// list — `channels()` guarantees `open` leads, groups follow
/// alphabetically. The name list (not `KnownGroup`) is the currency
/// of every pure function here.
pub fn strip_names(known: &[KnownGroup]) -> Vec<String> {
    known.iter().map(|k| k.group.name.clone()).collect()
}

/// A foreground tab whose channel vanished (dissolved by a peer,
/// swept) falls to the floor `#open` rather than pointing at a tab
/// the strip no longer renders. Merged and live channels pass
/// through untouched.
pub fn normalize(tab: Tab, names: &[String]) -> Tab {
    match tab {
        Tab::Channel(g) if !names.iter().any(|n| n == &g) => {
            Tab::Channel(BASE_CHANNEL_NAME.to_string())
        }
        t => t,
    }
}

/// Tab-key cycle: merged → #open → #g → … → wrap to merged.
pub fn cycle_next(current: &Tab, names: &[String]) -> Tab {
    match current {
        Tab::Merged => names
            .first()
            .map(|n| Tab::Channel(n.clone()))
            .unwrap_or(Tab::Merged),
        Tab::Channel(g) => match names.iter().position(|n| n == g) {
            Some(i) if i + 1 < names.len() => Tab::Channel(names[i + 1].clone()),
            _ => Tab::Merged,
        },
    }
}

/// Alt+N direct jump (IRC prior): 1 = merged, 2 = #open, 3.. = named
/// channels in strip order. `None` for an empty slot — the keypress
/// is a no-op rather than a wrap.
pub fn jump(slot: u32, names: &[String]) -> Option<Tab> {
    match slot {
        0 => None,
        1 => Some(Tab::Merged),
        n => names
            .get(n as usize - 2)
            .map(|g| Tab::Channel(g.clone())),
    }
}

/// The channel an unaddressed send targets from `tab`. Merged targets
/// the base channel — least surprise (today's behavior), made visible
/// by the destination flag (#392).
pub fn send_scope(tab: &Tab) -> String {
    match tab {
        Tab::Merged => BASE_CHANNEL_NAME.to_string(),
        Tab::Channel(g) => g.clone(),
    }
}

/// Channel-argument resolution for slash commands: an explicit
/// argument wins from any tab; no argument scopes to the foreground
/// tab.
pub fn command_scope(explicit: Option<String>, tab: &Tab) -> String {
    explicit.unwrap_or_else(|| send_scope(tab))
}

/// Focus after `/dissolve <g>` closed the foreground tab: the tab now
/// occupying the dissolved tab's slot, clamped to the end of the
/// strip. `#open` — always present at the head of the channel run —
/// is the floor: with no named channels left, focus lands there.
pub fn advance_after_dissolve(dissolved: &str, names_before: &[String]) -> Tab {
    let idx = names_before
        .iter()
        .position(|n| n == dissolved)
        .unwrap_or(0);
    let after: Vec<&String> = names_before
        .iter()
        .filter(|n| n.as_str() != dissolved)
        .collect();
    match after.get(idx.min(after.len().saturating_sub(1))) {
        Some(n) => Tab::Channel((*n).clone()),
        None => Tab::Channel(BASE_CHANNEL_NAME.to_string()),
    }
}

/// Does a signal that arrived on `channel` render in `tab`'s
/// transcript? Merged shows everything; a channel tab shows only its
/// own traffic plus local `Info` blocks (command feedback must be
/// visible wherever the command was typed). Directed messages surface
/// in merged only — the strict per-channel filter; a DM tab is out of
/// scope (#393).
pub fn visible_in(channel: &Channel, tab: &Tab) -> bool {
    match tab {
        Tab::Merged => true,
        Tab::Channel(g) => match channel {
            Channel::Info => true,
            Channel::Open => g == BASE_CHANNEL_NAME,
            Channel::Group(name) => name == g,
            Channel::Direct => false,
        },
    }
}

/// Render the tab strip: `[merged] [#open] [#g] …`. The foreground
/// tab renders inverse — its own channel color as background, black
/// text — so the active tab reads at a glance; merged wears a neutral
/// grey (it is a view, not a channel, and carries no hashed
/// identity). Underline-on-partial keeps the `#partial` completion
/// affordance the channel bar had.
pub fn tab_strip_row(
    known: &[KnownGroup],
    foreground: &Tab,
    current_partial: Option<&str>,
) -> Vec<AnyElement<'static>> {
    let caps = TermCaps::detect();
    let lc_partial = current_partial.map(|p| p.to_ascii_lowercase());
    let mut chips: Vec<AnyElement<'static>> = Vec::with_capacity(known.len() + 1);

    if matches!(foreground, Tab::Merged) {
        chips.push(
            element! {
                View(background_color: Color::Grey) {
                    Text(color: Color::Black, content: " merged ", wrap: TextWrap::NoWrap)
                }
            }
            .into_any(),
        );
    } else {
        chips.push(
            element! {
                Text(color: Color::Grey, content: " merged ", wrap: TextWrap::NoWrap)
            }
            .into_any(),
        );
    }

    for k in known {
        let active = matches!(foreground, Tab::Channel(g) if *g == k.group.name);
        let matches = lc_partial
            .as_ref()
            .map(|p| !p.is_empty() && k.group.name.to_ascii_lowercase().starts_with(p))
            .unwrap_or(false);
        let color = color_for(k.group.palette, caps);
        // Base channel keeps its always-bold commons affordance
        // (ADR-124 §4); partial-match underline overlays either way.
        let weight = if k.is_base || k.group.style.bold || matches {
            Weight::Bold
        } else {
            Weight::Normal
        };
        let decoration = if matches {
            TextDecoration::Underline
        } else {
            TextDecoration::None
        };
        let content = format!(" {} #{} ", k.group.glyph, k.group.name);
        chips.push(if active {
            element! {
                View(background_color: color) {
                    Text(color: Color::Black, weight, decoration, content, wrap: TextWrap::NoWrap)
                }
            }
            .into_any()
        } else {
            element! {
                Text(
                    color,
                    weight,
                    italic: k.group.style.italic,
                    decoration,
                    content,
                    wrap: TextWrap::NoWrap,
                )
            }
            .into_any()
        });
    }

    vec![element! {
        View(
            flex_direction: FlexDirection::Row,
            padding_left: 1,
            padding_right: 1,
            height: 1u32,
            flex_shrink: 0.0,
            overflow: Overflow::Hidden,
        ) {
            #(chips)
        }
    }
    .into_any()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn chan(g: &str) -> Tab {
        Tab::Channel(g.to_string())
    }

    #[test]
    fn cycle_walks_merged_then_channels_and_wraps() {
        let n = names(&["open", "deploy", "infra"]);
        assert_eq!(cycle_next(&Tab::Merged, &n), chan("open"));
        assert_eq!(cycle_next(&chan("open"), &n), chan("deploy"));
        assert_eq!(cycle_next(&chan("deploy"), &n), chan("infra"));
        assert_eq!(cycle_next(&chan("infra"), &n), Tab::Merged);
    }

    #[test]
    fn cycle_with_no_channels_stays_on_merged() {
        // Can't happen live (`#open` is always in the strip) but the
        // function must not panic on an empty list.
        assert_eq!(cycle_next(&Tab::Merged, &[]), Tab::Merged);
    }

    #[test]
    fn cycle_from_vanished_channel_wraps_to_merged() {
        // Foreground channel dissolved between renders: position()
        // misses, so the cycle lands on merged rather than panicking.
        let n = names(&["open", "deploy"]);
        assert_eq!(cycle_next(&chan("ghost"), &n), Tab::Merged);
    }

    #[test]
    fn jump_slots_match_strip_order() {
        let n = names(&["open", "deploy"]);
        assert_eq!(jump(1, &n), Some(Tab::Merged));
        assert_eq!(jump(2, &n), Some(chan("open")));
        assert_eq!(jump(3, &n), Some(chan("deploy")));
        // Empty slot and slot zero are no-ops.
        assert_eq!(jump(4, &n), None);
        assert_eq!(jump(0, &n), None);
    }

    #[test]
    fn send_scope_defaults_merged_to_open() {
        assert_eq!(send_scope(&Tab::Merged), "open");
        assert_eq!(send_scope(&chan("deploy")), "deploy");
    }

    #[test]
    fn command_scope_explicit_wins_over_foreground() {
        assert_eq!(command_scope(Some("infra".into()), &chan("deploy")), "infra");
        assert_eq!(command_scope(None, &chan("deploy")), "deploy");
        assert_eq!(command_scope(None, &Tab::Merged), "open");
    }

    #[test]
    fn dissolve_advances_to_next_channel_tab() {
        // Strip [#open #a #b #c], dissolve #b while it's foreground →
        // the tab that slides into its slot: #c.
        let n = names(&["open", "a", "b", "c"]);
        assert_eq!(advance_after_dissolve("b", &n), chan("c"));
    }

    #[test]
    fn dissolve_of_last_tab_clamps_to_strip_end() {
        let n = names(&["open", "a", "b"]);
        assert_eq!(advance_after_dissolve("b", &n), chan("a"));
    }

    #[test]
    fn dissolve_floor_is_open() {
        // Terminal state: no named channels left → focus lands on the
        // default, immutable channel.
        let n = names(&["open", "solo"]);
        assert_eq!(advance_after_dissolve("solo", &n), chan("open"));
    }

    #[test]
    fn normalize_drops_vanished_channel_to_open() {
        let n = names(&["open", "deploy"]);
        assert_eq!(normalize(chan("ghost"), &n), chan("open"));
        assert_eq!(normalize(chan("deploy"), &n), chan("deploy"));
        assert_eq!(normalize(Tab::Merged, &n), Tab::Merged);
    }

    #[test]
    fn merged_shows_everything() {
        for c in [
            Channel::Open,
            Channel::Group("g".into()),
            Channel::Direct,
            Channel::Info,
        ] {
            assert!(visible_in(&c, &Tab::Merged), "{c:?} must show in merged");
        }
    }

    #[test]
    fn channel_tabs_filter_to_their_own_traffic() {
        let tab = chan("deploy");
        assert!(visible_in(&Channel::Group("deploy".into()), &tab));
        assert!(!visible_in(&Channel::Group("infra".into()), &tab));
        assert!(!visible_in(&Channel::Open, &tab));
        // Directed traffic is merged-only.
        assert!(!visible_in(&Channel::Direct, &tab));
        // Command feedback follows the operator everywhere.
        assert!(visible_in(&Channel::Info, &tab));
    }

    #[test]
    fn open_tab_shows_broadcast_only() {
        let tab = chan("open");
        assert!(visible_in(&Channel::Open, &tab));
        assert!(!visible_in(&Channel::Group("deploy".into()), &tab));
        assert!(!visible_in(&Channel::Direct, &tab));
    }
}
