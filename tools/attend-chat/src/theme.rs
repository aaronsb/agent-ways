//! The look the chat draws with: the one user-level theme choice of
//! agent-ways (ADR-504), read only through `attend_config::theme`, which
//! reads ways' `theme.active` and `theme.shape` with the declarations
//! `agent_theme::settings` holds for both. A `theme` section that fails its
//! checks gives the defaults, as it does for `ways`.
//!
//! The theme rule is agent-theme's ([`Painter::select`]): the 16-colour
//! terminal palette is the default, and a chosen theme applies only where
//! the terminal shows 256 colours or truecolor. `NO_COLOR` and a dumb
//! terminal mean no colour.

use std::path::Path;

use agent_theme::{ColorDepth, Painter};
use agent_tui::theme::{Palette, Shape};

/// What the user file chose: the theme's name and the shape, `plain` by
/// default.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub active: Option<String>,
    pub shape: Shape,
}

impl From<attend_config::theme::Choice> for Choice {
    fn from(c: attend_config::theme::Choice) -> Choice {
        Choice { active: c.active, shape: c.shape.as_deref().map_or(Shape::PLAIN, Shape::named) }
    }
}

/// The choice in `config.yaml` under `dir`.
pub fn choice_in(dir: &Path) -> Choice {
    attend_config::theme::choice_in(dir).into()
}

/// The user's choice, or the defaults when there is no config directory.
pub fn choice() -> Choice {
    attend_config::theme::choice().into()
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
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("attend-chat-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_choice_maps_to_a_shape_with_plain_the_default() {
        let d = scratch("map");
        assert_eq!(choice_in(&d), Choice { active: None, shape: Shape::PLAIN });
        std::fs::write(d.join("config.yaml"), "theme:\n  active: nord\n  shape: round\n").unwrap();
        assert_eq!(choice_in(&d), Choice { active: Some("nord".into()), shape: Shape::ROUND });
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
