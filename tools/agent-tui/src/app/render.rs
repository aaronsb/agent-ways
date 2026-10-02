//! Drawing for the TUI: the tab bar, the tree, the detail and pending panes, the action
//! menu, the quit prompt, the status line and the key help. Review mode draws
//! its own tree and detail in `review`. Each frame also records where the
//! clickable parts landed, in `App::hits`.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::{self, Ground, Seg, Shape};
use super::apply::Outcome;
use super::{App, Btn, Mode};
use crate::tree::{self, Arg, Kind, Row};

/// A bordered pane in the theme: rule-coloured border, accent title.
pub(super) fn pane(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default().borders(Borders::ALL).border_style(theme::rule()).title(title).title_style(theme::title())
}

fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| s.width() as u16).sum()
}

/// Buttons on one line, each recorded as a click target. `focus` marks the
/// one Enter would press.
pub(super) fn button_row(sh: Shape, at: Rect, focus: Option<Btn>, buttons: &[(Btn, String)], hits: &mut Vec<(Rect, Btn)>) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (b, label) in buttons {
        let ground = match b {
            Btn::Apply | Btn::Finish | Btn::Save => Ground::Ok,
            Btn::Discard | Btn::Quit | Btn::Drop => Ground::Err,
            Btn::Back => Ground::AccentDim,
            Btn::Review | Btn::Next => Ground::Accent,
            Btn::Cancel => Ground::Rule,
        };
        let on = focus == Some(*b);
        let seg = Seg::on(format!(" {} {label} ", if on { "›" } else { " " }), ground);
        let target = sh.lozenge(&[if on { seg.bold() } else { seg }]);
        let x = at.x + width(&spans);
        hits.push((Rect { x, y: at.y, width: width(&target), height: 1 }.intersection(at), *b));
        spans.extend(target);
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

/// A confirm's two answers as lozenges, each recorded as a click target.
pub(super) fn answer_lozenges(sh: Shape, area: Rect, spans: &mut Vec<Span<'static>>, hits: &mut Vec<(Rect, bool)>, answers: [(bool, &'static str, Ground); 2]) {
    for (yes, text, g) in answers {
        let target = sh.lozenge(&[Seg::on(text, g).bold()]);
        let x = area.x + width(spans);
        hits.push((Rect { x, y: area.y, width: width(&target), height: 1 }.intersection(area), yes));
        spans.extend(target);
        spans.push(Span::raw(" "));
    }
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

/// A fixed-width mask once anything is typed, so the key's length never
/// reaches the screen.
fn mask(n: usize) -> String {
    if n == 0 { String::new() } else { "••••••••".into() }
}

impl App {
    /// One frame in the theme shown: the active one, or on the theme tab the
    /// one previewed or edited. With background=fill, every cell left on the
    /// terminal's default gets the theme's.
    pub fn draw(&mut self, f: &mut Frame) {
        theme::set(self.themes.palette(self.shown_theme()));
        self.draw_frame(f);
        theme::fill(f.buffer_mut());
    }

    fn draw_frame(&mut self, f: &mut Frame) {
        let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
        let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
        self.hits.menu = None;
        self.hits.answers.clear();
        self.hits.buttons.clear();
        self.hits.cta = Rect::default();
        self.hits.discard_tabs.clear();
        self.hits.sliders.clear();
        self.hits.hex = Rect::default();
        // Review keeps the browser's layout; the tree and the detail show what is pending.
        if let Mode::Review { tab, .. } = &self.mode {
            let tab = *tab;
            self.draw_tabs(f, bar, tab, true);
            self.draw_review_tree(f, left, tab);
            self.draw_review_detail(f, right, tab);
            return self.draw_status(f, status);
        }
        if self.on_theme_tab() {
            self.draw_tabs(f, bar, self.tab, false);
            self.draw_theme_tab(f, left, right);
            match &self.mode {
                Mode::Help { scroll } => {
                    let scroll = *scroll;
                    self.draw_help(f, main, scroll);
                }
                Mode::Guard { .. } => self.draw_guard(f, main),
                Mode::ThemeMenu { sel } => {
                    let sel = *sel;
                    let items = self.theme_acts().iter().map(|a| (a.label().to_string(), self.theme_act_tag(*a))).collect();
                    let title = format!("actions: theme {}", self.themes.under_cursor().0.name);
                    self.draw_menu_items(f, main, title, items, sel);
                }
                Mode::ThemeUnsaved => self.draw_unsaved(f, main),
                _ => {}
            }
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
                f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane(title)), bottom);
            }
        }
        if let Mode::Flow(flow) = &mut self.mode {
            flow.draw(f, main, self.shape);
        }
        match &self.mode {
            Mode::Help { scroll } => {
                let scroll = *scroll;
                self.draw_help(f, main, scroll);
            }
            Mode::Guard { .. } => self.draw_guard(f, main),
            Mode::Menu { path, sel } => {
                let (path, sel) = (path.clone(), *sel);
                self.draw_menu(f, main, &path, sel);
            }
            _ => {}
        }
        self.draw_status(f, status);
    }

    /// One lozenge per root: its number and name, then its pending count and
    /// a discard mark as a badge segment when non-zero. The shown tab takes the
    /// accent; in review, a tab with nothing pending is dimmed.
    fn draw_tabs(&mut self, f: &mut Frame, area: Rect, active: usize, review: bool) {
        let mut spans = Vec::new();
        self.hits.tabs.clear();
        for (i, r) in self.roots.iter().enumerate() {
            let label = format!(" {} {} ", i + 1, r.name);
            let pending = tree::pending(r, &self.queue);
            let mut segs = vec![if review && pending == 0 && i != active {
                Seg::faded(label)
            } else {
                crate::strip::tab_seg(label, i == active)
            }];
            if pending > 0 {
                segs.push(Seg::on(format!(" ●{pending} ↺ "), Ground::Warn).bold());
            }
            let tab = self.shape.lozenge(&segs);
            let x = area.x + width(&spans);
            if pending > 0 {
                // The badge is the discard target: everything in the lozenge from its first glyph on.
                let at = tab.iter().position(|sp| sp.content.starts_with(" ●")).unwrap_or(0);
                let bx = x + width(&tab[..at]);
                self.hits.discard_tabs.push((Rect { x: bx, y: area.y, width: tab[at].width() as u16, height: 1 }.intersection(area), i));
            }
            let rect = Rect { x, y: area.y, width: width(&tab), height: 1 }.intersection(area);
            self.hits.tabs.push((rect, i));
            spans.extend(tab);
            spans.push(Span::raw("  "));
        }
        // The theme tab: no review, so dimmed there; a badge for unsaved edits, with no discard mark.
        let i = self.theme_tab();
        let label = format!(" {} theme ", i + 1);
        let mut segs = vec![if review { Seg::faded(label) } else { crate::strip::tab_seg(label, i == active) }];
        if self.theme_dirty() {
            segs.push(Seg::on(" ●1 ", Ground::Warn).bold());
        }
        let tab = self.shape.lozenge(&segs);
        self.hits.tabs.push((Rect { x: area.x + width(&spans), y: area.y, width: width(&tab), height: 1 }.intersection(area), i));
        spans.extend(tab);
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
        let title = if self.filter.is_empty() { self.title.clone() } else { format!("{} — /{}", self.title, self.filter) };
        let block = pane(title);
        self.hits.list = block.inner(area);
        let list = List::new(items)
            .block(block)
            .highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
        self.list.select(if rows.is_empty() { None } else { Some(self.cursor) });
        f.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_detail(&self, f: &mut Frame, area: Rect, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        let dim = theme::muted();
        let title = tree::label(&self.roots, path);
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
            lines.push(field("value", s.value.clone()));
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
                let tag = if a.confirm { " (asks first)" } else { "" };
                lines.push(Line::from(vec![Span::styled(format!("  {} ", k.unwrap_or(' ')), theme::accent()), Span::raw(format!("{}{tag}", a.label))]));
                lines.push(Line::styled(format!("    {}", a.render("<arg>")), dim));
            }
        }
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane("detail"));
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
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane(title));
        f.render_widget(p, area);
    }

    /// The quit prompt over the tree: every tab with pending items and its
    /// count, then the choices.
    fn draw_guard(&mut self, f: &mut Frame, area: Rect) {
        let mut tabs: Vec<(&str, usize, String)> =
            self.roots.iter().enumerate().map(|(i, r)| (r.name.as_str(), self.pending_in(i), String::new())).filter(|t| t.1 > 0).collect();
        // Unsaved theme edits are listed last, as the theme tab is.
        if let Some(e) = self.themes.editor.as_ref().filter(|e| e.dirty()) {
            tabs.push(("theme", 1, format!("  edits to {}", e.theme.name)));
        }
        let total: usize = tabs.iter().map(|t| t.1).sum();
        let mut lines = vec![Line::raw(""), Line::from(Span::styled(format!(" {total} unsaved in {} tabs", tabs.len()), theme::changed()))];
        lines.extend(tabs.iter().map(|(name, n, note)| Line::from(vec![Span::raw(format!("   {name:<12}")), Span::styled(format!("●{n}"), theme::changed()), Span::styled(note.clone(), theme::hint())])));
        lines.push(Line::raw(""));
        let name = tabs.first().map_or("", |t| t.0).to_string();
        let r = modal_rect(area, 74, lines.len() as u16 + 3);
        let inner = r.inner(ratatui::layout::Margin::new(1, 1));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines.clone()).block(pane("quit with unsaved changes").border_style(theme::modal_border())), r);
        let row = Rect { y: inner.y + lines.len() as u16, height: 1, ..inner }.intersection(inner);
        let buttons = [(Btn::Back, "Back (Esc)".into()), (Btn::Review, format!("Review {name} (r)")), (Btn::Quit, "Quit and discard all (D)".into())];
        f.render_widget(Paragraph::new(button_row(self.shape, row, Some(Btn::Back), &buttons, &mut self.hits.buttons)), row);
    }

    fn draw_menu(&mut self, f: &mut Frame, area: Rect, path: &[usize], sel: usize) {
        let n = tree::get(&self.roots, path);
        let items = n
            .actions
            .iter()
            .zip(tree::action_keys(&n.actions))
            .map(|(a, k)| {
                let tag = match (&a.arg, a.confirm) {
                    (Arg::Secret, _) => "  masked".to_string(),
                    (Arg::Flow(_), _) => "  guided".to_string(),
                    (Arg::Text(p), true) => format!("  {p}, asks first"),
                    (Arg::Text(p), false) => format!("  {p}"),
                    (Arg::None, true) => "  asks first".to_string(),
                    (Arg::None, false) => String::new(),
                };
                (format!("{} {}", k.unwrap_or(' '), a.label), tag)
            })
            .collect();
        let title = format!("actions: {}", tree::label(&self.roots, path));
        self.draw_menu_items(f, area, title, items, sel);
    }

    /// A menu of (label, tag) items, the one at `sel` highlighted, each
    /// recorded as a click target.
    pub(super) fn draw_menu_items(&mut self, f: &mut Frame, area: Rect, title: String, items: Vec<(String, String)>, sel: usize) {
        let r = modal_rect(area, 50, items.len() as u16 + 2);
        let w = r.width;
        let lines: Vec<Line> = items
            .iter()
            .enumerate()
            .map(|(i, (label, tag))| {
                let style = if i == sel { theme::picked() } else { Style::new() };
                // Padded to the popup's inside, so the selection fills the row.
                Line::styled(format!("{:<1$}", format!(" {label:<10}{tag}"), w.saturating_sub(2) as usize), style)
            })
            .collect();
        let inner = r.inner(ratatui::layout::Margin::new(1, 1));
        let hits = (0..lines.len() as u16).map(|i| Rect { y: inner.y + i, height: 1, ..inner }.intersection(inner)).collect();
        self.hits.menu = Some((r, hits));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines).block(pane(title).border_style(theme::modal_border())), r);
    }

    /// The bottom line, built like the status line's first line: a mode
    /// lozenge, then flat parts between thin rules. A confirm draws its
    /// answers as lozenges and records them as click targets.
    fn draw_status(&mut self, f: &mut Frame, area: Rect) {
        let sh = self.shape;
        let mode = |label: &str, g: Ground| sh.lozenge(&[Seg::on(format!(" {label} "), g).bold()]);
        let input = |text: String| Span::raw(format!(" {text}▏ "));
        let hint = |text: &str| Span::styled(text.to_string(), theme::hint());
        let msg = Span::styled(
            format!("  {}", self.msg),
            if self.msg.starts_with("rejected") { theme::err().add_modifier(Modifier::BOLD) } else { Style::new() },
        );
        let mut spans: Vec<Span> = Vec::new();
        let mut cta = None;
        match &self.mode {
            Mode::Edit(buf) => {
                spans.extend(mode("edit", Ground::Accent));
                spans.extend([input(buf.clone()), hint(" Enter set · Esc cancel"), msg]);
            }
            Mode::Filter => {
                spans.extend(mode("/", Ground::Accent));
                spans.extend([input(self.filter.clone()), hint(" Enter keep · Esc clear")]);
            }
            Mode::Menu { .. } => {
                spans.extend(mode("action", Ground::Accent));
                spans.push(hint("  ↑↓ choose · Enter or click picks · Esc closes"));
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
            Mode::Confirm { queued } => {
                spans.extend(mode("confirm", Ground::Warn));
                spans.push(Span::raw(format!(" {}  ", queued.command)));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y queue ", Ground::Ok), (false, " n cancel ", Ground::Err)]);
            }
            Mode::Review { tab: t, discard: true, .. } | Mode::DiscardTab { tab: t } => {
                spans.extend(mode("confirm", Ground::Warn));
                spans.push(Span::raw(format!(" discard {} pending in {}?  ", self.pending_in(*t), self.roots[*t].name)));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y discard ", Ground::Err), (false, " n keep ", Ground::Ok)]);
            }
            Mode::Guard { confirm: true } => {
                spans.extend(mode("confirm", Ground::Warn));
                spans.push(Span::raw(format!(" quit and discard all {} pending?  ", self.pending())));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y quit ", Ground::Err), (false, " n back ", Ground::Ok)]);
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
            Mode::ThemeName { op, buf } => {
                spans.extend(mode(&op.prompt(), Ground::Accent));
                spans.extend([input(buf.clone()), hint(" Enter · Esc cancel"), msg]);
            }
            Mode::ThemeDelete { name } => {
                spans.extend(mode("confirm", Ground::Warn));
                spans.push(Span::raw(format!(" delete theme {name}?  ")));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y delete ", Ground::Err), (false, " n keep ", Ground::Ok)]);
            }
            Mode::ThemeMenu { .. } => {
                spans.extend(mode("action", Ground::Accent));
                spans.push(hint("  ↑↓ choose · Enter or click picks · Esc closes"));
            }
            Mode::ThemeUnsaved => {
                spans.extend(mode("unsaved", Ground::Warn));
                spans.push(hint("  s save · d discard · Esc back to the editor"));
            }
            Mode::Browse | Mode::Help { .. } if self.on_theme_tab() => {
                spans.extend(self.theme_status(sh));
                spans.push(msg);
            }
            Mode::Flow(flow) => {
                spans.extend(mode("flow", Ground::Accent));
                spans.push(hint(flow.hint()));
            }
            Mode::Guard { .. } => {
                spans.extend(mode("quit", Ground::Warn));
                spans.push(hint("  Esc back · r review · D quit and discard all"));
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
                // A row's actions take the place of the opening hint; a
                // message said since comes first, so it is never cut off.
                let here = self.footer_actions();
                if self.msg != crate::app::HINT || here.is_none() {
                    spans.push(msg);
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

    /// The selected row's actions as `key label` pairs, from their
    /// declaration: the row's own, else its tab's.
    fn footer_actions(&self) -> Option<Vec<Span<'static>>> {
        let rows = self.rows();
        let path = self.actions_at(&rows.get(self.cursor)?.path);
        let actions = &tree::get(&self.roots, &path).actions;
        if actions.is_empty() {
            return None;
        }
        let mut spans = Vec::new();
        for (i, (a, k)) in actions.iter().zip(tree::action_keys(actions)).enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", theme::hint()));
            }
            spans.push(Span::styled(k.map_or(String::new(), |k| format!("{k} ")), theme::accent()));
            spans.push(Span::styled(a.label.clone(), theme::hint()));
        }
        Some(spans)
    }

    /// Save, Discard or Back over the editor, Back focused.
    fn draw_unsaved(&mut self, f: &mut Frame, area: Rect) {
        let name = self.themes.editor.as_ref().map_or(String::new(), |e| e.theme.name.clone());
        let lines = vec![Line::raw(""), Line::styled(format!(" {name} has unsaved edits"), theme::changed()), Line::raw("")];
        let r = modal_rect(area, 56, lines.len() as u16 + 3);
        let inner = r.inner(ratatui::layout::Margin::new(1, 1));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines.clone()).block(pane("close the editor").border_style(theme::modal_border())), r);
        let row = Rect { y: inner.y + lines.len() as u16, height: 1, ..inner }.intersection(inner);
        let buttons = [(Btn::Save, "Save (s)".into()), (Btn::Drop, "Discard (d)".into()), (Btn::Back, "Back (Esc)".into())];
        f.render_widget(Paragraph::new(button_row(self.shape, row, Some(Btn::Back), &buttons, &mut self.hits.buttons)), row);
    }
}

