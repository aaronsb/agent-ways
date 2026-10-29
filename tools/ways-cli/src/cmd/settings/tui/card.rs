//! A [`Module`] as the detail pane's card, paths under home written `~`.
//! Where a module was added follows its origin, unless its source names
//! the same file.

use std::path::Path;

use agent_tui::tree::{Card, Origin, StateTone};

use super::build::{tilde, tilde_home};
use crate::cmd::modules::{self, Health, Module};

pub fn card(m: &Module, home: &Path) -> Card {
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
        kind: Some(m.kind.word().to_string()),
        source: tilde_home(&m.source, home),
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
        rows: m.rows.iter().map(|r| (r.label.clone(), tilde_home(&r.value, home))).collect(),
    }
}
