//! Drawing for the TUI: the tab bar, the tree, the detail and pending panes, the action
//! menu and the status line. Review mode draws its own tree and detail in
//! `review`, and the boxed modals draw in `modals`. Each frame also records where the
//! clickable parts landed, in `App::hits`.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::apply::Outcome;
use super::theme::{self, Ground, Seg, Shape};
use super::{App, Btn, Mode};
use crate::tree::{self, Arg, Kind, Row};
use crate::wrap::str_width;

/// A bordered pane in the theme, drawn in `area`: rule-coloured border in
/// the theme's glyphs, plain by default, and an accent title after the
/// theme's ornament where both fit inside the border.
pub(super) fn pane(title: impl Into<Line<'static>>, area: Rect) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_set(theme::border_set(border::PLAIN))
        .border_style(theme::rule())
        .title(theme::ornamented(title.into(), area.width.saturating_sub(2)))
        .title_style(theme::title())
}

pub(super) fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| s.width() as u16).sum()
}

/// Buttons on one line, each on its ground and recorded as a click target.
/// `focus` marks the one Enter would press.
///
/// [`Ground::Keep`] is green on a row that holds a button on [`Ground::Err`],
/// as the bar's keep answer is beside its drop answer; elsewhere it takes
/// the dim accent.
pub(super) fn button_row(sh: Shape, at: Rect, focus: Option<Btn>, buttons: &[(Btn, String, Ground)], hits: &mut Vec<(Rect, Btn)>) -> Line<'static> {
    let destructive = buttons.iter().any(|(_, _, g)| *g == Ground::Err);
    let mut spans = vec![Span::raw(" ")];
    for (b, label, ground) in buttons {
        let on = focus == Some(*b);
        let seg = Seg::on(format!(" {} {label} ", if on { "›" } else { " " }), ground.on_row(destructive));
        let target = sh.lozenge(&[if on { seg.bold() } else { seg }]);
        let x = at.x + width(&spans);
        hits.push((Rect { x, y: at.y, width: width(&target), height: 1 }.intersection(at), *b));
        spans.extend(target);
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

/// `text` cut to `room` columns, ending in `…` when it was cut.
pub(super) fn clip(text: &str, room: usize) -> String {
    if str_width(text) <= room {
        return text.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in text.chars() {
        let cw = str_width(c.encode_utf8(&mut [0; 4]));
        if w + cw + 1 > room {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// `text` cut in the middle with `…` to fit `room` columns, keeping the end,
/// where a step's file name and key count sit.
pub(super) fn elide(text: &str, room: usize) -> String {
    let n = text.chars().count();
    if n <= room || room < 8 {
        return text.to_string();
    }
    let head = room / 4;
    let tail = room - head - 1;
    format!("{}…{}", text.chars().take(head).collect::<String>(), text.chars().skip(n - tail).collect::<String>())
}

/// A modal of `w` by `h` centred in `area`. One whose top edge lands on the
/// area's first row takes its whole width too: narrower, it would leave the
/// start of the tree's title showing beside its top-left corner.
pub(super) fn modal_rect(area: Rect, w: u16, h: u16) -> Rect {
    let h = h.min(area.height);
    let w = if (area.height - h) / 2 == 0 { area.width } else { w.min(area.width) };
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// The fewest columns a status message is shown in: in fewer, the cut
/// leaves too little of it to read, so it goes.
const MSG_MIN: usize = 8;

/// The most dots the masked entry draws; past it a count follows, so the
/// hint beside the entry stays on screen for a long key.
const MASK_DOTS: usize = 24;

/// One dot per character typed, so each keystroke shows; the characters
/// never reach the screen.
pub(crate) fn mask(n: usize) -> String {
    if n <= MASK_DOTS {
        "•".repeat(n)
    } else {
        format!("{}… {n}", "•".repeat(MASK_DOTS))
    }
}

impl App {
    /// One frame in the theme shown: the active one, or on the theme tab the
    /// one previewed or edited. With background=fill, every cell left on the
    /// terminal's default gets the theme's.
    pub fn draw(&mut self, f: &mut Frame) {
        theme::set(self.pane_palette().unwrap_or_else(|| self.themes.palette(self.shown_theme())));
        self.draw_frame(f);
        theme::fill(f.buffer_mut());
    }

    fn draw_frame(&mut self, f: &mut Frame) {
        let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
        let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
        self.hits.dialog = None;
        self.hits.buttons.clear();
        self.hits.cta = Rect::default();
        self.hits.discard_tabs.clear();
        self.hits.sliders.clear();
        self.hits.hex = Rect::default();
        self.hits.pane = Rect::default();
        if self.pane.is_some() {
            return self.draw_pane_frame(f);
        }
        // Review keeps the browser's layout; the tree and the detail show what is pending.
        // A response opened over review keeps review under it.
        let review = match &self.mode {
            Mode::Review { tab, .. } => Some(*tab),
            Mode::Response(s) => s.back,
            _ => None,
        };
        if let Some(tab) = review {
            self.draw_tabs(f, bar, tab, true);
            self.draw_review_tree(f, left, tab);
            self.draw_review_detail(f, right, tab);
            self.draw_modal(f, main, false);
            return self.draw_status(f, status);
        }
        if self.on_theme_tab() {
            self.draw_tabs(f, bar, self.tab, false);
            self.draw_theme_tab(f, left, right);
            self.draw_modal(f, main, false);
            return self.draw_status(f, status);
        }
        let rows = self.rows();
        if !rows.is_empty() {
            self.cursor = self.cursor.min(rows.len() - 1);
        }
        // While a filter shows every tab's matches, the cursor's tab is the active one.
        let active = if self.filter.is_empty() { self.tab } else { rows.get(self.cursor).map_or(self.tab, |r| r.path[0]) };
        self.draw_tabs(f, bar, active, false);
        self.draw_tree(f, left, &rows);
        if self.show_changes {
            self.draw_changes(f, right);
        } else if let Some(r) = rows.get(self.cursor) {
            // A row with an `about` splits the pane: its controls above,
            // what it stands for below.
            let n = tree::get(&self.roots, &r.path);
            if n.about.is_empty() {
                self.draw_detail(f, right, &r.path);
            } else {
                let [top, bottom] = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);
                self.draw_detail(f, top, &r.path);
                let title = if n.about_title.is_empty() { "about".to_string() } else { n.about_title.clone() };
                let lines: Vec<Line> = n.about.lines().map(|l| Line::raw(l.to_string())).collect();
                f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane(title, bottom)), bottom);
            }
        }
        if let Mode::Flow(flow) = &mut self.mode {
            flow.draw(f, main, self.shape);
        }
        self.draw_modal(f, main, false);
        self.draw_status(f, status);
    }

    /// One lozenge per root: its number and name, then its pending count and
    /// a discard mark as a badge segment when non-zero. The shown tab takes the
    /// accent; in review, a tab with nothing pending is dimmed.
    ///
    /// The bar keeps to its width. When the tabs overflow it, the gaps tighten
    /// first, then the names are cut shorter, down to the number alone; the
    /// numbers and badges always stay. Tabs that still do not fit are left
    /// off behind a `›` marker, so none goes missing unannounced; the tab
    /// shown is never one of them.
    fn draw_tabs(&mut self, f: &mut Frame, area: Rect, active: usize, review: bool) {
        /// A tab before layout: its index, name and badge, and whether the
        /// badge is a discard target.
        struct Tab {
            i: usize,
            name: String,
            badge: Option<String>,
            faded: bool,
            discard: bool,
        }
        let mut tabs: Vec<Tab> = self
            .roots
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let pending = tree::pending(r, &self.queue);
                let badge = (pending > 0).then(|| format!(" ●{pending} ↺ "));
                Tab { i, name: r.name.clone(), badge, faded: review && pending == 0 && i != active, discard: true }
            })
            .collect();
        // The theme tab: no review, so dimmed there; a badge for unsaved edits, with no discard mark.
        let badge = self.theme_dirty().then(|| " ●1 ".to_string());
        tabs.push(Tab { i: self.theme_tab(), name: "theme".into(), badge, faded: review, discard: false });

        let sh = self.shape;
        // A tab's lozenge with its name cut to `cap` columns; a cap of 0 leaves the number alone.
        let lozenge = |t: &Tab, cap: usize| {
            let label = if cap == 0 { format!(" {} ", t.i + 1) } else { format!(" {} {} ", t.i + 1, crate::wrap::truncate(&t.name, cap)) };
            let mut segs = vec![if t.faded { Seg::faded(label) } else { crate::strip::tab_seg(label, t.i == active) }];
            if let Some(b) = &t.badge {
                segs.push(Seg::on(b.clone(), Ground::Warn).bold());
            }
            sh.lozenge(&segs)
        };
        let span = |lozs: &[(&Tab, Vec<Span>)], gap: u16| lozs.iter().map(|(_, l)| width(l)).sum::<u16>() + gap * lozs.len().saturating_sub(1) as u16;
        let longest = tabs.iter().map(|t| crate::wrap::str_width(&t.name)).max().unwrap_or(0);
        // Whole names at the usual gap, then at one column, then names cut
        // shorter. A cap of 1 would leave a bare `…`, so below 2 the name goes.
        let mut plan = None;
        'fit: for cap in (0..=longest).rev().filter(|c| *c != 1) {
            let lozs: Vec<_> = tabs.iter().map(|t| (t, lozenge(t, cap))).collect();
            let gaps: &[u16] = if cap == longest { &[2, 1] } else { &[1] };
            for &gap in gaps {
                if span(&lozs, gap) <= area.width {
                    plan = Some((lozs, gap, false));
                    break 'fit;
                }
            }
        }
        // Even the numbers alone overflow: as many as fit, then the marker.
        // The last tab other than the one shown goes first.
        let (lozs, gap, more) = plan.unwrap_or_else(|| {
            let mut lozs: Vec<_> = tabs.iter().map(|t| (t, lozenge(t, 0))).collect();
            while !lozs.is_empty() && span(&lozs, 1) + 2 > area.width {
                let last = lozs.iter().rposition(|(t, _)| t.i != active).unwrap_or(lozs.len() - 1);
                lozs.remove(last);
            }
            (lozs, 1, true)
        });

        self.hits.tabs.clear();
        let mut spans = Vec::new();
        for (k, (t, tab)) in lozs.into_iter().enumerate() {
            if k > 0 {
                spans.push(Span::raw(" ".repeat(gap as usize)));
            }
            let x = area.x + width(&spans);
            if t.discard && t.badge.is_some() {
                // The badge is the discard target: everything in the lozenge from its first glyph on.
                let at = tab.iter().position(|sp| sp.content.starts_with(" ●")).unwrap_or(0);
                let bx = x + width(&tab[..at]);
                self.hits.discard_tabs.push((Rect { x: bx, y: area.y, width: tab[at].width() as u16, height: 1 }.intersection(area), t.i));
            }
            self.hits.tabs.push((Rect { x, y: area.y, width: width(&tab), height: 1 }.intersection(area), t.i));
            spans.extend(tab);
        }
        if more {
            spans.push(Span::styled(" ›", theme::hint()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_tree(&mut self, f: &mut Frame, area: Rect, rows: &[Row]) {
        let inner = area.width.saturating_sub(4) as usize;
        let value_col = inner * 3 / 5;
        let items: Vec<ListItem> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let n = tree::get(&self.roots, &r.path);
                let marker = match (n.children.is_empty(), n.open || !self.filter.is_empty()) {
                    (true, _) => "  ",
                    (false, true) => "▾ ",
                    (false, false) => "▸ ",
                };
                let left = format!("{}{}{}", "  ".repeat(r.depth), marker, n.name);
                let mut spans = vec![Span::styled(
                    left.clone(),
                    if n.setting.is_none() { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() },
                )];
                // A header row: the name column's label after the name, the
                // value column's in its column.
                let (name_label, value_label) = match &n.columns {
                    Some((a, b)) if a.is_empty() => (String::new(), Some(b.clone())),
                    Some((a, b)) => (format!(" · {a}"), Some(b.clone())),
                    None => (String::new(), None),
                };
                spans.push(Span::styled(name_label.clone(), theme::muted()));
                let pad = value_col.saturating_sub(left.chars().count() + name_label.chars().count()).max(1);
                spans.push(Span::raw(" ".repeat(pad)));
                if let Some(v) = value_label {
                    spans.push(Span::styled(v, theme::muted()));
                }
                if let Some(s) = &n.setting {
                    let style = if s.changed() {
                        theme::changed()
                    } else if matches!(s.kind, Kind::Secret) {
                        theme::secret(s.value == "present")
                    } else if matches!(s.kind, Kind::ReadOnly) {
                        theme::read_only()
                    } else if s.default.as_deref().is_some_and(|d| d != s.value) {
                        theme::non_default()
                    } else {
                        Style::new()
                    };
                    spans.push(Span::styled(s.value.clone(), style));
                }
                if !n.actions.is_empty() {
                    spans.push(Span::styled(" [a]", theme::hint()));
                }
                if n.finding.is_some() {
                    spans.push(Span::styled(" !", theme::finding()));
                }
                let c = n.changes() + tree::queued_under(&self.roots, &r.path, &self.queue);
                if c > 0 && !n.children.is_empty() {
                    spans.push(Span::styled(format!("  ●{c}"), theme::warn()));
                }
                ListItem::new(Line::from(spans)).style(if i == self.cursor { theme::selected_text() } else { Style::new() })
            })
            .collect();
        let own = self.roots.get(self.tab).and_then(|t| self.adapter.title(&t.name)).unwrap_or_else(|| self.title.clone());
        let title = if self.filter.is_empty() { own } else { format!("{own} — /{}", self.filter) };
        // The border would clip a long title mid-word: cut it between the corners with `…`.
        let block = pane(crate::wrap::truncate(&title, area.width.saturating_sub(2) as usize), area);
        self.hits.list = block.inner(area);
        let list = List::new(items)
            .block(block)
            .highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::selected_mark(), theme::accent()))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
        self.list.select(if rows.is_empty() { None } else { Some(self.cursor) });
        f.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_detail(&self, f: &mut Frame, area: Rect, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        let dim = theme::muted();
        let title = n.heading.clone().unwrap_or_else(|| tree::label(&self.roots, path));
        let mut lines = vec![Line::styled(title, Style::new().add_modifier(Modifier::BOLD)), Line::raw("")];
        // A setting beside its `about` shows its controls alone; a group
        // keeps its doc above the summary.
        if !n.doc.is_empty() && (n.about.is_empty() || n.setting.is_none()) {
            lines.extend(n.doc.lines().map(|l| Line::raw(l.to_string())));
            lines.push(Line::raw(""));
        }
        if let Some(why) = &n.finding {
            for (i, l) in why.lines().enumerate() {
                let label = if i == 0 { "finding  " } else { "         " };
                lines.push(Line::from(vec![Span::styled(label, dim), Span::styled(l.to_string(), theme::finding())]));
            }
            lines.push(Line::raw(""));
        }
        if let Some(s) = &n.setting {
            let field = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<9}"), dim), Span::raw(v)]);
            let value = match s.label_of(&s.value) {
                Some(name) => format!("{} · {name}", s.value),
                None => s.value.clone(),
            };
            lines.push(field("value", value));
            if s.changed() {
                lines.push(field("was", s.loaded.clone()));
            }
            lines.push(field("type", kind_label(&s.kind)));
            if let Some(d) = &s.default {
                lines.push(field("default", d.clone()));
            }
            lines.push(field("from", s.source.clone()));
            if let Some(st) = &s.store {
                lines.push(field("writes", format!("{}  [{}]", st.shown, st.key)));
            }
            if let Some(why) = &s.locked {
                lines.push(Line::from(vec![Span::styled(format!("{:<9}", "locked"), dim), Span::styled(why.clone(), theme::finding())]));
            }
        } else {
            let c = n.changes();
            let n_ = n.children.len();
            lines.push(Line::styled(format!("{n_} {}, {c} changed", if n_ == 1 { "entry" } else { "entries" }), dim));
        }
        if !n.actions.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("actions", dim));
            for (a, k) in n.actions.iter().zip(tree::action_keys(&n.actions)) {
                let tag = match (&a.arg, a.confirm) {
                    (Arg::View(_), _) => " (view)",
                    (_, true) => " (asks first)",
                    _ => "",
                };
                lines.push(Line::from(vec![Span::styled(format!("  {} ", k.unwrap_or(' ')), theme::accent()), Span::raw(format!("{}{tag}", a.label))]));
                lines.push(Line::styled(format!("    {}", a.render("<arg>")), dim));
            }
        }
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane("detail", area));
        f.render_widget(p, area);
    }

    /// The pending pane: value changes by file, then queued actions in order.
    fn draw_changes(&self, f: &mut Frame, area: Rect) {
        let ch = tree::changes(&self.roots);
        let mut lines = Vec::new();
        if ch.is_empty() && self.queue.is_empty() {
            lines.push(Line::raw("nothing pending"));
        }
        let mut files: Vec<String> = ch.iter().map(|c| c.1.as_ref().map_or("(no store)".into(), |s| s.shown.clone())).collect();
        files.sort();
        files.dedup();
        for file in files {
            lines.push(Line::styled(file.clone(), Style::new().add_modifier(Modifier::BOLD)));
            for (k, st, from, to) in &ch {
                if st.as_ref().map_or("(no store)".to_string(), |s| s.shown.clone()) == file {
                    let key = st.as_ref().map_or(k.clone(), |s| s.key.clone());
                    lines.push(Line::from(vec![
                        Span::raw(format!("  {key}: ")),
                        Span::styled(from.clone(), theme::err()),
                        Span::raw(" → "),
                        Span::styled(to.clone(), theme::ok()),
                    ]));
                }
            }
        }
        if !self.queue.is_empty() {
            lines.push(Line::styled("queued actions (x unqueues the last)", Style::new().add_modifier(Modifier::BOLD)));
            for (i, q) in self.queue.items().iter().enumerate() {
                lines.push(Line::raw(format!("  {}. {}  [{}]", i + 1, q.label, q.key)));
                lines.push(Line::styled(format!("     $ {}", q.command), theme::queued()));
            }
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("w reviews and applies one tab", theme::hint()));
        let title = format!("pending ({} changes, {} actions)", ch.len(), self.queue.len());
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane(title, area));
        f.render_widget(p, area);
    }

    /// The bottom line, built like the status line's first line: a mode
    /// lozenge, then flat parts between thin rules. A question on the bar
    /// is drawn in its place ([`super::dialog::Placement::Bar`]).
    pub(super) fn draw_status(&mut self, f: &mut Frame, area: Rect) {
        if self.draw_modal(f, area, true) {
            return;
        }
        let sh = self.shape;
        let mode = |label: &str, g: Ground| sh.lozenge(&[Seg::on(format!(" {label} "), g).bold()]);
        let input = |text: String| Span::raw(format!(" {text}▏ "));
        let hint = |text: &str| Span::styled(text.to_string(), theme::hint());
        let msg = self.msg_span();
        let mut spans: Vec<Span> = Vec::new();
        let mut cta = None;
        let mut modal = self.modal().and_then(|m| m.status(self));
        match &self.mode {
            _ if modal.is_some() => spans.extend(modal.take().into_iter().flatten()),
            Mode::Edit(buf) => {
                spans.extend(mode("edit", Ground::Accent));
                spans.extend([input(buf.clone()), hint(" Enter set · Esc cancel"), msg]);
            }
            Mode::Filter => {
                spans.extend(mode("/", Ground::Accent));
                spans.extend([input(self.filter.clone()), hint(" Enter keep · Esc clear")]);
            }
            Mode::Arg { path, action, buf } => {
                let prompt = match &tree::get(&self.roots, path).actions[*action].arg {
                    Arg::Text(p) => p.clone(),
                    _ => "argument".into(),
                };
                spans.extend(mode(&prompt, Ground::Accent));
                spans.extend([input(buf.clone()), hint(" Enter queue · Esc cancel")]);
            }
            Mode::Secret { buf, .. } => {
                spans.extend(mode("secret", Ground::Hot));
                spans.extend([input(mask(buf.len())), hint(" Enter queue (goes to stdin, never shown) · Esc cancel")]);
            }
            Mode::Review { run: Some(run), .. } => {
                let (label, text) = match run.outcome {
                    Outcome::Stopped(_) => ("stopped", "  the failed step and the rest stay pending"),
                    _ => ("applying", "  writes first, then commands; a failure stops the run"),
                };
                spans.extend(mode(label, Ground::Hot));
                spans.extend([hint(text), msg]);
            }
            Mode::Review { tab, .. } => {
                spans.extend(mode("review", Ground::Hot));
                spans.push(Span::raw(" "));
                // The tab's name goes when the message would not fit beside it.
                let start = spans.len();
                for named in [true, false] {
                    spans.truncate(start);
                    let name = if named { format!(" {}", self.roots[*tab].name) } else { String::new() };
                    let targets = [(Btn::Apply, format!("a apply{name}")), (Btn::Discard, format!("X discard{name}")), (Btn::Back, "Esc back".to_string())];
                    let mut at = Vec::new();
                    for (i, (b, text)) in targets.into_iter().enumerate() {
                        if i > 0 {
                            spans.push(theme::sep());
                        }
                        at.push((Rect { x: area.x + width(&spans), y: area.y, width: text.chars().count() as u16, height: 1 }.intersection(area), b));
                        spans.push(Span::raw(text));
                    }
                    if width(&spans) + msg.width() as u16 <= area.width || !named {
                        self.hits.buttons.extend(at);
                        break;
                    }
                }
                spans.push(msg);
            }
            Mode::Name { op, buf } => {
                spans.extend(mode(&op.prompt(), Ground::Accent));
                spans.extend([input(buf.clone()), hint(" Enter · Esc cancel"), msg]);
            }
            Mode::Entry(_) => {
                spans.extend(mode("entry", Ground::Accent));
                spans.push(hint("  type a line · Enter sets · Esc cancels"));
            }
            Mode::Browse | Mode::Help { .. } if self.pane.is_some() => spans.extend(self.pane_status(area)),
            Mode::Browse | Mode::Help { .. } if self.on_theme_tab() => {
                spans.extend(self.theme_status(sh));
                spans.push(msg);
            }
            Mode::Flow(flow) => {
                spans.extend(mode("flow", Ground::Accent));
                spans.push(hint(flow.hint()));
            }
            _ => {
                spans.extend(mode(if self.filter.is_empty() { "browse" } else { "filter" }, Ground::Accent));
                let changed = tree::changes(&self.roots).len();
                spans.push(Span::raw(" "));
                spans.push(if changed > 0 { Span::styled(format!("●{changed} changed"), theme::changed()) } else { hint("0 changed") });
                spans.push(theme::sep());
                spans.push(if self.queue.is_empty() { hint("0 queued") } else { Span::styled(format!("{} queued", self.queue.len()), theme::queued()) });
                spans.push(theme::sep());
                let pending = self.pending_in(self.tab);
                if pending > 0 {
                    // The bar is short: the call to action takes the mouse hint's room.
                    let text = format!(" ● {pending} unsaved in {} · w review & apply ", self.roots[self.tab].name);
                    cta = Some(sh.lozenge(&[Seg::on(text, Ground::Hot).bold()]));
                } else {
                    spans.push(hint(if self.mouse { "mouse on (m)" } else { "mouse off (m)" }));
                }
                // A row's actions take the place of the opening hint when
                // both do not fit; a message said since comes first. It is
                // cut in the middle to the room left of the call to action,
                // keeping its end, and goes when too little is left to read.
                // The opening hint is cut at its tail instead: its keys
                // lead, and `Tab 1-9` must not be the part cut away.
                let here = self.footer_actions();
                let room = area.width.saturating_sub(cta.as_ref().map_or(0, |c| width(c)));
                let fits = |here: &[Span]| width(&spans) + msg.width() as u16 + width(&[theme::sep()]) + width(here) <= room;
                if self.msg != crate::app::HINT || here.as_deref().is_none_or(fits) {
                    // Two columns lead the message; it needs eight past them.
                    let left = room.saturating_sub(width(&spans)) as usize;
                    if left >= 2 + MSG_MIN {
                        let cut = if self.msg == crate::app::HINT { crate::wrap::truncate(&self.msg, left - 2) } else { elide(&self.msg, left - 2) };
                        spans.push(Span::styled(format!("  {cut}"), msg.style));
                    }
                }
                if let Some(here) = here {
                    spans.push(theme::sep());
                    spans.extend(here);
                }
            }
        }
        // The call to action sits at the right end; the rest yields to it.
        let left = match &cta {
            Some(c) => {
                let w = width(c).min(area.width);
                let at = Rect { x: area.x + area.width - w, width: w, ..area };
                self.hits.cta = at;
                f.render_widget(Paragraph::new(Line::from(c.clone())), at);
                Rect { width: area.width - w, ..area }
            }
            None => area,
        };
        f.render_widget(Paragraph::new(Line::from(spans)), left);
    }

    /// The message on the bottom bar, after two columns; a rejection is
    /// bold on the error colour.
    pub(super) fn msg_span(&self) -> Span<'static> {
        let style = if self.msg.starts_with("rejected") { theme::err().add_modifier(Modifier::BOLD) } else { Style::new() };
        Span::styled(format!("  {}", self.msg), style)
    }

    /// The selected row's actions as `key label` pairs, from their
    /// declaration: the row's own, else its tab's.
    pub(super) fn footer_actions(&self) -> Option<Vec<Span<'static>>> {
        let rows = self.rows();
        let path = self.actions_at(&rows.get(self.cursor)?.path);
        let actions = &tree::get(&self.roots, &path).actions;
        if actions.is_empty() {
            return None;
        }
        // The one footer look, which a pane's bindings share.
        let keys: Vec<String> = tree::action_keys(actions).iter().map(|k| k.map_or(String::new(), String::from)).collect();
        Some(super::pane::footer_spans(keys.iter().map(String::as_str).zip(actions.iter().map(|a| a.label.as_str()))))
    }

}

fn kind_label(k: &Kind) -> String {
    match k {
        Kind::Bool => "bool (Enter toggles)".into(),
        Kind::Float { min, max } => format!("number {min}..={max} (Enter edits)"),
        Kind::Int { min, max } => format!("integer {min}..={max} (Enter edits)"),
        Kind::Choice { options, multi: false } => format!("one of {} (Enter picks)", options.join(" | ")),
        Kind::Choice { options, multi: true } => format!("any of {} (Enter picks)", options.join(" | ")),
        Kind::Text => "text (Enter edits)".into(),
        Kind::ReadOnly => "read-only here".into(),
        Kind::Secret => "secret (Enter enters it masked)".into(),
    }
}
