//! The bundled themes, and the set that layers a user directory over them
//! (ADR-504 §5).

use std::path::{Path, PathBuf};

use crate::model::Theme;
use crate::text::{parse, ThemeError};

/// (file stem, source), in listing order. The default comes first.
pub const BUNDLED: [(&str, &str); 7] = [
    ("agent-ways", include_str!("../themes/agent-ways.theme")),
    ("nord", include_str!("../themes/nord.theme")),
    ("catppuccin-mocha", include_str!("../themes/catppuccin-mocha.theme")),
    ("dracula", include_str!("../themes/dracula.theme")),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.theme")),
    ("tokyonight-night", include_str!("../themes/tokyonight-night.theme")),
    ("paper", include_str!("../themes/paper.theme")),
];

/// The file extensions a user theme may have: the dotfiles palettes' own,
/// and TOML's.
pub const EXTENSIONS: [&str; 2] = ["theme", "toml"];

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
    /// Bundled themes only.
    pub fn bundled() -> ThemeSet {
        let mut set = ThemeSet::default();
        for (stem, src) in BUNDLED {
            match parse(src) {
                Ok(t) => set.entries.push((t, Source::Bundled)),
                Err(e) => set.rejected.push((format!("bundled:{stem}"), e)),
            }
        }
        set
    }

    /// Bundled themes, then every theme file in `user_dir` (when given and
    /// present), sorted by file name. A user theme with a bundled name takes
    /// its place in the list.
    pub fn load(user_dir: Option<&Path>) -> ThemeSet {
        let mut set = ThemeSet::bundled();
        let Some(dir) = user_dir else { return set };
        let Ok(rd) = std::fs::read_dir(dir) else { return set };
        let mut files: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| EXTENSIONS.contains(&x)))
            .collect();
        files.sort();
        for path in files {
            let file = path.display().to_string();
            let loaded = std::fs::read_to_string(&path).map_err(|e| vec![ThemeError::whole(e.to_string())]).and_then(|s| parse(&s));
            match loaded {
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

/// `$XDG_CONFIG_HOME/agent-ways/themes`, or `~/.config/agent-ways/themes`.
/// An empty or relative `XDG_CONFIG_HOME` is ignored, as the XDG spec says.
pub fn user_dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute());
    let base = xdg.or_else(|| std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("agent-ways").join("themes"))
}

/// The file beside the user themes that names the active one: its first
/// non-empty line is a theme name.
pub fn active_file(dir: &Path) -> PathBuf {
    dir.join("active")
}

/// The active theme's name, when `dir` names one.
pub fn active_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(active_file(dir)).ok()?;
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}
