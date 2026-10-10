//! The tab menu: what a tab's menu offers and what each item does, the
//! inline confirm a destructive item waits on, and the compose box lent
//! to a name or a description. Every item runs the slash command that
//! does the same thing ([`keys::run_slash`]); the commands stay.
//!
//! - merged: Clear view.
//! - `#open`: Clear history, Clear view.
//! - a named channel: Add agent ▸, Remove agent ▸, Describe…, Clear
//!   history, Leave, Delete channel.
//! - the `+` slot: New channel…, a name typed in the compose box.
//!
//! Clear history and Delete channel ask first: `y` goes ahead, any other
//! key keeps the history. Add agent invites the peer (it joins itself);
//! enrolling it without its own join is the operator enrollment the Draft
//! ADR-404 proposes, not done here.

use agent_theme::ColorDepth;
use agent_tui::Open;

use super::keys::{self, EnterAction};
use super::ChatPane;
use crate::groups::BASE_CHANNEL_NAME;
use crate::slash::SlashOutcome;
use crate::tabs::Tab;

pub(super) const CLEAR_VIEW: &str = "Clear view";
pub(super) const CLEAR_HISTORY: &str = "Clear history";
pub(super) const ADD_AGENT: &str = "Add agent ▸";
pub(super) const REMOVE_AGENT: &str = "Remove agent ▸";
pub(super) const DESCRIBE: &str = "Describe…";
pub(super) const LEAVE: &str = "Leave";
pub(super) const DELETE: &str = "Delete channel";

/// A step waiting for `y`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pending {
    /// Delete the channel's history (`/purge`).
    ClearHistory(String),
    /// Delete the channel (`/dissolve`).
    Delete(String),
    /// Quit: Esc where the terminal sends Ctrl+3 as Esc.
    Quit,
}

/// What the compose box is lent to, and the draft it held before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Prompt {
    NewChannel,
    Describe(String),
}

/// The items of a tab's menu, by the channel it shows (`None`: merged).
pub(super) fn items(channel: Option<&str>) -> Vec<String> {
    let items: &[&str] = match channel {
        None => &[CLEAR_VIEW],
        Some(BASE_CHANNEL_NAME) => &[CLEAR_HISTORY, CLEAR_VIEW],
        Some(_) => &[ADD_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, LEAVE, DELETE],
    };
    items.iter().map(|s| s.to_string()).collect()
}

impl ChatPane {
    /// The bottom bar's line while a step waits or the compose box is lent.
    pub(super) fn menu_status(&self) -> Option<String> {
        if let Some(p) = &self.pending {
            return Some(match p {
                Pending::ClearHistory(c) => {
                    format!("clear #{c} history? new agents are no longer caught up on it · y clears · any other key keeps it")
                }
                Pending::Delete(c) => format!("delete #{c} and its history? y deletes · any other key keeps it"),
                Pending::Quit => "quit? y quits · any other key stays · Ctrl-C quits at once".into(),
            });
        }
        self.prompt.as_ref().map(|(p, _)| match p {
            Prompt::NewChannel => "new channel: type its name · Enter creates it · Esc cancels".into(),
            Prompt::Describe(c) => format!("describe #{c}: type a line · Enter sets it (empty clears) · Esc cancels"),
        })
    }

    /// Open the menu of tab `i`: 0 is merged, then the channels in strip
    /// order, then the `+` slot.
    pub(super) fn open_tab_menu(&mut self, i: usize) {
        self.dirty = true;
        let names = self.strip_names();
        if i == names.len() + 1 {
            return self.start_prompt(Prompt::NewChannel);
        }
        let channel = match i {
            0 => None,
            i => match names.get(i - 1) {
                Some(n) => Some(n.clone()),
                None => return,
            },
        };
        let title = channel.as_ref().map_or("merged".to_string(), |c| format!("#{c}"));
        let id = format!("menu:{}", channel.as_deref().unwrap_or(""));
        self.open = Some(Open::Pick { id, title, options: items(channel.as_deref()), multi: false, chosen: Vec::new() });
    }

    /// A menu item, or a submenu's peer, was chosen.
    pub(super) fn menu_picked(&mut self, id: &str, value: &str) {
        self.dirty = true;
        if let Some(c) = id.strip_prefix("menu:") {
            return self.item(c, value);
        }
        // A peer from Add agent or Remove agent: the label's first word is
        // the display name the commands resolve.
        let peer = value.split_whitespace().next().unwrap_or(value).to_string();
        if let Some(c) = id.strip_prefix("add:") {
            let invited = self.run("invite", SlashOutcome::Invite { member: peer, channel: Some(c.into()) });
            if invited {
                let said = format!("{} — they join themselves; enrolling them is Draft ADR-404", self.status);
                self.say(said, false);
            }
        } else if let Some(c) = id.strip_prefix("remove:") {
            self.run("kick", SlashOutcome::Kick { member: peer, channel: Some(c.into()) });
        }
    }

