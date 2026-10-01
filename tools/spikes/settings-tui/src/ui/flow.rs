//! A guided flow: a short run of steps in a modal, for a job that takes
//! several decisions. Pick from a list, preview what would change, confirm.
//! Finishing hands back the commands to queue; the tab that launched the flow
//! queues them, so they meet that tab's review like any other pending item.
//!
//! Nothing here knows about ways. An adapter supplies the candidates and two
//! closures: what to preview for the picks, and what finishing queues.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, HighlightSpacing, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::render::{button_row, elide, modal_rect, pane};
use super::theme::{self, Shape};
use super::Btn;

/// The colour family of a badge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tone {
    Ok,
    Warn,
    Err,
    Accent,
    Muted,
}

/// One row of a picker: a label, a detail line and a state badge.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// What the flow's closures get back; a path, for the ways adapter.
    pub id: String,
    pub label: String,
    pub detail: String,
    pub badge: String,
    pub tone: Tone,
    /// False: shown with its badge, but there is nothing to pick.
    pub pickable: bool,
}

impl Candidate {
    pub fn new(id: impl Into<String>, label: impl Into<String>, detail: impl Into<String>, badge: impl Into<String>, tone: Tone) -> Self {
        Candidate { id: id.into(), label: label.into(), detail: detail.into(), badge: badge.into(), tone, pickable: true }
    }
    pub fn unpickable(mut self) -> Self {
        self.pickable = false;
        self
    }
}

/// How a preview line reads: what happens to the thing it names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    Heading,
    Plain,
    Kept,
    Added,
    Replaced,
    Removed,
    Refused,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PLine {
    pub verb: Verb,
    pub text: String,
}

impl PLine {
    pub fn new(verb: Verb, text: impl Into<String>) -> Self {
        PLine { verb, text: text.into() }
    }
}

/// One command a finished flow asks its tab to queue.
#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub label: String,
    pub command: String,
    pub confirm: bool,
}

impl Out {
    pub fn new(label: impl Into<String>, command: impl Into<String>, confirm: bool) -> Self {
        Out { label: label.into(), command: command.into(), confirm }
    }
}

/// What a key or click did to the flow.
#[derive(Debug, PartialEq)]
pub enum FlowEvent {
    Stay,
    Cancel,
    /// Finished: queue `outs` under the node keyed `key`.
    Finish { key: String, outs: Vec<Out> },
}

struct Check {
    label: String,
    on: bool,
}

/// Text entry in the picker, when one is open.
enum Entry {
    Off,
    Filter,
    Path(String),
}

#[derive(Default)]
struct Hits {
    list: Rect,
    buttons: Vec<(Rect, Btn)>,
    checks: Vec<(Rect, usize)>,
}

type PreviewFn = Box<dyn Fn(&[&Candidate]) -> Vec<PLine>>;
type FinishFn = Box<dyn Fn(&[&Candidate], &[bool]) -> Vec<Out>>;
type OtherFn = Box<dyn Fn(&str) -> Candidate>;

const STEPS: [&str; 3] = ["pick", "preview", "confirm"];

pub struct Flow {
    /// The dotted key of the node that launched it; finished commands queue under it.
    pub key: String,
    title: String,
    notes: [String; 3],
    multi: bool,
    cands: Vec<Candidate>,
    picked: Vec<bool>,
    /// The cursor among the visible rows: the matching candidates, then "Other…".
    cursor: usize,
    filter: String,
    entry: Entry,
    other: Option<OtherFn>,
    preview: PreviewFn,
    lines: Vec<PLine>,
    scroll: usize,
    /// Rows the preview showed at the last draw.
    view: usize,
    checks: Vec<Check>,
    check_at: usize,
    finish: FinishFn,
    step: usize,
    note: String,
    list: ListState,
    hits: Hits,
}

