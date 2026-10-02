//! The look the chat draws with: the one user-level theme choice of
//! agent-ways (ADR-504), read only.
//!
//! The choice is `theme.active`, with the lozenge shape `theme.shape`
//! beside it, in the user `config.yaml` that `ways settings` writes. The
//! keys are ways' (ADR-503, ways-core `settings.rs`); the chat never
//! writes them, and reads them through the settings machinery with a
//! schema of the `theme` section alone, so a file that fails the section's
//! checks falls back to the defaults exactly as it does for `ways`. When
//! attend's settings move onto the registry (#698), this read moves with
//! them.
//!
//! The theme rule is agent-theme's ([`Painter::select`]): the 16-colour
//! terminal palette is the default, and a chosen theme applies only where
//! the terminal shows 256 colours or truecolor. `NO_COLOR` and a dumb
//! terminal mean no colour.

use std::path::{Path, PathBuf};

use agent_settings::schema::{DefaultValue, FileSpec, Kind, KeySpec, LayerScope, Schema, Scope, SectionSpec};
use agent_settings::Layer;
use agent_theme::{ColorDepth, Painter};
use agent_tui::theme::{Palette, Shape};

const FILE: &str = "config";

const KEY: KeySpec = KeySpec {
    name: "",
    section: "theme",
    file: FILE,
    path: &[],
    kind: Kind::Text,
    default: DefaultValue::None,
    instances: &[],
    scope: Scope::User,
    doc: "",
    long: "",
    check: None,
    computed: None,
    fail_closed: None,
};

const KEYS: &[KeySpec] = &[
    KeySpec {
        name: "theme.active",
        path: &["theme", "active"],
        default: DefaultValue::Yaml("terminal"),
        check: Some(check_theme_name),
        ..KEY
    },
    KeySpec {
        name: "theme.shape",
        path: &["theme", "shape"],
        kind: Kind::Choice(&Shape::NAMES),
        default: DefaultValue::Yaml("plain"),
        ..KEY
    },
];

/// A theme name as a theme file names itself: `[a-z0-9-]+`, the check
/// ways makes, so a name ways rejects falls back here too.
fn check_theme_name(v: &serde_yaml::Value) -> Result<(), String> {
    match v.as_str() {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') => Ok(()),
        _ => Err("a theme name is lowercase letters, digits and -".into()),
    }
}

/// The `theme` section of ways' user file, as read here.
static SCHEMA: Schema = Schema {
    component: "ways",
    files: &[FileSpec { id: FILE, retired: &[] }],
    sections: &[SectionSpec {
        name: "theme",
        file: FILE,
        top: &["theme"],
        per_entry: false,
        repair: None,
        doc: "The look of the interactive screens.",
    }],
    keys: KEYS,
};

/// The agent-ways config directory: the themes directory's parent, so
/// both follow one rule (`$XDG_CONFIG_HOME/agent-ways`, else
/// `~/.config/agent-ways`).
fn config_dir() -> Option<PathBuf> {
    agent_theme::user_dir().and_then(|d| d.parent().map(Path::to_path_buf))
}

/// What the user file chose: the theme's name and the shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub active: Option<String>,
    pub shape: Shape,
}

/// Read the choice from `config.yaml` under `dir`. A missing file, or a
/// section that fails its checks, gives the defaults: the terminal
/// palette, and the plain shape.
pub fn choice_in(dir: &Path) -> Choice {
    let layer = Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &dir.join("config.yaml"));
    let text = |key: &str| layer.get(&key.split('.').map(str::to_string).collect::<Vec<_>>()).and_then(|v| v.as_str().map(str::to_string));
    Choice { active: text("theme.active"), shape: text("theme.shape").map_or(Shape::PLAIN, |s| Shape::named(&s)) }
}

/// The user's choice, or the defaults when there is no config directory.
pub fn choice() -> Choice {
    config_dir().map_or(Choice { active: None, shape: Shape::PLAIN }, |d| choice_in(&d))
}

/// The palette to draw with at `depth`: the chosen theme from the
/// bundled and user themes under the rule above. A warning names a
/// choice that could not be honoured.
pub fn palette(active: Option<&str>, depth: ColorDepth) -> (Palette, Option<String>) {
    let Some(dir) = agent_theme::user_dir() else { return (Palette::terminal(depth), None) };
    let (painter, warning) = Painter::named_in(active, &dir, depth);
    (Palette { painter }, warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-chat-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_file_is_the_terminal_palette_and_the_plain_shape() {
        let d = scratch("none");
        assert_eq!(choice_in(&d), Choice { active: None, shape: Shape::PLAIN });
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_user_file_names_the_theme_and_the_shape() {
        let d = scratch("set");
        std::fs::write(d.join("config.yaml"), "theme:\n  active: nord\n  shape: round\n").unwrap();
        assert_eq!(choice_in(&d), Choice { active: Some("nord".into()), shape: Shape::ROUND });
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bad_section_falls_back_whole() {
        let d = scratch("bad");
        std::fs::write(d.join("config.yaml"), "theme:\n  active: nord\n  shape: zigzag\n").unwrap();
        assert_eq!(choice_in(&d), Choice { active: None, shape: Shape::PLAIN });
        std::fs::write(d.join("config.yaml"), "theme:\n  active: Nord\n").unwrap();
        assert_eq!(choice_in(&d).active, None, "a name ways rejects is not used");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_chosen_theme_applies_only_at_256_colours_or_more() {
        let dir = scratch("themes");
        let nord = |depth| Painter::named_in(Some("nord"), &dir, depth).0;
        assert!(nord(ColorDepth::TrueColor).roles().is_some());
        assert!(nord(ColorDepth::Ansi256).roles().is_some());
        assert_eq!(nord(ColorDepth::Ansi16), Painter::terminal(ColorDepth::Ansi16), "16 colours: the terminal palette whole");
        assert_eq!(nord(ColorDepth::NoColor), Painter::terminal(ColorDepth::NoColor));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
