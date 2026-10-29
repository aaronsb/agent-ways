//! An item's card ([`Card`]): the head of the detail pane on a mod, a
//! sensor or a theme, drawn the same on every tab from the item's
//! descriptor (ADR-516 §6).

use agent_settings::item::{Origin, Tone};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::review::fields;
use super::theme;
use crate::tree::Card;

/// The origin word's role: builtin apart from what was added, at a glance.
pub(super) fn origin_style(o: Origin) -> Style {
    match o {
        Origin::Builtin => theme::info(),
        Origin::User | Origin::Project => theme::non_default().add_modifier(Modifier::BOLD),
    }
}

fn tone_style(t: Tone) -> Style {
    match t {
        Tone::Ok => theme::ok(),
        Tone::Plain => Style::new(),
        Tone::Warn => theme::warn(),
        Tone::Err => theme::err().add_modifier(Modifier::BOLD),
    }
}

/// A row whose value opens with `word` in `style`, the rest plain, wrapped
/// to `room` like any value.
fn headed(label: &str, word: &str, style: Style, rest: &str, room: usize, dim: Style) -> Vec<Line<'static>> {
    let mut lines = fields(label, &format!("{word}{rest}"), Style::new(), room, dim);
    if let Some(first) = lines.first_mut().filter(|l| l.spans.get(1).is_some_and(|s| s.content.starts_with(word))) {
        let tail: String = first.spans[1].content.chars().skip(word.chars().count()).collect();
        first.spans[1] = Span::styled(word.to_string(), style);
        first.spans.push(Span::raw(tail));
    }
    lines
}

/// ` · <text>`, or nothing.
fn after(text: Option<&str>) -> String {
    text.map(|t| format!(" · {t}")).unwrap_or_default()
}

/// The card's lines, each value wrapped to `room` columns beside its label:
/// the title, the summary, a blank line, then origin, each override, kind,
/// source, version, state and the rows of its family. A field the item
/// leaves null draws no row. A folded state follows the last override, or
/// the origin, on its line.
pub(super) fn card_lines(c: &Card, room: usize) -> Vec<Line<'static>> {
    let dim = theme::muted();
    let i = &c.item;
    let mut lines = vec![Line::styled(c.title.clone(), Style::new().add_modifier(Modifier::BOLD))];
    if !c.summary.is_empty() {
        lines.push(Line::styled(c.summary.clone(), dim));
    }
    lines.push(Line::raw(""));
    let path = |p: Option<&std::path::Path>| p.map(|p| p.display().to_string());
    let mut head = headed("origin", i.origin.word(), origin_style(i.origin), &after(path(i.path.as_deref()).as_deref()), room, dim);
    for o in &i.overrides {
        let rest = format!(" {}{}", o.how.word(), after(path(o.path.as_deref()).as_deref()));
        head.extend(headed("override", o.origin.word(), origin_style(o.origin), &rest, room, dim));
    }
    let word = i.state.map(|s| (s.word(), tone_style(s.tone())));
    let detail = i.detail.as_deref();
    // A folded state rides the block's last line while it fits there, and
    // takes a row of its own when it would not.
    let width = |l: &Line| l.spans.iter().map(|s| crate::wrap::str_width(&s.content)).sum::<usize>();
    let state_width = word.map_or(0, |(w, _)| crate::wrap::str_width(w)) + crate::wrap::str_width(&after(detail));
    let folds = c.fold_state && word.is_some() && head.last().is_some_and(|l| width(l) + 3 + state_width <= super::review::LABEL + room);
    if let (true, Some((w, style)), Some(last)) = (folds, word, head.last_mut()) {
        last.spans.push(Span::raw(" · "));
        last.spans.push(Span::styled(w.to_string(), style));
        last.spans.push(Span::raw(after(detail)));
    }
    lines.extend(head);
    let mut plain = |label: &str, v: Option<&str>| {
        if let Some(v) = v {
            lines.extend(fields(label, v, Style::new(), room, dim));
        }
    };
    plain("kind", i.kind.map(|k| k.word()));
    plain("source", i.source.as_deref());
    plain("version", i.version.as_deref());
    match (folds, word, detail) {
        (true, _, _) | (false, None, None) => {}
        (false, Some((w, style)), _) => lines.extend(headed("state", w, style, &after(detail), room, dim)),
        (false, None, Some(d)) => lines.extend(fields("state", d, Style::new(), room, dim)),
    }
    for (k, v) in &c.rows {
        lines.extend(fields(k, v, Style::new(), room, dim));
    }
    lines
}