impl Flow {
    pub fn new(
        title: impl Into<String>,
        multi: bool,
        cands: Vec<Candidate>,
        preview: impl Fn(&[&Candidate]) -> Vec<PLine> + 'static,
        finish: impl Fn(&[&Candidate], &[bool]) -> Vec<Out> + 'static,
    ) -> Self {
        Flow {
            key: String::new(),
            title: title.into(),
            notes: ["choose what to include".into(), "what would change; nothing has run".into(), "finishing queues these".into()],
            multi,
            picked: vec![false; cands.len()],
            cands,
            cursor: 0,
            filter: String::new(),
            entry: Entry::Off,
            other: None,
            preview: Box::new(preview),
            lines: Vec::new(),
            scroll: 0,
            view: 10,
            checks: Vec::new(),
            check_at: 0,
            finish: Box::new(finish),
            step: 0,
            note: String::new(),
            list: ListState::default(),
            hits: Hits::default(),
        }
    }

    /// An "Other…" row that takes a typed path; `f` turns it into a candidate.
    pub fn other(mut self, f: impl Fn(&str) -> Candidate + 'static) -> Self {
        self.other = Some(Box::new(f));
        self
    }

    /// The one-line guide under the step header, for each step.
    pub fn notes(mut self, pick: &str, preview: &str, confirm: &str) -> Self {
        self.notes = [pick.into(), preview.into(), confirm.into()];
        self
    }

    /// A checkbox on the confirm step; `finish` gets its state.
    pub fn check(mut self, label: impl Into<String>, on: bool) -> Self {
        self.checks.push(Check { label: label.into(), on });
        self
    }

    #[cfg(test)]
    pub fn candidates(&self) -> &[Candidate] {
        &self.cands
    }

    #[cfg(test)]
    pub fn step(&self) -> usize {
        self.step
    }

    #[cfg(test)]
    pub fn picked_ids(&self) -> Vec<String> {
        self.picks().iter().map(|c| c.id.clone()).collect()
    }

    fn picks(&self) -> Vec<&Candidate> {
        self.cands.iter().zip(&self.picked).filter(|(_, p)| **p).map(|(c, _)| c).collect()
    }

    /// Candidates the filter lets through, as indexes.
    fn visible(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        (0..self.cands.len())
            .filter(|&i| {
                let c = &self.cands[i];
                f.is_empty() || [&c.label, &c.detail, &c.badge].iter().any(|t| t.to_lowercase().contains(&f))
            })
            .collect()
    }

    fn rows(&self) -> usize {
        self.visible().len() + self.other.is_some() as usize
    }

    /// The candidate under the cursor, or None on the "Other…" row.
    fn under_cursor(&self) -> Option<usize> {
        self.visible().get(self.cursor).copied()
    }

    fn select(&mut self, i: usize) {
        if !self.multi {
            self.picked.fill(false);
        }
        self.picked[i] = true;
    }

    fn toggle(&mut self, i: usize) {
        let c = &self.cands[i];
        if !c.pickable {
            self.note = format!("{} is {}: nothing to pick", c.label, c.badge);
        } else if self.multi || self.picked[i] {
            self.picked[i] = !self.picked[i];
        } else {
            self.select(i);
        }
    }

    pub fn key(&mut self, k: KeyEvent) -> FlowEvent {
        if self.step == 0 && !matches!(self.entry, Entry::Off) {
            return self.entry_key(k);
        }
        self.note.clear();
        let last = self.rows().saturating_sub(1);
        match k.code {
            KeyCode::Char('q') => return FlowEvent::Cancel,
            KeyCode::Esc if self.step == 0 && self.filter.is_empty() => return FlowEvent::Cancel,
            KeyCode::Esc if self.step == 0 => self.filter.clear(),
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Left if self.step > 0 => self.step -= 1,
            KeyCode::Enter => return self.enter(),
            KeyCode::Right | KeyCode::Char('n') => return self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.go(-1, last),
            KeyCode::Down | KeyCode::Char('j') => self.go(1, last),
            KeyCode::PageUp => self.go(-10, last),
            KeyCode::PageDown => self.go(10, last),
            KeyCode::Home | KeyCode::Char('g') => self.go(isize::MIN / 2, last),
            KeyCode::End | KeyCode::Char('G') => self.go(isize::MAX / 2, last),
            KeyCode::Char('/') if self.step == 0 => self.entry = Entry::Filter,
            KeyCode::Char(' ') => self.space(),
            _ => {}
        }
        FlowEvent::Stay
    }

