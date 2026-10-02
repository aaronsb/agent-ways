//! One-row strips: tabs as lozenges, and rows of chips such as a legend of
//! names Tab completes. A tab is the shell's look in every application:
//! the active one bold on the accent, the others on the accent stepped
//! back. In a chip row, the chips Tab would complete to are bold and
//! underlined; that is the one meaning underline has.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::app::theme::{Ground, Seg, Shape};

/// A tab's segment: bold on the accent when active, else on the accent
/// stepped back.
pub fn tab_seg(label: impl Into<String>, active: bool) -> Seg {
    if active {
        Seg::on(label, Ground::Accent).bold()
    } else {
        Seg::on(label, Ground::AccentDim)
    }
}

/// One tab as a lozenge in `shape`.
pub fn tab(shape: Shape, label: impl Into<String>, active: bool) -> Vec<Span<'static>> {
    shape.lozenge(&[tab_seg(label, active)])
}

/// `style` marked as the target of Tab completion when `target` is set.
pub fn target(style: Style, target: bool) -> Style {
    if target {
        style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        style
    }
}

/// Whether `name` starts with `partial`, ignoring ASCII case. An empty or
/// absent partial matches nothing: no chip is a target before anything is
/// typed.
pub fn prefix_match(name: &str, partial: Option<&str>) -> bool {
    partial.is_some_and(|p| !p.is_empty() && name.to_ascii_lowercase().starts_with(&p.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::theme::{set, Palette};
    use agent_theme::ColorDepth;

    #[test]
    fn the_active_tab_is_bold_on_the_accent() {
        set(Palette::terminal(ColorDepth::Ansi16));
        let on = tab(Shape::PLAIN, " a ", true);
        let off = tab(Shape::PLAIN, " b ", false);
        assert_eq!(on[0].style.bg, Some(Ground::Accent.bg()));
        assert!(on[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(off[0].style.bg, Some(Ground::AccentDim.bg()));
        set(Palette::default());
    }

    #[test]
    fn a_target_is_bold_and_underlined_and_needs_a_partial() {
        assert!(target(Style::new(), true).add_modifier.contains(Modifier::UNDERLINED | Modifier::BOLD));
        assert_eq!(target(Style::new(), false), Style::new());
        assert!(prefix_match("Tamsin", Some("tam")));
        assert!(!prefix_match("Tamsin", Some("")));
        assert!(!prefix_match("Tamsin", None));
        assert!(!prefix_match("Urban", Some("tam")));
    }
}
