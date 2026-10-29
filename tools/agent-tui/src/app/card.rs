//! A module's card ([`Card`]): the head of the detail pane on a mod, a
//! sensor or a theme, drawn the same on every tab.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::review::fields;
use super::theme;
use crate::tree::{Card, Origin, StateTone};

/// The origin word's role: builtin apart from what was added, at a glance.
pub(super) fn origin_style(o: Origin) -> Style {
    match o {
        Origin::Builtin => theme::info(),
        Origin::User | Origin::Project => theme::non_default().add_modifier(Modifier::BOLD),
    }
}

fn state_style(t: StateTone) -> Style {
    match t {
        StateTone::Ok => theme::ok(),
        StateTone::Plain => Style::new(),
        StateTone::Warn => theme::warn(),
        StateTone::Err => theme::err().add_modifier(Modifier::BOLD),
    }
}

/// The card's lines, each value wrapped to `room` columns beside its label:
/// the title, the summary, a blank line, then origin, kind, source,
/// version, state and the rows of its kind.
pub(super) fn card_lines(c: &Card, room: usize) -> Vec<Line<'static>> {
    let dim = theme::muted();
    let mut lines = vec![Line::styled(c.title.clone(), Style::new().add_modifier(Modifier::BOLD))];
    if !c.summary.is_empty() {
        lines.push(Line::styled(c.summary.clone(), dim));
    }
    lines.push(Line::raw(""));
    // The location follows the word, wrapped like any value; only the word
    // takes the origin's role.
    let word = c.origin.word();
    let value = match &c.at {
        Some(at) => format!("{word} · {at}"),
        None => word.to_string(),
    };
    let mut origin = fields("origin", &value, Style::new(), room, dim);
    if let Some(first) = origin.first_mut().and_then(|l| l.spans.get_mut(1)) {
        let rest: String = first.content.chars().skip(word.chars().count()).collect();
        let head = Span::styled(word.to_string(), origin_style(c.origin));
        *first = head;
        origin[0].spans.push(Span::raw(rest));
    }
    lines.extend(origin);
    if let Some(k) = &c.kind {
        lines.extend(fields("kind", k, Style::new(), room, dim));
    }
    lines.extend(fields("source", &c.source, Style::new(), room, dim));
    if let Some(v) = &c.version {
        lines.extend(fields("version", v, Style::new(), room, dim));
    }
    lines.extend(fields("state", &c.state.0, state_style(c.state.1), room, dim));
    for (k, v) in &c.rows {
        lines.extend(fields(k, v, Style::new(), room, dim));
    }
    lines
}