    /// Move the cursor, the preview's scroll or the options' cursor by `by`.
    fn go(&mut self, by: isize, last: usize) {
        let step = |at: usize, max: usize| (at as isize).saturating_add(by).clamp(0, max as isize) as usize;
        match self.step {
            0 => self.cursor = step(self.cursor, last),
            1 => self.scroll = step(self.scroll, self.lines.len().saturating_sub(self.view)),
            _ => self.check_at = step(self.check_at, self.checks.len().saturating_sub(1)),
        }
    }

    fn space(&mut self) {
        match self.step {
            0 => match self.under_cursor() {
                Some(i) => self.toggle(i),
                None if self.other.is_some() => self.entry = Entry::Path(String::new()),
                None => {}
            },
            2 => {
                if let Some(c) = self.checks.get_mut(self.check_at) {
                    c.on = !c.on;
                }
            }
            _ => {}
        }
    }

    /// Enter: on the picker it opens "Other…", or picks the row when nothing
    /// is picked yet, then goes on.
    fn enter(&mut self) -> FlowEvent {
        if self.step == 0 {
            match self.under_cursor() {
                None if self.other.is_some() => {
                    self.entry = Entry::Path(String::new());
                    return FlowEvent::Stay;
                }
                Some(i) if self.picked.iter().all(|p| !p) => self.toggle(i),
                _ => {}
            }
        }
        self.next()
    }

    fn next(&mut self) -> FlowEvent {
        match self.step {
            0 => {
                let picks = self.picks();
                if picks.is_empty() {
                    if self.note.is_empty() {
                        self.note = "nothing picked yet: Space marks a row".into();
                    }
                    return FlowEvent::Stay;
                }
                self.lines = (self.preview)(&picks);
                self.scroll = 0;
                self.step = 1;
            }
            1 => {
                self.check_at = 0;
                self.step = 2;
            }
            _ => {
                let on: Vec<bool> = self.checks.iter().map(|c| c.on).collect();
                return FlowEvent::Finish { key: self.key.clone(), outs: (self.finish)(&self.picks(), &on) };
            }
        }
        FlowEvent::Stay
    }

