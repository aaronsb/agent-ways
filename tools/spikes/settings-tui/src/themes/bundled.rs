//! The bundled themes, and the set that layers a user directory over them.

use std::path::Path;

use super::model::Theme;
use super::text::{parse, ThemeError};

/// (file stem, source), in listing order.
pub const BUNDLED: [(&str, &str); 7] = [
    ("agent-ways", include_str!("../../themes/agent-ways.toml")),
    ("nord", include_str!("../../themes/nord.toml")),
    ("catppuccin-mocha", include_str!("../../themes/catppuccin-mocha.toml")),
    ("dracula", include_str!("../../themes/dracula.toml")),
    ("gruvbox-dark", include_str!("../../themes/gruvbox-dark.toml")),
    ("tokyonight-night", include_str!("../../themes/tokyonight-night.toml")),
    ("paper", include_str!("../../themes/paper.toml")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Bundled,
    /// A user file with no bundled namesake.
    User,
    /// A user file replacing the bundled theme of the same name.
    Override,
}

#[derive(Debug, Default)]
pub struct ThemeSet {
    entries: Vec<(Theme, Source)>,
    /// Files that failed to load, as (file, errors). Their themes are absent.
    pub rejected: Vec<(String, Vec<ThemeError>)>,
}

impl ThemeSet {
    /// Bundled themes, then every `*.toml` in `user_dir` (when given and
    /// present), sorted by file name. A user theme with a bundled name takes
    /// its place in the list.
    pub fn load(user_dir: Option<&Path>) -> ThemeSet {
        let mut set = ThemeSet::default();
        for (stem, src) in BUNDLED {
            match parse(src) {
                Ok(t) => set.entries.push((t, Source::Bundled)),
                Err(e) => set.rejected.push((format!("bundled:{stem}"), e)),
            }
        }
        let Some(dir) = user_dir else { return set };
        let Ok(rd) = std::fs::read_dir(dir) else { return set };
        let mut files: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
        files.sort();
        for path in files {
            let file = path.display().to_string();
            match std::fs::read_to_string(&path).map_err(|e| vec![ThemeError { line: None, message: e.to_string() }]).and_then(|s| parse(&s)) {
                Ok(t) => set.insert(t),
                Err(e) => set.rejected.push((file, e)),
            }
        }
        set
    }

    fn insert(&mut self, t: Theme) {
        match self.entries.iter_mut().find(|(b, _)| b.name == t.name) {
            Some(slot) => {
                let src = if slot.1 == Source::Bundled { Source::Override } else { slot.1 };
                *slot = (t, src);
            }
            None => self.entries.push((t, Source::User)),
        }
    }

    pub fn list(&self) -> impl Iterator<Item = (&Theme, Source)> {
        self.entries.iter().map(|(t, s)| (t, *s))
    }

    pub fn get(&self, name: &str) -> Option<&Theme> {
        self.entries.iter().find(|(t, _)| t.name == name).map(|(t, _)| t)
    }
}