    fn item(&mut self, channel: &str, item: &str) {
        let chan = || (!channel.is_empty()).then(|| channel.to_string());
        match item {
            CLEAR_VIEW => {
                self.run("clear", SlashOutcome::ClearTranscript);
            }
            CLEAR_HISTORY => self.pending = Some(Pending::ClearHistory(chan().unwrap_or(BASE_CHANNEL_NAME.into()))),
            DELETE => self.pending = chan().map(Pending::Delete),
            LEAVE => {
                self.run("leave", SlashOutcome::Leave(chan()));
            }
            DESCRIBE => self.start_prompt(Prompt::Describe(channel.into())),
            ADD_AGENT => self.peer_menu(channel, true),
            REMOVE_AGENT => self.peer_menu(channel, false),
            _ => {}
        }
    }

    /// The live peers not in `channel` (add), or the agents in it (remove).
    fn peer_menu(&mut self, channel: &str, add: bool) {
        let members = attend_groups::load_groups(&crate::signal::signals_base()).get(channel).map(|e| e.members.clone()).unwrap_or_default();
        let roster = crate::peers::roster(ColorDepth::detect(), &attend_instances::SnapshotCache::new());
        let options: Vec<String> = roster
            .iter()
            .filter(|p| members.contains(&p.session_id) != add)
            .map(|p| p.display.clone())
            .collect();
        if options.is_empty() {
            let what = if add { "no live agent outside" } else { "no live agent in" };
            return self.say(format!("{what} #{channel}"), true);
        }
        let (id, title) = if add { (format!("add:{channel}"), format!("add to #{channel}")) } else { (format!("remove:{channel}"), format!("remove from #{channel}")) };
        self.open = Some(Open::Pick { id, title, options, multi: false, chosen: Vec::new() });
    }

    /// Lend the compose box to `p`, keeping the draft for after.
    fn start_prompt(&mut self, p: Prompt) {
        let draft = self.input.text().to_string();
        self.input.clear();
        self.prompt = Some((p, draft));
    }

    /// Give the compose box back, with the draft it held.
    fn end_prompt(&mut self) {
        if let Some((_, draft)) = self.prompt.take() {
            let n = draft.chars().count();
            self.input.set(draft, n);
        }
    }

    /// Enter while the compose box is lent: run what it was lent for.
    pub(super) fn prompt_enter(&mut self) {
        let Some((p, _)) = self.prompt.clone() else { return };
        let text = self.input.text().trim().to_string();
        match p {
            Prompt::NewChannel => {
                let name = text.trim_start_matches('#').to_string();
                if let Err(e) = attend_groups::validate_group_name(&name) {
                    return self.say(format!("new channel: {e}"), true);
                }
                self.end_prompt();
                if self.run("channels create", SlashOutcome::CreateChannel { name: name.clone(), description: None }) && !self.dry_run {
                    self.stale();
                    self.set_tab(Tab::Channel(name));
                }
            }
            Prompt::Describe(c) => {
                self.end_prompt();
                self.run("channels describe", SlashOutcome::DescribeChannel { name: c, description: text });
            }
        }
    }

    /// Esc while the compose box is lent: give it back unchanged.
    pub(super) fn prompt_cancel(&mut self) {
        self.end_prompt();
        self.say("cancelled", false);
    }

    /// A key while a step waits: `y` goes ahead, any other key keeps
    /// things as they are. True when the screen should quit.
    pub(super) fn answer(&mut self, yes: bool) -> bool {
        let Some(p) = self.pending.take() else { return false };
        self.dirty = true;
        if !yes {
            self.say(if p == Pending::Quit { "stayed" } else { "kept" }, false);
            return false;
        }
        match p {
            Pending::Quit => return true,
            Pending::ClearHistory(c) => {
                self.run("purge", SlashOutcome::Purge(Some(c)));
            }
            Pending::Delete(c) => {
                self.run("dissolve", SlashOutcome::Dissolve(Some(c)));
            }
        }
        false
    }

    /// Run a slash command for a menu item and show what it did, the
    /// compose box left as it was. A dry run (`--snap`) runs nothing.
    /// True when it succeeded.
    fn run(&mut self, name: &str, outcome: SlashOutcome) -> bool {
        if self.dry_run {
            self.say(format!("dry run: /{name} not run"), false);
            return true;
        }
        let fg = self.normal_tab();
        let ok = match keys::run_slash(outcome, &fg) {
            EnterAction::None => true,
            EnterAction::ClearWithStatus(s) => {
                self.say(s, false);
                true
            }
            EnterAction::ClearWithStatusAndEcho { status, echo } => {
                self.say(status, false);
                self.push(echo);
                true
            }
            EnterAction::StatusOnly(s) => {
                self.say(s, true);
                false
            }
            EnterAction::ClearWithStatusAndFocus { status, focus } => {
                self.say(status, false);
                self.set_tab(focus);
                true
            }
            EnterAction::ClearTranscript => {
                self.signals.clear();
                self.say("transcript cleared", false);
                true
            }
        };
        self.stale();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tab_offers_what_its_channel_allows() {
        assert_eq!(items(None), [CLEAR_VIEW]);
        assert_eq!(items(Some("open")), [CLEAR_HISTORY, CLEAR_VIEW]);
        assert_eq!(items(Some("deploy")), [ADD_AGENT, REMOVE_AGENT, DESCRIBE, CLEAR_HISTORY, LEAVE, DELETE]);
    }
}
