//! The `theme` section of ways' user `config.yaml` (ADR-503, ADR-504 §5),
//! declared once. ways' schema includes these declarations as they are, and
//! attend's crates read the choice through them (`attend_config::theme`),
//! so there is one definition of the keys, their checks and their defaults.
//!
//! Declarations only: this module reads no file.

use agent_settings::schema::{DefaultValue, Kind, KeySpec, Scope, SectionSpec};
use agent_settings::Layer;
use serde_yaml::Value;

/// The file kind the section lives in: ways' `config.yaml`.
pub const FILE: &str = "config";

/// The lozenge shapes `theme.shape` takes, in the order the settings
/// screens cycle them.
pub const SHAPES: [&str; 6] = ["round", "plain", "flame", "arrow", "slant", "pixel"];

/// The section. No hook path reads it (ADR-504 §11).
pub const SECTION: SectionSpec = SectionSpec {
    name: "theme",
    file: FILE,
    top: &["theme"],
    per_entry: false,
    entry: None,
    repair: None,
    columns: None,
    doc: "The look of the interactive screens: the active theme and the lozenge shape (ADR-504).",
};

/// `theme.active`: the one user-level theme choice.
pub const ACTIVE: KeySpec = KeySpec {
    name: "theme.active",
    section: "theme",
    file: FILE,
    path: &["theme", "active"],
    kind: Kind::ChoiceOf { options: theme_names, multi: false },
    default: DefaultValue::Yaml("terminal"),
    instances: &[],
    scope: Scope::User,
    doc: "The theme the interactive screens draw with.",
    long: "A bundled theme or one in $XDG_CONFIG_HOME/agent-ways/themes (ADR-504 §5). terminal is the terminal's own 16 colours; another theme is used where the terminal shows 256 colours or more, and terminal in its place below that. The theme tab of `ways settings` previews and sets it.",
    check: Some(check_theme_name),
    computed: None,
    fail_closed: None,
};

/// `theme.shape`: the lozenge caps of tabs and the bottom bar.
pub const SHAPE: KeySpec = KeySpec {
    name: "theme.shape",
    path: &["theme", "shape"],
    kind: Kind::Choice(&SHAPES),
    default: DefaultValue::Yaml("plain"),
    doc: "The lozenge caps of tabs and the bottom bar.",
    long: "plain, the default, lets the coloured segments abut and works on any terminal font. Every other shape draws Nerd Font glyphs.",
    check: None,
    ..ACTIVE
};

/// The themes `theme.active` may name: the terminal's own colours, the
/// bundled themes, then each theme file in the user's themes directory. A
/// file that does not parse names nothing a screen could draw, so it is
/// left out. The list is read from the machine, not from the layers.
fn theme_names(_layers: &[Layer]) -> Result<Vec<String>, String> {
    let set = crate::ThemeSet::load(crate::bundled::user_dir().as_deref());
    let mut out = vec![crate::TERMINAL.to_string()];
    for (t, _) in set.list() {
        if !out.contains(&t.name) {
            out.push(t.name.clone());
        }
    }
    Ok(out)
}

/// A theme name as a theme file names itself: lowercase letters, digits
/// and `-`.
pub fn check_theme_name(v: &Value) -> Result<(), String> {
    match v.as_str() {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') => Ok(()),
        _ => Err("a theme name is lowercase letters, digits and -".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_check_names_and_shapes() {
        assert!(ACTIVE.check_value(&Value::from("nord")).is_ok());
        assert!(ACTIVE.check_value(&Value::from("Nord")).is_err());
        assert!(ACTIVE.check_value(&Value::from(3)).is_err());
        assert!(SHAPE.check_value(&Value::from("flame")).is_ok());
        assert!(SHAPE.check_value(&Value::from("star")).is_err());
        assert_eq!(SHAPE.section, "theme");
    }

    #[test]
    fn the_active_theme_is_one_of_terminal_and_the_bundled_themes() {
        let names = theme_names(&[]).unwrap();
        assert_eq!(names[0], "terminal");
        for (stem, _) in crate::BUNDLED {
            assert!(names.iter().any(|n| n == stem), "{stem} missing from {names:?}");
        }
        let mut sorted = names.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }
}
