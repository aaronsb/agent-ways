// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The tab the chat last showed, so a restart opens on it (#1349).
//!
//! The chat keeps two files in attend-chat's state directory
//! (`$XDG_STATE_HOME/attend-chat`, or `~/.local/state/attend-chat` when the
//! variable is unset):
//!
//! - `last-tab`, the tab shown. The chat writes it when the record changes:
//!   when a channel or the operator view is shown, and when the operator
//!   becomes a member of the channel shown. A DM view or an agent's own DMs
//!   is not recorded, and the file keeps the channel shown before it. The
//!   write replaces the file whole, through a temporary file and a rename,
//!   so a chat that is killed leaves the last record it made.
//! - `shown`, the operator view's DM watermark: the newest DM to the
//!   operator that the operator view held when it was last left, in the
//!   format of Hide messages here's file ([`crate::hidden`]).
//!
//! `last-tab` is one line: `all` for the operator view, or `#name` for a
//! channel, followed by a tab and `member` when the operator was a member
//! of the channel at any time while it was shown ([`Last::showing`]).
//!
//! At start the chat reopens the recorded tab ([`reopen`]). A channel that
//! no longer exists, or that the operator was a member of and has left
//! since, opens `#open` instead. A missing or unreadable file, or one that
//! does not parse, leaves the chat on the tab it opens on without one.

use std::path::{Path, PathBuf};

use crate::groups::{KnownGroup, BASE_CHANNEL_NAME};
use crate::tabs::Tab;

/// The tab recorded, and whether the operator was a member of its channel
/// while it was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Last {
    pub tab: Tab,
    pub member: bool,
}

/// Whether the file can hold the channel `name`: one with no tab, newline
/// or carriage return, the characters its line is split on.
fn keepable(name: &str) -> bool {
    !name.is_empty() && !name.contains(['\t', '\n', '\r'])
}

impl Last {
    /// The record for `tab` shown now, given the record `prev` made while it
    /// was shown and the channels `known` lists with their members. The
    /// operator `person` counts as a member when they are one now, or when
    /// `prev` already recorded them as one for this tab: leaving the channel
    /// shown does not clear it. `None` for a tab that is not recorded, a DM
    /// view or a channel whose name the file cannot hold.
    pub fn showing(tab: &Tab, prev: Option<&Last>, known: &[KnownGroup], person: &str) -> Option<Last> {
        match tab {
            Tab::Merged => Some(Last { tab: Tab::Merged, member: false }),
            Tab::Channel(name) if keepable(name) => {
                let now = known.iter().any(|k| k.group.name == *name && k.membership.members.iter().any(|m| m == person));
                let before = prev.is_some_and(|p| p.tab == *tab && p.member);
                Some(Last { tab: tab.clone(), member: now || before })
            }
            Tab::Channel(_) | Tab::Dm(_) | Tab::Review(_) => None,
        }
    }

    /// The record kept at `path`; `None` when the file is missing,
    /// unreadable, or does not parse.
    pub fn load(path: &Path) -> Option<Last> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let line = lines.next()?;
        if lines.next().is_some() {
            return None;
        }
        let mut fields = line.split('\t');
        let key = fields.next()?;
        let member = match fields.next() {
            None => false,
            Some("member") => true,
            Some(_) => return None,
        };
        if fields.next().is_some() {
            return None;
        }
        let tab = match key {
            "all" => Tab::Merged,
            _ => Tab::Channel(key.strip_prefix('#').filter(|n| keepable(n))?.to_string()),
        };
        Some(Last { member: member && matches!(tab, Tab::Channel(_)), tab })
    }

    /// Write the record to `path`, whole. A channel whose name the file
    /// cannot hold is not written, and the file keeps the record before it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let key = match &self.tab {
            Tab::Channel(c) if !keepable(c) => return Ok(()),
            Tab::Channel(c) => format!("#{c}"),
            _ => "all".to_string(),
        };
        let body = if self.member { format!("{key}\tmember\n") } else { format!("{key}\n") };
        agent_settings::writer::write_atomic(path, body)
    }
}