    fn entry_key(&mut self, k: KeyEvent) -> FlowEvent {
        let typed = match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => Some(c),
            _ => None,
        };
        match (&mut self.entry, k.code) {
            (Entry::Filter, KeyCode::Esc) => {
                self.filter.clear();
                self.entry = Entry::Off;
            }
            (Entry::Filter, KeyCode::Enter) => self.entry = Entry::Off,
            (Entry::Filter, KeyCode::Backspace) => {
                self.filter.pop();
            }
            (Entry::Filter, _) => {
                if let Some(c) = typed {
                    self.filter.push(c);
                    self.cursor = 0;
                }
            }
            (Entry::Path(_), KeyCode::Esc) => self.entry = Entry::Off,
            (Entry::Path(buf), KeyCode::Enter) => {
                let text = buf.trim().to_string();
                if !text.is_empty() {
                    self.entry = Entry::Off;
                    self.add_other(&text);
                }
            }
            (Entry::Path(buf), KeyCode::Backspace) => {
                buf.pop();
            }
            (Entry::Path(buf), _) => {
                if let Some(c) = typed {
                    buf.push(c);
                }
            }
            (Entry::Off, _) => {}
        }
        FlowEvent::Stay
    }

    /// A typed path becomes a candidate, picked when it can be.
    fn add_other(&mut self, text: &str) {
        let Some(make) = &self.other else { return };
        let c = make(text);
        let i = match self.cands.iter().position(|x| x.id == c.id) {
            Some(i) => i,
            None => {
                self.cands.push(c);
                self.picked.push(false);
                self.cands.len() - 1
            }
        };
        self.filter.clear();
        self.cursor = i;
        if self.cands[i].pickable {
            self.select(i);
        } else {
            self.note = format!("{} is {}: nothing to pick", self.cands[i].label, self.cands[i].badge);
        }
    }

    /// A click: a button, a row of the picker, or an option.
    pub fn click(&mut self, at: Position) -> FlowEvent {
        let typing = self.step == 0 && !matches!(self.entry, Entry::Off);
        if let Some(&(_, b)) = self.hits.buttons.iter().find(|(r, _)| r.contains(at)) {
            return match b {
                Btn::Cancel => FlowEvent::Cancel,
                _ if typing => FlowEvent::Stay,
                _ => self.key(KeyEvent::new(b.key(), KeyModifiers::NONE)),
            };
        }
        if typing {
            return FlowEvent::Stay;
        }
        match self.step {
            0 if self.hits.list.contains(at) => {
                // Each candidate takes two lines: its label, then its detail.
                let row = self.list.offset() + (at.y - self.hits.list.y) as usize / 2;
                if row < self.rows() {
                    self.cursor = row;
                    self.note.clear();
                    self.space();
                }
            }
            2 => {
                if let Some(&(_, i)) = self.hits.checks.iter().find(|(r, _)| r.contains(at)) {
                    self.check_at = i;
                    self.space();
                }
            }
            _ => {}
        }
        FlowEvent::Stay
    }

    /// The status line's hint for what the keys do now.
    pub fn hint(&self) -> &'static str {
        match (&self.entry, self.step) {
            (Entry::Filter, _) => "  type to filter · Enter keeps it · Esc clears",
            (Entry::Path(_), _) => "  type a path · Enter adds it · Esc cancels",
            (_, 0) => "  ↑↓ move · Space marks · Enter next · / filters · q or Esc cancels",
            (_, 1) => "  ↑↓ PgUp PgDn scroll · Enter next · Esc back · q cancels",
            _ => "  Space toggles an option · Enter finishes · Esc back · q cancels",
        }
    }

    pub(super) fn draw(&mut self, f: &mut Frame, area: Rect, sh: Shape) {
        let r = modal_rect(area, 100, 26);
        f.render_widget(Clear, r);
        let block = pane(self.title.clone()).border_style(Style::new().fg(theme::ACCENT_DIM));
        let inner = block.inner(r);
        f.render_widget(block, r);
        let [head, sub, body, foot, btns] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
        f.render_widget(Paragraph::new(self.header()), head);
        f.render_widget(Paragraph::new(Line::styled(format!(" {}", self.notes[self.step]), theme::hint())), sub);
        self.hits.list = Rect::default();
        self.hits.checks.clear();
        match self.step {
            0 => self.draw_picker(f, body),
            1 => self.draw_preview(f, body),
            _ => self.draw_confirm(f, body),
        }
        f.render_widget(Paragraph::new(self.footer()), foot);
        let go = if self.step == 2 { (Btn::Finish, "Finish (Enter)") } else { (Btn::Next, "Next (Enter)") };
        let mut row = Vec::new();
        if self.step > 0 {
            row.push((Btn::Back, "Back (Esc)".to_string()));
        }
        row.extend([(go.0, go.1.to_string()), (Btn::Cancel, "Cancel (q)".to_string())]);
        self.hits.buttons.clear();
        f.render_widget(Paragraph::new(button_row(sh, btns, Some(go.0), &row, &mut self.hits.buttons)), btns);
    }

    /// `1 pick · 2 preview · 3 confirm`, the current step on the accent.
    fn header(&self) -> Line<'static> {
        let mut spans = vec![Span::raw(" ")];
        for (i, name) in STEPS.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", theme::rule()));
            }
            let text = format!("{} {name}", i + 1);
            spans.push(if i == self.step {
                Span::styled(text, Style::new().fg(theme::INK).bg(theme::ACCENT).add_modifier(Modifier::BOLD))
            } else if i < self.step {
                Span::styled(text, Style::new().fg(theme::OK))
            } else {
                Span::styled(text, theme::read_only())
            });
        }
        Line::from(spans)
    }

    /// The line under the body: text entry, the filter, the preview's place, or a note.
    fn footer(&self) -> Line<'static> {
        let mut spans = vec![Span::raw(" ")];
        match (&self.entry, self.step) {
            (Entry::Filter, _) => spans.push(Span::raw(format!("/ {}▏", self.filter))),
            (Entry::Path(buf), _) => spans.push(Span::raw(format!("path {buf}▏"))),
            (_, 0) if !self.filter.is_empty() => spans.push(Span::styled(format!("filter: {} ({} shown)", self.filter, self.visible().len()), theme::hint())),
            (_, 1) if self.lines.len() > self.view => {
                let to = (self.scroll + self.view).min(self.lines.len());
                spans.push(Span::styled(format!("lines {}-{to} of {}", self.scroll + 1, self.lines.len()), theme::hint()));
            }
            _ => {}
        }
        if !self.note.is_empty() {
            spans.push(Span::styled(format!("  {}", self.note), Style::new().fg(theme::WARN).add_modifier(Modifier::BOLD)));
        }
        Line::from(spans)
    }

    fn draw_picker(&mut self, f: &mut Frame, body: Rect) {
        let rows = self.rows();
        self.cursor = self.cursor.min(rows.saturating_sub(1));
        let vis = self.visible();
        let room = (body.width as usize).saturating_sub(1);
        let items: Vec<ListItem> = (0..rows)
            .map(|row| {
                let (mark, label, detail, badge) = match vis.get(row) {
                    Some(&i) => {
                        let c = &self.cands[i];
                        let on = self.picked[i];
                        let mark = match (c.pickable, self.multi, on) {
                            (false, _, _) => "[-]",
                            (_, true, true) => "[x]",
                            (_, true, false) => "[ ]",
                            (_, false, true) => "(•)",
                            (_, false, false) => "( )",
                        };
                        (mark, c.label.as_str(), c.detail.as_str(), Some((c.badge.as_str(), c.tone)))
                    }
                    None => ("[+]", "Other…", "type a path", None),
                };
                let on = vis.get(row).is_some_and(|&i| self.picked[i]);
                let pickable = vis.get(row).is_none_or(|&i| self.cands[i].pickable);
                let badge_text = badge.map(|(b, _)| format!(" {b} ")).unwrap_or_default();
                let left = elide(&format!(" {mark} {label}"), room.saturating_sub(badge_text.chars().count() + 1));
                let pad = room.saturating_sub(left.chars().count() + badge_text.chars().count()).max(1);
                let text_style = if pickable { Style::new() } else { theme::read_only() };
                let mut first = vec![
                    Span::styled(left.chars().take(5).collect::<String>(), if on { Style::new().fg(theme::ACCENT) } else { text_style }),
                    Span::styled(left.chars().skip(5).collect::<String>(), text_style),
                    Span::raw(" ".repeat(pad)),
                ];
                if let Some((_, tone)) = badge {
                    first.push(Span::styled(badge_text, badge_style(tone)));
                }
                let second = Line::styled(format!("     {}", elide(detail, room.saturating_sub(6))), theme::hint());
                ListItem::new(Text::from(vec![Line::from(first), second])).style(if row == self.cursor { theme::selected().patch(theme::selected_text()) } else { Style::new() })
            })
            .collect();
        // The selection is the item's own style, not the list's highlight, which
        // would repaint the badge's background.
        let list = List::new(items)
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, Style::new().fg(theme::ACCENT)))
            .highlight_spacing(HighlightSpacing::Always);
        self.list.select(if rows == 0 { None } else { Some(self.cursor) });
        self.hits.list = body;
        f.render_stateful_widget(list, body, &mut self.list);
    }

    fn draw_preview(&mut self, f: &mut Frame, body: Rect) {
        self.view = body.height as usize;
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(self.view));
        let room = (body.width as usize).saturating_sub(1);
        let lines: Vec<Line> = self.lines.iter().skip(self.scroll).take(self.view).map(|l| verb_line(l, room)).collect();
        f.render_widget(Paragraph::new(lines), body);
    }

    fn draw_confirm(&mut self, f: &mut Frame, body: Rect) {
        let on: Vec<bool> = self.checks.iter().map(|c| c.on).collect();
        let outs = (self.finish)(&self.picks(), &on);
        let room = (body.width as usize).saturating_sub(1);
        let mut lines = vec![Line::styled(" queues, in order", Style::new().add_modifier(Modifier::BOLD))];
        if outs.is_empty() {
            lines.push(Line::styled("   nothing: this choice has no command to queue", theme::hint()));
        }
        for (i, o) in outs.iter().enumerate() {
            let mut row = vec![Span::raw(format!("  {}. ", i + 1)), Span::styled(format!("$ {}", elide(&o.command, room.saturating_sub(24))), theme::queued())];
            if o.confirm {
                row.push(Span::styled("  ▲ asks first", Style::new().fg(theme::WARN).add_modifier(Modifier::BOLD)));
            }
            lines.push(Line::from(row));
        }
        if !self.checks.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled(" options", Style::new().add_modifier(Modifier::BOLD)));
            for (i, c) in self.checks.iter().enumerate() {
                let y = body.y + lines.len() as u16;
                if y < body.y + body.height {
                    self.hits.checks.push((Rect { y, height: 1, ..body }, i));
                }
                let style = if i == self.check_at { Style::new().fg(theme::INK).bg(theme::ACCENT).add_modifier(Modifier::BOLD) } else { Style::new() };
                lines.push(Line::styled(format!("  {} {}", if c.on { "[x]" } else { "[ ]" }, c.label), style));
            }
        }
        f.render_widget(Paragraph::new(lines), body);
    }
}

