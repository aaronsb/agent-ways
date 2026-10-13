//! Tab model for the channel strip (#393, ADR-173 Decision 6).
//!
//! The tab bar holds `[≡]`, the one created channel shown, `[+]` on the
//! left, and the reserved `[#open] [⋯all]` pinned at the right end
//! ([`reserved`]). The left column lists every channel, numbered for
//! Ctrl+digit; the bar has no tab per channel. A channel tab shows a
//! filtered transcript (the strict IRC prior); merged keeps the full
//! cross-channel stream (ambient awareness). Slash commands with no channel
//! argument scope to the foreground tab — the `/part`-in-the-current-window
//! convention — while explicit arguments work from anywhere.
//!
//! Everything here is a pure function of the tab + the ordered
//! channel-name list, so the bar, dissolve-advance, and scope
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
/// channel `shown`, if it is one the strip lists, `+`, then the
/// [`reserved`] tabs at the right end. While `#open` or the operator view
/// is shown the left holds no channel tab. `#open` is there while the strip
/// lists it, as it always does live.
pub fn bar(names: &[String], shown: &Tab) -> Vec<Slot> {
    let mut out = vec![Slot::Common];
    if let Tab::Channel(c) = shown {
        if !is_reserved(shown) && names.contains(c) {
            out.push(Slot::Tab(shown.clone()));
        }
    }
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
    fn the_bar_holds_the_channel_shown_and_plus_left_and_the_reserved_right() {
        let n = names(&["open", "deploy", "infra"]);
        let t = |g: &str| Slot::Tab(chan(g));
        let right = [t("open"), Slot::Tab(Tab::Merged)];
        assert_eq!(bar(&n, &chan("infra")), [Slot::Common, t("infra"), Slot::New, right[0].clone(), right[1].clone()], "one tab: the created channel shown");
        assert_eq!(bar(&n, &Tab::Merged), [Slot::Common, Slot::New, right[0].clone(), right[1].clone()], "none while the operator view is shown");
        assert_eq!(bar(&n, &chan("open")), [Slot::Common, Slot::New, right[0].clone(), right[1].clone()], "none while #open is shown");
        assert_eq!(bar(&n, &chan("ghost")), [Slot::Common, Slot::New, right[0].clone(), right[1].clone()], "none for a channel the strip lacks");
        assert!(is_reserved(&chan("open")) && is_reserved(&Tab::Merged) && !is_reserved(&chan("deploy")));
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
