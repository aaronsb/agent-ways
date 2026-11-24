// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The tab the chat last showed, so a restart opens on it (#1349).
//!
//! Each time the shown tab changes to a channel or the operator view, the
//! chat writes it to attend-chat's state file
//! `$XDG_STATE_HOME/attend-chat/last-tab`
//! (`~/.local/state/attend-chat/last-tab` when the variable is unset). The
//! write replaces the file whole, through a temporary file and a rename, so
//! a chat that is killed leaves the last tab it showed. A DM view or an
//! agent's own DMs is not recorded: the file keeps the channel shown before
//! it.
//!
//! The file is one line: the tab's key, `all` for the operator view or
//! `#name` for a channel, then for a channel the operator was a member of
//! when it was shown, a tab and `member`.
//!
//! At start the chat reopens the recorded tab ([`reopen`]). A channel that
//! no longer exists, or that the operator was a member of and has left
//! since, opens `#open` instead. A missing or unreadable file, or one that
//! does not parse, leaves the chat on the tab it opens on without one.

use std::path::{Path, PathBuf};

use crate::groups::{KnownGroup, BASE_CHANNEL_NAME};
use crate::tabs::Tab;

/// The tab recorded, and whether the operator was a member of its channel
/// when it was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Last {
    pub tab: Tab,
    pub member: bool,
}

impl Last {
    /// The record for `tab`, or `None` for a tab that is not recorded.
    pub fn of(tab: &Tab, member: bool) -> Option<Last> {
        match tab {
            Tab::Merged => Some(Last { tab: Tab::Merged, member: false }),
            Tab::Channel(_) => Some(Last { tab: tab.clone(), member }),
            Tab::Dm(_) | Tab::Review(_) => None,
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
            _ => {
                let name = key.strip_prefix('#')?;
                if name.is_empty() || name.chars().any(char::is_whitespace) {
                    return None;
                }
                Tab::Channel(name.to_string())
            }
        };
        Some(Last { member: member && matches!(tab, Tab::Channel(_)), tab })
    }

    /// Write the record to `path`, whole.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let key = match &self.tab {
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

/// attend-chat's state file for the last tab shown.
pub fn default_path() -> PathBuf {
    crate::hidden::state_dir().join("last-tab")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_theme::ColorDepth;
    use agent_identity::Group;
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

    #[test]
    fn the_record_survives_a_restart() {
        let dir = crate::test_dir::unique("last-tab");
        let path = dir.join("attend-chat").join("last-tab");
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

    #[test]
    fn a_malformed_file_is_no_record() {
        let dir = crate::test_dir::unique("last-tab");
        let path = dir.join("last-tab");
        for bad in ["", "\n", "deploy\n", "#\n", "#two words\n", "#deploy\tguest\n", "#deploy\tmember\textra\n", "all\n#deploy\n", "\u{0}\u{1}garbage"] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(Last::load(&path), None, "{bad:?}");
            assert_eq!(reopen(Last::load(&path).as_ref(), &known(&[("deploy", &[])]), "operator"), None, "{bad:?} opens where a first start does");
        }
        std::fs::write(&path, [0xff, 0xfe, b'\n']).unwrap();
        assert_eq!(Last::load(&path), None, "not UTF-8");
    }

    #[test]
    fn views_are_not_recorded() {
        assert_eq!(Last::of(&Tab::Dm("a1".into()), true), None);
        assert_eq!(Last::of(&Tab::Review("a1".into()), false), None);
        assert_eq!(Last::of(&Tab::Merged, true), Some(Last { tab: Tab::Merged, member: false }));
        assert_eq!(Last::of(&chan("deploy"), true), Some(Last { tab: chan("deploy"), member: true }));
    }

    #[test]
    fn the_state_file_is_under_xdg_state_home() {
        let _env = crate::test_dir::env_lock();
        let prev = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", "/tmp/st");
        assert_eq!(default_path(), PathBuf::from("/tmp/st/attend-chat/last-tab"));
        match prev {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }
}
