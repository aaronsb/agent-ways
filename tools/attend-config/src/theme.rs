//! The one user-level theme choice, read only (ADR-503, ADR-504 §5). The
//! keys are ways' `theme.active` and `theme.shape`, in the user
//! `config.yaml` that `ways settings` writes; their declarations are
//! `agent_theme::settings`, the same ones ways' schema holds. attend keeps
//! no theme key of its own. A crate on attend's side (attend-chat) reads the
//! choice here, never writes it, and a `theme` section that fails its
//! checks gives the defaults, as it does for ways.
//!
//! This reads ways' config file, so a hook path never calls it (ADR-504
//! §11).

use std::path::{Path, PathBuf};

use agent_settings::schema::{FileSpec, LayerScope, Schema};
use agent_settings::Layer;
use agent_theme::settings::{ACTIVE, FILE, SECTION, SHAPE};

/// ways' user file, as far as the `theme` section goes.
static SCHEMA: Schema = Schema { component: "ways", files: &[FileSpec { id: FILE, retired: &[] }], sections: &[SECTION], keys: &[ACTIVE, SHAPE] };

/// What the user file chose. `None` is the default: the terminal palette,
/// and the plain shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    pub active: Option<String>,
    /// One of `agent_theme::settings::SHAPES`.
    pub shape: Option<String>,
}

/// The agent-ways config directory, `$XDG_CONFIG_HOME/agent-ways` or
/// `~/.config/agent-ways`: the themes directory's parent, so both follow
/// one rule.
pub fn config_dir() -> Option<PathBuf> {
    agent_theme::user_dir().and_then(|d| d.parent().map(Path::to_path_buf))
}

/// The choice in `config.yaml` under `dir`. A missing file, or a `theme`
/// section that fails its checks, gives the defaults.
pub fn choice_in(dir: &Path) -> Choice {
    let layer = Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &dir.join("config.yaml"));
    let text = |path: &[&str]| {
        let p: Vec<String> = path.iter().map(|s| s.to_string()).collect();
        layer.get(&p).and_then(|v| v.as_str().map(str::to_string))
    };
    Choice { active: text(ACTIVE.path), shape: text(SHAPE.path) }
}

/// The user's choice, or the defaults when there is no config directory.
pub fn choice() -> Choice {
    config_dir().map(|d| choice_in(&d)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str, text: Option<&str>) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-config-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        if let Some(t) = text {
            std::fs::write(d.join("config.yaml"), t).unwrap();
        }
        d
    }

    #[test]
    fn the_choice_is_read_from_ways_file_and_a_bad_section_gives_the_defaults() {
        assert_eq!(choice_in(&dir("none", None)), Choice::default());
        // Every other ways key is someone else's: it changes nothing here.
        let d = dir("set", Some("language: es\ntheme:\n  active: nord\n  shape: flame\n"));
        assert_eq!(choice_in(&d), Choice { active: Some("nord".into()), shape: Some("flame".into()) });
        let d = dir("bad", Some("theme:\n  active: Nord!\n  shape: flame\n"));
        assert_eq!(choice_in(&d), Choice::default(), "the section falls back whole, as in ways");
        let d = dir("broken", Some("theme: [\n"));
        assert_eq!(choice_in(&d), Choice::default());
    }
}