/// The tab to open on for the record `last`, against the channels `known`
/// lists now and the operator `person`: the recorded tab, or `#open` for a
/// channel that is gone or that the operator has left. `None` with no
/// record: the chat opens where it opens without one.
pub fn reopen(last: Option<&Last>, known: &[KnownGroup], person: &str) -> Option<Tab> {
    let last = last?;
    let Tab::Channel(name) = &last.tab else {
        return Some(last.tab.clone());
    };
    let open = Tab::Channel(BASE_CHANNEL_NAME.to_string());
    let Some(group) = known.iter().find(|k| k.group.name == *name) else {
        return Some(open);
    };
    let left = last.member && !group.membership.members.iter().any(|m| m == person);
    Some(if left { open } else { last.tab.clone() })
}

/// The file in the state directory `dir` that holds the last tab shown.
pub fn path(dir: &Path) -> PathBuf {
    dir.join("last-tab")
}

/// The file in the state directory `dir` that holds the operator view's DM
/// watermark.
pub fn shown_path(dir: &Path) -> PathBuf {
    dir.join("shown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::Group;
    use agent_theme::ColorDepth;
    use attend_groups::GroupEntry;

    fn known(channels: &[(&str, &[&str])]) -> Vec<KnownGroup> {
        let mut out = vec![KnownGroup { group: Group::for_name(BASE_CHANNEL_NAME, ColorDepth::NoColor), membership: GroupEntry::default(), is_base: true }];
        out.extend(channels.iter().map(|(name, members)| KnownGroup {
            group: Group::for_name(name, ColorDepth::NoColor),
            membership: GroupEntry { members: members.iter().map(|m| m.to_string()).collect(), ..GroupEntry::default() },
            is_base: false,
        }));
        out
    }

    fn chan(name: &str) -> Tab {
        Tab::Channel(name.into())
    }

    #[test]
    fn a_recorded_channel_reopens() {
        let now = known(&[("deploy", &["operator"]), ("infra", &[])]);
        let member = Last { tab: chan("deploy"), member: true };
        assert_eq!(reopen(Some(&member), &now, "operator"), Some(chan("deploy")), "a channel the operator is still in");
        let viewed = Last { tab: chan("infra"), member: false };
        assert_eq!(reopen(Some(&viewed), &now, "operator"), Some(chan("infra")), "a channel only viewed, never joined");
        assert_eq!(reopen(Some(&Last { tab: Tab::Merged, member: false }), &now, "operator"), Some(Tab::Merged), "the operator view");
        assert_eq!(reopen(Some(&Last { tab: chan("open"), member: false }), &now, "operator"), Some(chan("open")));
    }

    #[test]
    fn a_channel_gone_or_left_opens_open() {
        let now = known(&[("deploy", &["someone-else"])]);
        assert_eq!(reopen(Some(&Last { tab: chan("infra"), member: false }), &now, "operator"), Some(chan("open")), "gone");
        assert_eq!(reopen(Some(&Last { tab: chan("infra"), member: true }), &now, "operator"), Some(chan("open")), "gone, once a member");
        assert_eq!(reopen(Some(&Last { tab: chan("deploy"), member: true }), &now, "operator"), Some(chan("open")), "left");
    }

    #[test]
    fn no_record_keeps_the_opening_tab() {
        assert_eq!(reopen(None, &known(&[("deploy", &[])]), "operator"), None);
    }

    /// The member bit means "a member at some time while it was shown": it
    /// rises when the operator joins the channel shown and stays set when
    /// they leave it, and a channel shown afresh starts from what is true
    /// now.
    #[test]
    fn the_member_bit_only_rises_while_the_tab_is_shown() {
        let out = known(&[("deploy", &[])]);
        let inside = known(&[("deploy", &["operator"])]);
        let shown = Last::showing(&chan("deploy"), None, &out, "operator").unwrap();
        assert!(!shown.member, "shown before joining");
        let joined = Last::showing(&chan("deploy"), Some(&shown), &inside, "operator").unwrap();
        assert!(joined.member, "joined while shown");
        let left = Last::showing(&chan("deploy"), Some(&joined), &out, "operator").unwrap();
        assert!(left.member, "leaving does not clear it");
        let elsewhere = Last::showing(&chan("infra"), Some(&joined), &known(&[("infra", &[])]), "operator").unwrap();
        assert!(!elsewhere.member, "another channel starts afresh");
        let again = Last::showing(&chan("deploy"), Some(&elsewhere), &out, "operator").unwrap();
        assert!(!again.member, "shown again after another tab, as a non-member");
    }

    #[test]
    fn views_and_names_the_file_cannot_hold_are_not_recorded() {
        let none = known(&[]);
        assert_eq!(Last::showing(&Tab::Dm("a1".into()), None, &none, "operator"), None);
        assert_eq!(Last::showing(&Tab::Review("a1".into()), None, &none, "operator"), None);
        assert_eq!(Last::showing(&Tab::Merged, None, &none, "operator"), Some(Last { tab: Tab::Merged, member: false }));
        for bad in ["x\tmember", "a\nb", "a\rb"] {
            assert_eq!(Last::showing(&chan(bad), None, &none, "operator"), None, "{bad:?}");
        }
    }

    #[test]
    fn the_record_survives_a_restart() {
        let dir = crate::test_dir::unique("last-tab");
        let path = path(&dir.join("attend-chat"));
        assert_eq!(Last::load(&path), None, "a first start: no file");
        for last in [Last { tab: chan("deploy"), member: true }, Last { tab: chan("infra"), member: false }, Last { tab: Tab::Merged, member: false }] {
            last.save(&path).unwrap();
            assert_eq!(Last::load(&path), Some(last));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "all\n");
        Last { tab: chan("deploy"), member: true }.save(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "#deploy\tmember\n");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers, ["last-tab"], "the temporary file is renamed into place");
    }

    /// Every name without a tab, newline or carriage return comes back as
    /// written, spaces and Unicode whitespace included. A name with one of
    /// those is not written, and the file keeps the record before it.
    #[test]
    fn names_round_trip_or_are_not_written() {
        let dir = crate::test_dir::unique("last-tab");
        let path = path(&dir);
        for name in ["deploy", "two words", "café\u{a0}ops", "tab\u{2003}em", "trailing "] {
            for member in [false, true] {
                let last = Last { tab: chan(name), member };
                last.save(&path).unwrap();
                assert_eq!(Last::load(&path), Some(last), "{name:?}");
            }
        }
        Last { tab: chan("kept"), member: false }.save(&path).unwrap();
        for bad in ["x\tmember", "a\nb", "a\rb"] {
            Last { tab: chan(bad), member: false }.save(&path).unwrap();
            assert_eq!(Last::load(&path), Some(Last { tab: chan("kept"), member: false }), "{bad:?} leaves the record before it");
        }
    }

    #[test]
    fn a_malformed_file_is_no_record() {
        let dir = crate::test_dir::unique("last-tab");
        let path = dir.join("last-tab");
        for bad in ["", "\n", "deploy\n", "#\n", "#a\rb\n", "#deploy\tguest\n", "#deploy\tmember\textra\n", "all\n#deploy\n", "\u{0}\u{1}garbage"] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(Last::load(&path), None, "{bad:?}");
            assert_eq!(reopen(Last::load(&path).as_ref(), &known(&[("deploy", &[])]), "operator"), None, "{bad:?} opens where a first start does");
        }
        std::fs::write(&path, [0xff, 0xfe, b'\n']).unwrap();
        assert_eq!(Last::load(&path), None, "not UTF-8");
    }

    #[test]
    fn the_files_sit_in_the_state_directory() {
        let dir = Path::new("/tmp/st/attend-chat");
        assert_eq!(path(dir), PathBuf::from("/tmp/st/attend-chat/last-tab"));
        assert_eq!(shown_path(dir), PathBuf::from("/tmp/st/attend-chat/shown"));
    }
}
