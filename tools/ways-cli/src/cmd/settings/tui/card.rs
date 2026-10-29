//! A [`Module`] as the detail pane's card, paths under home written `~` and
//! with `/` between their parts, as the screens write every path.
//! Where a module was added follows its origin, unless its source names
//! the same file. A theme's card leaves its kind to the tab and puts its
//! state beside its origin, so the theme's own rows keep their room.

use std::path::Path;

use agent_tui::tree::{Card, Origin, StateTone};

use super::build::{tilde, tilde_text};
use crate::cmd::modules::{self, Health, Kind, Module};

pub fn card(m: &Module, home: &Path) -> Card {
    let theme = m.kind == Some(Kind::Theme);
    let at = m.path.as_deref().filter(|p| p.display().to_string() != m.source).map(|p| tilde(p, home));
    Card {
        title: m.name.clone(),
        summary: m.summary.clone(),
        origin: match m.origin {
            modules::Origin::Builtin => Origin::Builtin,
            modules::Origin::User => Origin::User,
            modules::Origin::Project => Origin::Project,
        },
        at,
        kind: m.kind.filter(|_| !theme).map(|k| k.word().to_string()),
        source: tilde_text(&m.source, home),
        version: m.version.clone(),
        state: (
            m.state.word.clone(),
            match m.state.health {
                Health::Running => StateTone::Ok,
                Health::Idle => StateTone::Plain,
                Health::Attention => StateTone::Warn,
                Health::Broken => StateTone::Err,
            },
        ),
        fold_state: theme,
        rows: m.rows.iter().map(|r| (r.label.clone(), tilde_text(&r.value, home))).collect(),
    }
}