fn badge_style(t: Tone) -> Style {
    match t {
        Tone::Ok => Style::new().fg(theme::INK).bg(theme::OK),
        Tone::Warn => Style::new().fg(theme::INK).bg(theme::WARN),
        Tone::Err => Style::new().fg(theme::INK).bg(theme::ERR),
        Tone::Accent => Style::new().fg(theme::INK).bg(theme::ACCENT),
        Tone::Muted => Style::new().fg(theme::TEXT).bg(theme::RULE),
    }
}

/// One preview line: a marker and a colour per verb, cut to `room` columns.
/// The marker keeps a verb readable without its colour.
fn verb_line(l: &PLine, room: usize) -> Line<'static> {
    let (mark, style) = match l.verb {
        Verb::Heading => ("", Style::new().add_modifier(Modifier::BOLD)),
        Verb::Plain => ("  ", Style::new()),
        Verb::Kept => (" = ", theme::read_only()),
        Verb::Added => (" + ", Style::new().fg(theme::OK)),
        Verb::Replaced => (" ~ ", Style::new().fg(theme::WARN)),
        Verb::Removed => (" - ", Style::new().fg(theme::HOT)),
        Verb::Refused => (" ✗ ", Style::new().fg(theme::ERR).add_modifier(Modifier::BOLD)),
    };
    let text = format!("{mark}{}", l.text);
    let cut = if text.chars().count() > room { format!("{}…", text.chars().take(room.saturating_sub(1)).collect::<String>()) } else { text };
    Line::styled(cut, style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn two() -> Vec<Candidate> {
        vec![
            Candidate::new("/a", "/a", "first", "available", Tone::Accent),
            Candidate::new("/b", "/b", "second", "disabled", Tone::Warn),
            Candidate::new("/c", "/c", "third", "active", Tone::Ok).unpickable(),
        ]
    }

    fn flow(multi: bool) -> Flow {
        Flow::new(
            "t",
            multi,
            two(),
            |p| p.iter().map(|c| PLine::new(Verb::Added, c.id.clone())).collect(),
            |p, on| p.iter().map(|c| Out::new("do", format!("do {}{}", c.id, if on.first() == Some(&true) { " +" } else { "" }), false)).collect(),
        )
        .other(|t| Candidate::new(t, t, "typed", "available", Tone::Accent))
        .check("also", false)
    }

    fn keys(fl: &mut Flow, ks: &[KeyCode]) -> Vec<FlowEvent> {
        ks.iter().map(|k| fl.key(press(*k))).collect()
    }

    fn typed(fl: &mut Flow, s: &str) {
        for c in s.chars() {
            fl.key(press(KeyCode::Char(c)));
        }
    }

    #[test]
    fn next_needs_a_pick_then_steps_forward_and_back() {
        let mut f = flow(true);
        assert_eq!(f.key(press(KeyCode::Right)), FlowEvent::Stay);
        assert_eq!(f.step(), 0, "nothing picked: no step");
        keys(&mut f, &[KeyCode::Char(' '), KeyCode::Right]);
        assert_eq!((f.step(), f.picked_ids()), (1, vec!["/a".to_string()]));
        keys(&mut f, &[KeyCode::Right]);
        assert_eq!(f.step(), 2);
        keys(&mut f, &[KeyCode::Esc, KeyCode::Char('b')]);
        assert_eq!(f.step(), 0, "Esc and b both go back");
        assert_eq!(f.picked_ids(), ["/a"], "back keeps the picks");
    }

    #[test]
    fn esc_on_the_first_step_cancels_and_q_cancels_anywhere() {
        assert_eq!(flow(true).key(press(KeyCode::Esc)), FlowEvent::Cancel);
        let mut f = flow(true);
        keys(&mut f, &[KeyCode::Enter, KeyCode::Enter]);
        assert_eq!(f.step(), 2);
        assert_eq!(f.key(press(KeyCode::Char('q'))), FlowEvent::Cancel);
        let mut f = flow(true);
        keys(&mut f, &[KeyCode::Char('/')]);
        typed(&mut f, "q");
        assert_eq!(f.filter, "q", "while typing, q is text");
    }

    #[test]
    fn enter_picks_the_row_when_nothing_is_picked_and_finish_returns_the_outs() {
        let mut f = flow(true);
        f.key = "install".into();
        keys(&mut f, &[KeyCode::Down, KeyCode::Enter, KeyCode::Enter]);
        assert_eq!(f.step(), 2);
        keys(&mut f, &[KeyCode::Char(' ')]);
        let ev = f.key(press(KeyCode::Enter));
        assert_eq!(ev, FlowEvent::Finish { key: "install".into(), outs: vec![Out::new("do", "do /b +", false)] });
    }

    #[test]
    fn multi_marks_many_and_single_marks_one_and_a_listed_active_row_cannot_be_picked() {
        let mut m = flow(true);
        keys(&mut m, &[KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' ')]);
        assert_eq!(m.picked_ids(), ["/a", "/b"]);
        assert!(m.note.contains("active"), "{}", m.note);
        let mut s = flow(false);
        keys(&mut s, &[KeyCode::Char(' '), KeyCode::Down, KeyCode::Char(' ')]);
        assert_eq!(s.picked_ids(), ["/b"]);
    }

    #[test]
    fn other_takes_a_typed_path_and_picks_it() {
        let mut f = flow(true);
        keys(&mut f, &[KeyCode::End, KeyCode::Enter]);
        assert!(matches!(f.entry, Entry::Path(_)));
        typed(&mut f, "/x y");
        keys(&mut f, &[KeyCode::Backspace]);
        typed(&mut f, "z");
        keys(&mut f, &[KeyCode::Enter]);
        assert_eq!(f.picked_ids(), ["/x z"]);
        assert!(matches!(f.entry, Entry::Off));
        // The same path again picks the existing row rather than adding one.
        keys(&mut f, &[KeyCode::End, KeyCode::Char(' ')]);
        typed(&mut f, "/x z");
        keys(&mut f, &[KeyCode::Enter]);
        assert_eq!(f.cands.len(), 4);
        keys(&mut f, &[KeyCode::End, KeyCode::Enter, KeyCode::Esc]);
        assert_eq!(f.picked_ids(), ["/x z"], "Esc cancels the path entry and keeps the picks");
    }

    #[test]
    fn a_filter_narrows_the_rows_and_esc_clears_it_before_cancelling() {
        let mut f = flow(true);
        keys(&mut f, &[KeyCode::Char('/')]);
        typed(&mut f, "second");
        keys(&mut f, &[KeyCode::Enter]);
        assert_eq!(f.visible(), [1]);
        keys(&mut f, &[KeyCode::Char(' ')]);
        assert_eq!(f.picked_ids(), ["/b"]);
        assert_eq!(f.key(press(KeyCode::Esc)), FlowEvent::Stay);
        assert!(f.filter.is_empty() && f.visible().len() == 3);
        assert_eq!(f.key(press(KeyCode::Esc)), FlowEvent::Cancel);
    }
}
