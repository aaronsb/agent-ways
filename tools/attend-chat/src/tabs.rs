//! Tab model for the channel strip (#393, ADR-173 Decision 6).
//!
//! Channels are tabs: `[≡] [#g] … [+]` on the left and the reserved
//! `[#open] [⋯all]` pinned at the right end ([`reserved`]). Per-channel tabs show a filtered transcript (the
//! strict IRC prior); merged keeps today's full cross-channel stream
//! (ambient awareness). Slash commands with no channel argument scope
//! to the foreground tab — the `/part`-in-the-current-window
//! convention — while explicit arguments work from anywhere.
//!
//! Everything here is a pure function of the tab + the ordered
//! channel-name list, so cycling, dissolve-advance, and scope
//! resolution are testable without a terminal. The strip is drawn in
//! `crate::view`.

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

/// The tabs reserved to the bar's right end, in the order drawn: the base
/// channel `#open`, then the operator view (merged). attend reserves `open`
/// (and `broadcast`, which is `#open`'s own room and has no tab of its own),
/// so no created channel can take either place. This is the one list the
/// tab bar, the focus ring and the bar's width rules read (ADR-405).
pub fn reserved() -> [Tab; 2] {
    [Tab::Channel(BASE_CHANNEL_NAME.to_string()), Tab::Merged]
}

/// Whether `tab` is one of the [`reserved`] tabs.
pub fn is_reserved(tab: &Tab) -> bool {
    reserved().contains(tab)
}

/// One slot of the tab bar: the common menu `≡`, a tab, or `+` for a new
/// channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Common,
    Tab(Tab),
    New,
}

/// The tab bar in the order it is drawn and walked: `≡`, the created
/// channels in strip order, `+`, then the [`reserved`] tabs at the right
/// end. `#open` is there while the strip lists it, as it always does live.
pub fn bar(names: &[String]) -> Vec<Slot> {
    let mut out = vec![Slot::Common];
    out.extend(names.iter().map(|n| Tab::Channel(n.clone())).filter(|t| !is_reserved(t)).map(Slot::Tab));
    out.push(Slot::New);
    out.extend(
        reserved()
            .into_iter()
            .filter(|t| match t {
                Tab::Channel(n) => names.contains(n),
                Tab::Merged => true,
            })
            .map(Slot::Tab),
    );
    out
}

/// Tab on an empty compose line: the next tab in the bar's order, the
/// created channels, then `#open`, then the operator view, wrapping. A tab
/// no longer on the bar starts the walk again at the first.
pub fn cycle_next(current: &Tab, names: &[String]) -> Tab {
    let tabs: Vec<Tab> = bar(names)
        .into_iter()
        .filter_map(|s| match s {
            Slot::Tab(t) => Some(t),
            _ => None,
        })
        .collect();
    match tabs.iter().position(|t| t == current) {
        Some(i) => tabs[(i + 1) % tabs.len()].clone(),
        None => tabs.first().cloned().unwrap_or_default(),
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
    fn the_bar_puts_created_channels_and_plus_left_and_the_reserved_right() {
        let n = names(&["open", "deploy", "infra"]);
        let t = |g: &str| Slot::Tab(chan(g));
        assert_eq!(bar(&n), [Slot::Common, t("deploy"), t("infra"), Slot::New, t("open"), Slot::Tab(Tab::Merged)]);
        assert!(is_reserved(&chan("open")) && is_reserved(&Tab::Merged) && !is_reserved(&chan("deploy")));
    }

    #[test]
    fn cycle_walks_the_bar_order_and_wraps() {
        let n = names(&["open", "deploy", "infra"]);
        assert_eq!(cycle_next(&chan("deploy"), &n), chan("infra"));
        assert_eq!(cycle_next(&chan("infra"), &n), chan("open"));
        assert_eq!(cycle_next(&chan("open"), &n), Tab::Merged);
        assert_eq!(cycle_next(&Tab::Merged, &n), chan("deploy"));
    }

    #[test]
    fn cycle_with_no_channels_stays_on_merged() {
        // Can't happen live (`#open` is always in the strip) but the
        // function must not panic on an empty list.
        assert_eq!(cycle_next(&Tab::Merged, &[]), Tab::Merged);
    }

    #[test]
    fn cycle_from_vanished_channel_starts_again() {
        // Foreground channel dissolved between renders: position()
        // misses, so the cycle starts again rather than panicking.
        let n = names(&["open", "deploy"]);
        assert_eq!(cycle_next(&chan("ghost"), &n), chan("deploy"));
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
