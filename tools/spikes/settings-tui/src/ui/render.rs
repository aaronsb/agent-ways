//! Drawing for the TUI: the tab bar, the tree, the detail and pending panes, the action
//! menu, the quit prompt, the status line and the key help. Review mode draws
//! its own tree and detail in `review`. Each frame also records where the
//! clickable parts landed, in `App::hits`.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::{self, Seg, Shape};
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
        let (fg, bg) = match b {
            Btn::Apply => (theme::INK, theme::OK),
            Btn::Discard | Btn::Quit => (theme::INK, theme::ERR),
            Btn::Back => (theme::TEXT, theme::ACCENT_DIM),
            Btn::Review | Btn::Next => (theme::INK, theme::ACCENT),
            Btn::Finish => (theme::INK, theme::OK),
            Btn::Cancel => (theme::TEXT, theme::RULE),
        };
        let on = focus == Some(*b);
        let seg = Seg::new(format!(" {} {label} ", if on { "›" } else { " " }), fg, bg);
        let target = sh.lozenge(&[if on { seg.bold() } else { seg }]);
        let x = at.x + width(&spans);
        hits.push((Rect { x, y: at.y, width: width(&target), height: 1 }.intersection(at), *b));
        spans.extend(target);
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

/// A confirm's two answers as lozenges, each recorded as a click target.
fn answer_lozenges(sh: Shape, area: Rect, spans: &mut Vec<Span<'static>>, hits: &mut Vec<(Rect, bool)>, answers: [(bool, &'static str, Color); 2]) {
    for (yes, text, bg) in answers {
        let target = sh.lozenge(&[Seg::new(text, theme::INK, bg).bold()]);
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

/// A modal of `w` by `h` centred in `area`. One as tall as the area takes its
/// whole width too: narrower, it would leave the start of the tree's title
/// showing beside its top-left corner.
pub(super) fn modal_rect(area: Rect, w: u16, h: u16) -> Rect {
    let h = h.min(area.height);
    let w = if h == area.height { area.width } else { w.min(area.width) };
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// A fixed-width mask once anything is typed, so the key's length never
/// reaches the screen.
fn mask(n: usize) -> String {
    if n == 0 { String::new() } else { "••••••••".into() }
}

impl App {
    pub(super) fn draw(&mut self, f: &mut Frame) {
        let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
        let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
        self.hits.menu = None;
        self.hits.answers.clear();
        self.hits.buttons.clear();
        self.hits.cta = Rect::default();
        self.hits.discard_tabs.clear();
        // Review keeps the browser's layout; the tree and the detail show what is pending.
        if let Mode::Review { tab, .. } = &self.mode {
            let tab = *tab;
            self.draw_tabs(f, bar, tab, true);
            self.draw_review_tree(f, left, tab);
            self.draw_review_detail(f, right, tab);
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
            self.draw_detail(f, right, &r.path);
        }
        if let Mode::Flow(flow) = &mut self.mode {
            flow.draw(f, main, self.shape);
        }
        match &self.mode {
            Mode::Help => draw_help(f, main),
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
            let mut segs = vec![if i == active {
                Seg::new(label, theme::INK, theme::ACCENT).bold()
            } else if review && pending == 0 {
                Seg::new(label, theme::MUTED, theme::DIM_TAB)
            } else {
                Seg::new(label, theme::TEXT, theme::ACCENT_DIM)
            }];
            if pending > 0 {
                segs.push(Seg::new(format!(" ●{pending} ↺ "), theme::INK, theme::WARN).bold());
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
                let pad = value_col.saturating_sub(left.chars().count()).max(1);
                spans.push(Span::raw(" ".repeat(pad)));
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
                let c = n.changes() + self.queue.under(&tree::key(&self.roots, &r.path));
                if c > 0 && !n.children.is_empty() {
                    spans.push(Span::styled(format!("  ●{c}"), Style::new().fg(theme::WARN)));
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
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, Style::new().fg(theme::ACCENT)))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
        self.list.select(if rows.is_empty() { None } else { Some(self.cursor) });
        f.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_detail(&self, f: &mut Frame, area: Rect, path: &[usize]) {
        let n = tree::get(&self.roots, path);
        let dim = Style::new().fg(theme::MUTED);
        let mut lines = vec![Line::styled(tree::key(&self.roots, path), Style::new().add_modifier(Modifier::BOLD)), Line::raw("")];
        if !n.doc.is_empty() {
            lines.push(Line::raw(n.doc.clone()));
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
                lines.push(field("writes", format!("{}  [{}]", st.file.display(), st.key)));
            }
        } else {
            let c = n.changes();
            lines.push(Line::styled(format!("{} entries, {} changed", n.children.len(), c), dim));
        }
        if !n.actions.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("actions (a)", dim));
            for a in &n.actions {
                let tag = if a.confirm { " (asks first)" } else { "" };
                lines.push(Line::raw(format!("  {}{tag}", a.label)));
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
        let mut files: Vec<String> = ch.iter().map(|c| c.1.as_ref().map_or("(no store)".into(), |s| s.file.display().to_string())).collect();
        files.sort();
        files.dedup();
        for file in files {
            lines.push(Line::styled(file.clone(), Style::new().add_modifier(Modifier::BOLD)));
            for (k, st, from, to) in &ch {
                if st.as_ref().map_or("(no store)".to_string(), |s| s.file.display().to_string()) == file {
                    let key = st.as_ref().map_or(k.clone(), |s| s.key.clone());
                    lines.push(Line::from(vec![
                        Span::raw(format!("  {key}: ")),
                        Span::styled(from.clone(), Style::new().fg(theme::ERR)),
                        Span::raw(" → "),
                        Span::styled(to.clone(), Style::new().fg(theme::OK)),
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
        lines.push(Line::styled("spike: nothing is written or run", theme::hint()));
        let title = format!("pending ({} changes, {} actions)", ch.len(), self.queue.len());
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(pane(title));
        f.render_widget(p, area);
    }

    /// The quit prompt over the tree: every tab with pending items and its
    /// count, then the choices.
    fn draw_guard(&mut self, f: &mut Frame, area: Rect) {
        let tabs: Vec<(usize, &str, usize)> =
            self.roots.iter().enumerate().map(|(i, r)| (i, r.name.as_str(), self.pending_in(i))).filter(|t| t.2 > 0).collect();
        let first = tabs.first().map_or(0, |t| t.0);
        let mut lines = vec![Line::raw(""), Line::from(Span::styled(format!(" {} unsaved in {} tabs", self.pending(), tabs.len()), theme::changed()))];
        lines.extend(tabs.iter().map(|(_, name, n)| Line::from(vec![Span::raw(format!("   {name:<12}")), Span::styled(format!("●{n}"), theme::changed())])));
        lines.push(Line::raw(""));
        let name = self.roots[first].name.clone();
        let r = modal_rect(area, 74, lines.len() as u16 + 3);
        let inner = r.inner(ratatui::layout::Margin::new(1, 1));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines.clone()).block(pane("quit with unsaved changes").border_style(Style::new().fg(theme::ACCENT_DIM))), r);
        let row = Rect { y: inner.y + lines.len() as u16, height: 1, ..inner }.intersection(inner);
        let buttons = [(Btn::Back, "Back (Esc)".into()), (Btn::Review, format!("Review {name} (r)")), (Btn::Quit, "Quit and discard all (D)".into())];
        f.render_widget(Paragraph::new(button_row(self.shape, row, Some(Btn::Back), &buttons, &mut self.hits.buttons)), row);
    }

    fn draw_menu(&mut self, f: &mut Frame, area: Rect, path: &[usize], sel: usize) {
        let n = tree::get(&self.roots, path);
        let r = modal_rect(area, 50, n.actions.len() as u16 + 2);
        let w = r.width;
        let lines: Vec<Line> = n
            .actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let tag = match (&a.arg, a.confirm) {
                    (Arg::Secret, _) => "  masked".to_string(),
                    (Arg::Flow(_), _) => "  guided".to_string(),
                    (Arg::Text(p), true) => format!("  {p}, asks first"),
                    (Arg::Text(p), false) => format!("  {p}"),
                    (Arg::None, true) => "  asks first".to_string(),
                    (Arg::None, false) => String::new(),
                };
                let style = if i == sel { Style::new().fg(theme::INK).bg(theme::ACCENT).add_modifier(Modifier::BOLD) } else { Style::new() };
                // Padded to the popup's inside, so the selection fills the row.
                Line::styled(format!("{:<1$}", format!(" {:<10}{tag}", a.label), w.saturating_sub(2) as usize), style)
            })
            .collect();
        let inner = r.inner(ratatui::layout::Margin::new(1, 1));
        let items = (0..lines.len() as u16).map(|i| Rect { y: inner.y + i, height: 1, ..inner }.intersection(inner)).collect();
        self.hits.menu = Some((r, items));
        f.render_widget(Clear, r);
        let title = format!("actions: {}", tree::key(&self.roots, path));
        f.render_widget(Paragraph::new(lines).block(pane(title).border_style(Style::new().fg(theme::ACCENT_DIM))), r);
    }

    /// The bottom line, built like the status line's first line: a mode
    /// lozenge, then flat parts between thin rules. A confirm draws its
    /// answers as lozenges and records them as click targets.
    fn draw_status(&mut self, f: &mut Frame, area: Rect) {
        let sh = self.shape;
        let mode = |label: &str, bg: Color| sh.lozenge(&[Seg::new(format!(" {label} "), theme::INK, bg).bold()]);
        let input = |text: String| Span::raw(format!(" {text}▏ "));
        let hint = |text: &str| Span::styled(text.to_string(), theme::hint());
        let msg = Span::styled(
            format!("  {}", self.msg),
            if self.msg.starts_with("rejected") { Style::new().fg(theme::ERR).add_modifier(Modifier::BOLD) } else { Style::new() },
        );
        let mut spans: Vec<Span> = Vec::new();
        let mut cta = None;
        match &self.mode {
            Mode::Edit(buf) => {
                spans.extend(mode("edit", theme::ACCENT));
                spans.extend([input(buf.clone()), hint(" Enter set · Esc cancel"), msg]);
            }
            Mode::Filter => {
                spans.extend(mode("/", theme::ACCENT));
                spans.extend([input(self.filter.clone()), hint(" Enter keep · Esc clear")]);
            }
            Mode::Menu { .. } => {
                spans.extend(mode("action", theme::ACCENT));
                spans.push(hint("  ↑↓ choose · Enter or click picks · Esc closes"));
            }
            Mode::Arg { path, action, buf } => {
                let prompt = match &tree::get(&self.roots, path).actions[*action].arg {
                    Arg::Text(p) => p.clone(),
                    _ => "argument".into(),
                };
                spans.extend(mode(&prompt, theme::ACCENT));
                spans.extend([input(buf.clone()), hint(" Enter queue · Esc cancel")]);
            }
            Mode::Secret { buf, .. } => {
                spans.extend(mode("secret", theme::HOT));
                spans.extend([input(mask(buf.len())), hint(" Enter queue (goes to stdin, never shown) · Esc cancel")]);
            }
            Mode::Confirm { queued } => {
                spans.extend(mode("confirm", theme::WARN));
                spans.push(Span::raw(format!(" {}  ", queued.command)));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y queue ", theme::OK), (false, " n cancel ", theme::ERR)]);
            }
            Mode::Review { tab: t, discard: true, .. } | Mode::DiscardTab { tab: t } => {
                spans.extend(mode("confirm", theme::WARN));
                spans.push(Span::raw(format!(" discard {} pending in {}?  ", self.pending_in(*t), self.roots[*t].name)));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y discard ", theme::ERR), (false, " n keep ", theme::OK)]);
            }
            Mode::Guard { confirm: true } => {
                spans.extend(mode("confirm", theme::WARN));
                spans.push(Span::raw(format!(" quit and discard all {} pending?  ", self.pending())));
                answer_lozenges(sh, area, &mut spans, &mut self.hits.answers, [(true, " y quit ", theme::ERR), (false, " n back ", theme::OK)]);
            }
            Mode::Review { run: Some(run), .. } => {
                let (label, text) = match run.outcome {
                    Outcome::Stopped(_) => ("stopped", "  the failed step and the rest stay pending"),
                    _ => ("applying", "  simulated: a step per tick"),
                };
                spans.extend(mode(label, theme::HOT));
                spans.extend([hint(text), msg]);
            }
            Mode::Review { tab, .. } => {
                spans.extend(mode("review", theme::HOT));
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
            Mode::Flow(flow) => {
                spans.extend(mode("flow", theme::ACCENT));
                spans.push(hint(flow.hint()));
            }
            Mode::Guard { .. } => {
                spans.extend(mode("quit", theme::WARN));
                spans.push(hint("  Esc back · r review · D quit and discard all"));
            }
            _ => {
                spans.extend(mode(if self.filter.is_empty() { "browse" } else { "filter" }, theme::ACCENT));
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
                    cta = Some(sh.lozenge(&[Seg::new(text, theme::INK, theme::HOT).bold()]));
                } else {
                    spans.push(hint(if self.mouse { "mouse on (m)" } else { "mouse off (m)" }));
                }
                spans.push(msg);
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

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = [
        "Tab S-Tab 1-9  switch tab; each keeps its cursor and open groups",
        "↑↓ / j k     move          PgUp PgDn g G   jump",
        "→ / l        open group    ← / h           close / parent",
        "Enter Space  toggle bool, cycle choice, edit value, open group;",
        "             on a secret, masked entry; on an action-only node, its menu",
        "e            edit as text  d               set to default",
        "u            revert        /               filter all tabs by key",
        "Enter        on a filter hit: jump to it in its tab (Space acts)",
        "a            actions of the row, or of the tab   x   unqueue the last",
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
        "[a] = has actions · orange = queued · y/n answers a confirm",
        "any key closes",
    ];
    let r = modal_rect(area, 72, lines.len() as u16 + 2);
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(lines.iter().map(|l| Line::raw(*l)).collect::<Vec<_>>()).block(pane("keys").border_style(Style::new().fg(theme::ACCENT_DIM))),
        r,
    );
}
