//! A mod as an item: shipped with agent-ways in its marketplace folder, its
//! state in ADR-512's words, and a critical mod that does not run saying so
//! as the bottom bar's notice does.

use agent_settings::item::{Family, Item, ItemKind, ModState, Origin};
use agent_tui::tree::Card;

use super::row;
use crate::cmd::attend_mod::{self, Mod, JUDGE};
use crate::cmd::mods::Overview;

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
        S::Off => ModState::Off,
    }
}

/// `m` as its overview `o` reads it, or as far as an unread state tells.
pub fn of_mod(m: &Mod, o: Result<&Overview, Unread>) -> Card {
    let critical = match (m.critical, *m == JUDGE, o.is_ok_and(|o| o.claude_code)) {
        (false, _, _) => "no",
        (true, false, _) => "yes",
        (true, true, true) => "yes: gate.engine is claude-code",
        (true, true, false) => "no: only while gate.engine is claude-code",
    };
    let mut rows = vec![row("critical", critical), row("needs", format!("Claude Code {} or later", m.min_claude_code))];
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
            item.path = Some(o.folder.join(m.name));
            item.version = o.shipped.clone();
            item = item.with_state(state(o.word));
            item.detail = match (o.attention.first(), o.word) {
                (Some(a), _) => Some(format!("critical: {}", a.why.word())),
                (None, S::Drift) if o.older => Some("an installed copy is older".into()),
                (None, S::Drift) => Some("repair converges it".into()),
                (None, S::Off) => Some("not wanted".into()),
                _ => None,
            };
            if !o.installed.is_empty() {
                rows.insert(0, row("installed", o.installed.join(", ")));
            }
        }
    }
    Card { title: m.name.into(), summary: "Claude Code mod".into(), item, fold_state: false, rows }
}
