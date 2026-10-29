//! A theme as a [`Module`]: a bundled theme is builtin and read-only, a
//! file in the user's themes directory is the user's, and one that takes a
//! bundled theme's name says so in its rows.

use std::path::Path;

use agent_theme::{Background, Source, Theme};

use super::{Health, Kind, Module, Origin, Row, State};

/// `t`, read from `file` as `source` says; `active` when it is the chosen
/// theme. Its rows name where edits save (`saves_to`, none when nothing
/// saves) and the shape the screens draw with.
pub fn of_theme(t: &Theme, source: Source, file: Option<&Path>, active: bool, saves_to: Option<&Path>, shape: &str) -> Module {
    let mut rows = Vec::new();
    if source == Source::Override {
        rows.push(Row::new("overrides", format!("the bundled {}", t.name)));
    }
    rows.push(Row::new("saves to", saves_to.map_or("none: nothing saves".to_string(), |d| d.display().to_string())));
    rows.push(Row::new("shape", format!("{shape} (a: shape cycles it)")));
    let (origin, path, source) = match source {
        Source::Bundled => (Origin::Builtin, None, "bundled (read-only)".to_string()),
        Source::User | Source::Override => (Origin::User, file.map(Path::to_path_buf), file.map(|f| f.display().to_string()).unwrap_or_default()),
    };
    let kind = if t.kind == agent_theme::Kind::Dark { "dark" } else { "light" };
    let ground = if t.background == Background::Fill { "fill" } else { "terminal" };
    Module {
        name: t.label.clone(),
        summary: format!("{} · {kind} · background {ground}", t.name),
        kind: Some(Kind::Theme),
        origin,
        path,
        source,
        version: None,
        state: if active { State::new("active", Health::Running) } else { State::new("inactive", Health::Idle) },
        rows,
    }
}
