//! The `≡` menu's Settings ▸: the settings the chat and the attend mod
//! read ([`crate::settings_edit`]), each as `name = value (layer)`. Picking
//! one walks to a value through the shell's pickers:
//!
//! - an attend mod key (`attend_mod.listening`, `attend_mod.keepwarm`)
//!   first asks where to write it: the user file, the project the chat runs
//!   in, a live agent's project, or another project typed in an entry
//!   dialog. Each place says what its file holds.
//! - a chat key (`attend.chat.*`) is the user file's alone, so it goes
//!   straight to its values.
//!
//! The values mark the one in effect at that place; Unset here, offered
//! where the place's file holds the key, removes it. The pick is written
//! as `ways settings set` writes it, the bottom bar says where and names a
//! layer that still overrides it, and the chat's own keys apply at once.

use std::path::PathBuf;

use agent_tui::{Anchor, Item, Open};

use super::ChatPane;
use crate::settings_edit::{self as edit, Place};

/// The list's id, and the ids of the steps after it: a key's layer
/// picker, its value picker (with the place, after a separator), and the
/// entry dialog for another project.
const LIST: &str = "settings";
const LAYER: &str = "setting-layer:";
const VALUE: &str = "setting-value:";
const DIR: &str = "setting-dir:";
const SEP: char = '\u{1f}';

/// The layer picker's last item: a project typed in a dialog.
pub(super) const ANOTHER: &str = "Another project…";
/// The value picker's item that removes the key from the place's file.
pub(super) const UNSET: &str = "Unset here";

impl ChatPane {
    /// The list of settings, docked at `anchor`.
    pub(super) fn settings_list(&mut self, anchor: Anchor) {
        let dir = std::env::current_dir().unwrap_or_default();
        let items = edit::settings(&dir).iter().map(|s| Item::new(s.line())).collect();
        self.to_open = Some(Open::Pick { id: LIST.into(), title: "settings".into(), items, multi: false, anchor });
    }

    /// The live agents' project roots, for the layer picker.
    fn agent_roots(&mut self) -> Vec<String> {
        self.world()
            .seen
            .iter()
            .filter(|s| s.live && s.identity.is_claude)
            .map(|s| attend_presence::session::normalize_origin(&s.identity.cwd))
            .collect()
    }

    /// Where `name` can be written, each place with what its file holds.
    fn layer_menu(&mut self, name: &str) {
        let dir = std::env::current_dir().unwrap_or_default();
        let roots = self.agent_roots();
        let places = std::iter::once(Place::User).chain(edit::projects(&dir, &roots).into_iter().map(Place::Project));
        let mut items: Vec<Item> = places.map(|p| Item::new(p.label(edit::held(name, &p).as_deref()))).collect();
        items.push(Item::new(ANOTHER));
        self.to_open = Some(Open::Pick { id: format!("{LAYER}{name}"), title: format!("{name}: write to"), items, multi: false, anchor: Anchor::Parent });
    }

    /// The values of `name` at `place`, the one in effect there marked.
    fn value_menu(&mut self, name: &str, place: &Place, anchor: Anchor) {
        let now = edit::in_effect(name, place);
        let mut items: Vec<Item> = edit::choices(name).into_iter().map(|c| Item::new(c.clone()).current(c == now)).collect();
        if edit::held(name, place).is_some() {
            items.push(Item::new(UNSET));
        }
        let id = format!("{VALUE}{name}{SEP}{}", place.id());
        self.to_open = Some(Open::Pick { id, title: format!("{name} · {}", place.name()), items, multi: false, anchor });
    }

    /// The entry dialog for a project the layer picker does not list.
    fn project_entry(&mut self, name: &str, value: String, error: Option<String>) {
        let title = format!("{name}: a project's directory");
        self.to_open = Some(Open::Entry { id: format!("{DIR}{name}"), title, value, error, anchor: Anchor::Center });
    }

    /// A pick in the list or one of its steps; false when `id` is none of
    /// theirs.
    pub(super) fn setting_picked(&mut self, id: &str, value: &str) -> bool {
        if id == LIST {
            let name = edit::name_of(value).to_string();
            if edit::per_project(&name) {
                self.layer_menu(&name);
            } else {
                self.value_menu(&name, &Place::User, Anchor::Parent);
            }
        } else if let Some(name) = id.strip_prefix(LAYER) {
            match Place::from_label(value) {
                Some(place) => self.value_menu(name, &place, Anchor::Parent),
                None if value == ANOTHER => self.project_entry(name, String::new(), None),
                None => {}
            }
        } else if let Some(name) = id.strip_prefix(DIR) {
            let given = PathBuf::from(value.trim());
            let root = given.canonicalize().ok().filter(|d| d.is_dir()).and_then(|d| ways_core::settings::project_root(&d));
            match root {
                Some(root) => self.value_menu(name, &Place::Project(root), Anchor::Tab(0)),
                None => self.project_entry(name, value.to_string(), Some("no project encloses that directory".into())),
            }
        } else if let Some(rest) = id.strip_prefix(VALUE) {
            let Some((name, place)) = rest.split_once(SEP) else { return true };
            self.setting_chosen(name, &Place::from_id(place), value);
        } else {
            return false;
        }
        true
    }

    /// Write `value` (or remove the key, for [`UNSET`]) at `place`, say on
    /// the bottom bar what changed and what still overrides it, and apply
    /// the chat's own keys. The picker closes, as Theme ▸ and Keybinding
    /// set ▸ do, so the bar is in sight.
    fn setting_chosen(&mut self, name: &str, place: &Place, value: &str) {
        if self.dry_run {
            return self.say("dry run: nothing written", false);
        }
        let dir = std::env::current_dir().unwrap_or_default();
        let done = if value == UNSET { edit::unset_at(name, place) } else { edit::set_at(name, value, place, &dir) };
        match done {
            Ok(said) => {
                self.reload_settings();
                let warn = if name == "attend.chat.tabs.jump" { self.jump_warning() } else { "" };
                self.say(format!("{said}{warn}"), false);
            }
            Err(e) => self.say(e, true),
        }
    }
}
