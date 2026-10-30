//! A mod as an item: shipped with agent-ways in its marketplace folder, its
//! state in ADR-512's words, and a critical mod that does not run saying so
//! as the bottom bar's notice does.

use agent_settings::item::{Entry, Family, Item, ItemKind, ModState, Origin};
use serde::Serialize;

use crate::cmd::attend_mod::{self, Mod, JUDGE};
use crate::cmd::mods::{InTarget, Overview};

/// Why a mod's state is not to hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The first read has not landed.
    Reading,
    /// The read failed with nothing read before it.
    Failed,
    /// Read, and the mod was not in it.
    Missing,
}

fn state(s: attend_mod::State) -> ModState {
    use attend_mod::State as S;
    match s {
        S::Converged => ModState::Converged,
        S::Drift => ModState::Drift,
        S::Pending => ModState::Pending,
        S::Disabled => ModState::Disabled,
        S::Removed => ModState::Removed,
        S::Held => ModState::Held,
        S::Off => ModState::Off,
    }
}

/// A mod's own fields, after its descriptor (ADR-516 §2). The screens also
/// draw whether it is critical, the Claude Code it needs and the versions
/// installed, which the CLI leaves to `targets`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModFields {
    /// Whether the user config wants it; null until its state is read.
    pub wanted: Option<bool>,
    /// Each enabled target's state of it.
    pub targets: Vec<InTarget>,
    #[serde(skip)]
    pub critical: &'static str,
    #[serde(skip)]
    pub needs: String,
    #[serde(skip)]
    pub installed: Vec<String>,
}

/// `m` as its overview `o` reads it, or as far as an unread state tells:
/// `unknown`, with where the read stands.
pub fn item(m: &Mod, o: Result<&Overview, Unread>) -> Entry<ModFields> {
    let critical = match (m.critical, *m == JUDGE, o.is_ok_and(|o| o.claude_code)) {
        (false, _, _) => "no",
        (true, false, _) => "yes",
        (true, true, true) => "yes: gate.engine is claude-code",
        (true, true, false) => "no: only while gate.engine is claude-code",
    };
    let mut fields = ModFields { wanted: None, targets: Vec::new(), critical, needs: format!("Claude Code {} or later", m.min_claude_code), installed: Vec::new() };
    let mut item = Item { kind: Some(ItemKind::Plugin), source: Some(m.id.into()), ..Item::new(Family::Mod, m.name, Origin::Builtin) };
    match o {
        Err(why) => {
            let said = match why {
                Unread::Reading => "reading…",
                Unread::Failed => "state read failed",
                Unread::Missing => "state not read",
            };
            item.detail = Some(said.into());
        }
        Ok(o) => {
            use attend_mod::State as S;
            item.path = o.shipped.is_some().then(|| o.folder.join(m.name));
            item.version = o.shipped.clone();
            item = item.with_state(state(o.word));
            item.detail = match (o.attention.first(), o.word) {
                (Some(a), _) => Some(format!("critical: {}", a.why.word())),
                (None, S::Drift) if o.older => Some("an installed copy is older".into()),
                (None, S::Drift) => Some("repair converges it".into()),
                (None, S::Held) => Some("a finding has its settings entry ignored".into()),
                (None, S::Off) if o.shipped.is_none() => Some("not shipped in this app".into()),
                (None, S::Off) if !o.wanted => Some("not wanted".into()),
                (None, S::Off) if o.targets.is_empty() => Some("no enabled target holds it".into()),
                _ => None,
            };
            fields.wanted = Some(o.wanted);
            fields.targets = o.targets.clone();
            fields.installed = o.installed.clone();
        }
    }
    Entry { item, fields }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::attend_mod::ATTEND;

    #[test]
    fn a_mod_that_is_not_shipped_has_no_path_or_version() {
        let o = Overview {
            m: ATTEND,
            shipped: None,
            word: attend_mod::State::Off,
            rows: vec![],
            older: false,
            claude_code: false,
            attention: vec![],
            installed: vec![],
            folder: "/home/u/plugins".into(),
            wanted: true,
            targets: vec![],
        };
        let e = item(&ATTEND, Ok(&o));
        assert_eq!((e.item.path, e.item.version, e.item.detail.as_deref()), (None, None, Some("not shipped in this app")));
        let shipped = Overview { shipped: Some("0.1.0".into()), ..o };
        assert_eq!(item(&ATTEND, Ok(&shipped)).item.path.as_deref(), Some(std::path::Path::new("/home/u/plugins/attend")));
    }
}