fn kind_label(k: &Kind) -> String {
    match k {
        Kind::Bool => "bool (Enter toggles)".into(),
        Kind::Float { min, max } => format!("number {min}..={max} (Enter edits)"),
        Kind::Int { min, max } => format!("integer {min}..={max} (Enter edits)"),
        Kind::Choice(o) => format!("one of {} (Enter cycles)", o.join(" | ")),
        Kind::Text => "text (Enter edits)".into(),
        Kind::ReadOnly => "read-only here".into(),
        Kind::Secret => "secret (Enter enters it masked)".into(),
    }
}

/// The keys the shell answers to, as the help overlay lists them.
pub const KEYS: &[&str] = &[
    "Tab S-Tab 1-9  switch tab; each keeps its cursor and open groups",
    "↑↓ / j k     move          PgUp PgDn g G   jump",
    "→ / l        open group    ← / h           close / parent",
    "Enter Space  toggle bool, cycle choice, edit value, open group;",
    "             on a secret, masked entry; on an action-only node, its menu",
    "e            edit as text (^U clears)      d   set to default",
    "u            revert        /               filter all tabs by key",
    "Enter        on a filter hit: jump to it in its tab (Space acts)",
    "a            menu of the row's actions, or the tab's   x   unqueue the last",
    "             each action's own key runs it; the footer and detail name them",
    "c            pending pane (changes and queued actions)",
    "w Ctrl-S     review this tab's pending items, read-only (or click the ●",
    "             bar): a applies the tab, X discards it, Tab next tab, Esc back",
    "X ↺          discard this tab's pending items (y/n); ↺ on the tab badge",
    "q Esc ^C     quit; with items pending in any tab, asks first",
    "mouse        click a tab or row; click the selected row, or ▸ ▾, to act;",
    "             wheel moves; click a menu item, an answer, or a button",
    "m            mouse on or off (off lets the terminal select text)",
    "",
    "yellow = changed · blue = differs from default · grey = read-only",
    "[a] = has actions · orange = queued · ! = a finding · y/n answers a confirm",
    "theme tab    ↑↓ preview · Enter use · a actions · e edit (^S saves)",
];

impl App {
    /// The keys, then the shown tab's help as the adapter's own `--help`
    /// prints it, scrolled `scroll` lines.
    fn draw_help(&mut self, f: &mut Frame, area: Rect, scroll: u16) {
        let tab = self.tab_names().get(self.tab).cloned().unwrap_or_default();
        let mut lines: Vec<Line> = KEYS.iter().map(|l| Line::raw(*l)).collect();
        if let Some(here) = self.footer_actions() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("actions here", Style::new().add_modifier(Modifier::BOLD)));
            lines.push(Line::from(here));
        }
        if let Some(text) = self.adapter.help(&tab) {
            lines.push(Line::raw(""));
            lines.push(Line::styled(format!("help: {tab}"), Style::new().add_modifier(Modifier::BOLD)));
            lines.extend(text.lines().map(|l| Line::raw(l.to_string())));
        }
        let widest = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let r = modal_rect(area, widest.max(72) + 2, lines.len() as u16 + 2);
        let room = r.height.saturating_sub(2);
        let scroll = scroll.min((lines.len() as u16).saturating_sub(room));
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(lines).scroll((scroll, 0)).block(pane("keys and help · ↑↓ scroll · any other key closes").border_style(theme::modal_border())),
            r,
        );
    }
}
