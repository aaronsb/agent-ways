//! An item's card as the detail pane draws it: paths under home written `~`
//! and with `/` between their parts, as the screens write every path.

use std::path::{Path, PathBuf};

use agent_tui::tree::Card;

use super::build::{tilde, tilde_text};

pub fn shown(mut c: Card, home: &Path) -> Card {
    let at = |p: &Path| PathBuf::from(tilde(p, home));
    c.item.path = c.item.path.as_deref().map(at);
    c.item.overrides.iter_mut().for_each(|o| o.path = at(&o.path));
    c.item.source = c.item.source.map(|s| tilde_text(&s, home));
    c.item.detail = c.item.detail.map(|s| tilde_text(&s, home));
    for (_, v) in &mut c.rows {
        *v = tilde_text(v, home);
    }
    c
}
