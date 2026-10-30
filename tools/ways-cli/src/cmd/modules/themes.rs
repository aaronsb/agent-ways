//! A theme as an item: a bundled theme is builtin, a file in the user's
//! themes directory is the user's, a file that takes a bundled theme's name
//! is that builtin, replaced, and a file that does not load is the user's,
//! invalid.

use std::path::{Path, PathBuf};

use agent_settings::item::{Entry, Family, How, Item, Origin, Override, ThemeState};
use agent_theme::{Background, Source, Theme, ThemeError};
use serde::Serialize;

/// A theme's own fields, after its descriptor (ADR-516 §2). The screens
/// also draw whether it is dark or light and what its background is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeFields {
    /// The name it shows; null for a file that does not load.
    pub label: Option<String>,
    #[serde(skip)]
    pub kind: &'static str,
    #[serde(skip)]
    pub ground: &'static str,
}

/// `t`, read from `file` as `source` says; `active` when it is the chosen
/// theme.
pub fn item(t: &Theme, source: Source, file: Option<&Path>, active: bool) -> Entry<ThemeFields> {
    let (origin, path, overrides) = match source {
        Source::Bundled => (Origin::Builtin, None, vec![]),
        Source::User => (Origin::User, file.map(Path::to_path_buf), vec![]),
        Source::Override => (Origin::Builtin, None, file.map(|f| Override { origin: Origin::User, how: How::Replaces, path: f.to_path_buf() }).into_iter().collect()),
    };
    let state = if active { ThemeState::Active } else { ThemeState::Inactive };
    let item = Item { path, ..Item::new(Family::Theme, t.name.clone(), origin) }.with_overrides(overrides).with_state(state);
    let kind = if t.kind == agent_theme::Kind::Dark { "dark" } else { "light" };
    let ground = if t.background == Background::Fill { "fill" } else { "terminal" };
    Entry { item, fields: ThemeFields { label: Some(t.label.clone()), kind, ground } }
}

/// The user's theme file `file`, which did not load for `errors`: named by
/// its stem, `invalid`, with the first finding.
pub fn rejected(file: &Path, errors: &[ThemeError]) -> Entry<ThemeFields> {
    let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let item = Item { path: Some(file.to_path_buf()), detail: errors.first().map(ToString::to_string), ..Item::new(Family::Theme, name, Origin::User) };
    Entry { item: item.with_state(ThemeState::Invalid), fields: ThemeFields { label: None, kind: "", ground: "" } }
}

/// Every theme `themes` offers, in its order, then each user file that did
/// not load. A rejected file named for a bundled theme is not listed twice:
/// that theme says in its detail that the file does not load.
pub fn all(themes: &agent_tui::Themes) -> Vec<Entry<ThemeFields>> {
    let mut out: Vec<_> = themes
        .list()
        .into_iter()
        .map(|(t, s)| {
            let file = (s != Source::Bundled).then(|| themes.path_of(&t.name)).flatten();
            item(t, s, file.as_deref(), t.name == themes.active)
        })
        .collect();
    // A bundled theme that failed to parse is a build defect, which a test
    // catches; only the user's files are listed.
    for (f, e) in themes.set.rejected.iter().filter(|(f, _)| !f.starts_with("bundled:")) {
        let file = PathBuf::from(f);
        let bad = rejected(&file, e);
        match out.iter_mut().find(|o| o.item.name == bad.item.name) {
            Some(o) => {
                let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let why = bad.item.detail.unwrap_or_default();
                o.item.detail = Some(format!("user file {name} does not load: {why}; the bundled theme is in effect"));
            }
            None => out.push(bad),
        }
    }
    out
}
