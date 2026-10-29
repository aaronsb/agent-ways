//! A theme as an item: a bundled theme is builtin, a file in the user's
//! themes directory is the user's, and a file that takes a bundled theme's
//! name is that builtin, replaced.

use std::path::Path;

use agent_settings::item::{Family, How, Item, Origin, Override, ThemeState};
use agent_theme::{Background, Source, Theme};
use agent_tui::tree::Card;

use super::row;

/// `t`, read from `file` as `source` says; `active` when it is the chosen
/// theme. Its rows name where edits save (`saves_to`, none when nothing
/// saves) and the shape the screens draw with.
pub fn of_theme(t: &Theme, source: Source, file: Option<&Path>, active: bool, saves_to: Option<&Path>, shape: &str) -> Card {
    let (origin, path, overrides) = match source {
        Source::Bundled => (Origin::Builtin, None, vec![]),
        Source::User => (Origin::User, file.map(Path::to_path_buf), vec![]),
        Source::Override => (Origin::Builtin, None, file.map(|f| Override { origin: Origin::User, how: How::Replaces, path: f.to_path_buf() }).into_iter().collect()),
    };
    let state = if active { ThemeState::Active } else { ThemeState::Inactive };
    let item = Item { path, overrides, ..Item::new(Family::Theme, t.name.clone(), origin) }.with_state(state);
    let kind = if t.kind == agent_theme::Kind::Dark { "dark" } else { "light" };
    let ground = if t.background == Background::Fill { "fill" } else { "terminal" };
    let rows = vec![
        row("saves to", saves_to.map_or("none: nothing saves".to_string(), |d| d.display().to_string())),
        row("shape", format!("{shape} (a: shape cycles it)")),
    ];
    Card { title: t.label.clone(), summary: format!("{} · {kind} · background {ground}", t.name), item, fold_state: true, rows }
}
