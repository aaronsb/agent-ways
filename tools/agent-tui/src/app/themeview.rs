//! Drawing for the theme tab: the theme list and a theme's detail, and the
//! editor's slot rows and slot editor (sliders, hex, the role in context,
//! and the contrast and distinctness checks).

use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem, Paragraph};
use ratatui::Frame;

use super::render::{button_row, pane};
use super::review::{fields, LABEL};
use super::theme::{self, Ground, Seg, Shape};
use super::themestate::{Editor, Focus, ThemeAct, CHANNELS};
use super::{App, Btn};
use agent_theme::{contrast, Background, Kind, Rgb, Roles, Slots, Source, MIN_DISTINCT};

/// The slots a list row shows as a strip, in this order.
const STRIP: [&str; 8] = ["bg", "fg", "accent", "info", "ok", "warn", "err", "alt"];

fn kind_name(k: Kind) -> &'static str {
    if k == Kind::Dark { "dark" } else { "light" }
}

fn bg_name(b: Background) -> &'static str {
    if b == Background::Fill { "fill" } else { "terminal" }
}

/// `n` cells of `c` as a background: a swatch. Without colour, a block glyph.
fn swatch(c: Rgb, n: usize) -> Span<'static> {
    if theme::colourless() {
        return Span::raw("▒".repeat(n));
    }
    Span::styled(" ".repeat(n), Style::new().bg(theme::rgb(c)))
}

/// The role a slot is drawn as, and whether it is text on the ground.
fn role_of(slot: &str, r: &Roles) -> (&'static str, Rgb) {
    match slot {
        "accent" => ("accent", r.accent),
        "info" => ("info", r.info),
        "ok" => ("ok", r.ok),
        "warn" => ("warn", r.warn),
        "err" => ("err", r.err),
        "alt" => ("alt", r.alt),
        "dim" => ("muted", r.muted),
        _ => ("body", r.body),
    }
}

/// The contrast and distinctness checks of `r`, failing ones named in err.
fn checks(r: &Roles, room: usize) -> Vec<Line<'static>> {
    let dim = theme::muted();
    let bad = theme::err().add_modifier(Modifier::BOLD);
    let good = theme::ok();
    let mut out = Vec::new();
    let unreadable = r.unreadable();
    if unreadable.is_empty() {
        out.extend(fields("contrast", "ok", good, room, dim));
    }
    for (name, fg, g, c, min) in unreadable {
        out.extend(fields("contrast", &format!("{name} {} on {} is {c:.1}:1 < {min}", fg.hex(), g.hex()), bad, room, dim));
    }
    let close = r.too_close();
    if close.is_empty() {
        let s = r.status();
        let mut best = (f64::MAX, "", "");
        for (i, (an, a)) in s.iter().enumerate() {
            for (bn, b) in &s[i + 1..] {
                let d = agent_theme::delta_e(*a, *b);
                if d < best.0 {
                    best = (d, an, bn);
                }
            }
        }
        out.extend(fields("distinct", &format!("ok, {}/{} ΔE {:.3}", best.1, best.2, best.0), good, room, dim));
    }
    for (a, b, d) in close {
        out.extend(fields("distinct", &format!("{a}/{b} ΔE {d:.3} < {MIN_DISTINCT}"), bad, room, dim));
    }
    out
}

impl App {
    pub(super) fn theme_act_tag(&self, a: ThemeAct) -> String {
        let (t, src) = self.themes.under_cursor();
        match a {
            ThemeAct::New => "from agent-ways".into(),
            ThemeAct::Copy => format!("of {}", t.name),
            ThemeAct::Rename => "the active choice follows".into(),
            ThemeAct::Delete => "asks first".into(),
            ThemeAct::Edit if src == Source::Bundled => "bundled: edits a copy".into(),
            ThemeAct::Edit => String::new(),
            ThemeAct::Shape => format!("{} now; the next is {}", self.shape.name(), self.shape.next().name()),
        }
    }

    /// The bottom bar's mode and hints on the theme tab.
    pub(super) fn theme_status(&self, sh: Shape) -> Vec<Span<'static>> {
        let hint = |t: &str| Span::styled(t.to_string(), theme::hint());
        let mut spans = Vec::new();
        match &self.themes.editor {
            Some(e) => {
                spans.extend(sh.lozenge(&[Seg::on(if e.hex.is_some() { " hex " } else { " edit " }, Ground::Warn).bold()]));
                spans.push(hint(match (e.hex.is_some(), e.focus) {
                    (true, _) => "  type #rrggbb · Enter sets · Esc cancels",
                    (_, Focus::Slider(_)) => "  ↑↓ H S L · ←→ step, ⇧ coarse · # hex · ^S save · Esc rows",
                    _ => "  ↑↓ slot · Enter sliders · # hex · ^S save · Esc close",
                }));
            }
            None => {
                spans.extend(sh.lozenge(&[Seg::on(" theme ", Ground::Accent).bold()]));
                spans.push(hint("  ↑↓ preview · Enter use · a actions · e edit · Esc back"));
            }
        }
        spans
    }

    pub(super) fn draw_theme_tab(&mut self, f: &mut Frame, left: Rect, right: Rect) {
        if self.themes.editor.is_some() {
            self.draw_slot_rows(f, left);
            self.draw_slot_editor(f, right);
        } else {
            self.draw_theme_list(f, left);
            self.draw_theme_detail(f, right);
        }
    }

    fn draw_theme_list(&mut self, f: &mut Frame, area: Rect) {
        // Inside the border and the selection mark: the mark, the name, kind, source tag, then the strip.
        let inner = area.width.saturating_sub(3) as usize;
        let name_w = self.themes.list().iter().map(|(t, _)| t.name.chars().count()).max().unwrap_or(10).min(24) + 1;
        let strip = inner.saturating_sub(2 + name_w + 7 + 9).min(STRIP.len());
        let active = self.themes.active.clone();
        let items: Vec<ListItem> = self
            .themes
            .list()
            .iter()
            .map(|(t, src)| {
                let tag = match src {
                    Source::Bundled => "",
                    Source::User => "user",
                    Source::Override => "override",
                };
                let mark = if t.name == active { Span::styled("● ", theme::ok()) } else { Span::raw("  ") };
                let mut spans = vec![
                    mark,
                    Span::raw(format!("{:<name_w$}", t.name)),
                    Span::styled(format!("{:<6} ", kind_name(t.kind)), theme::read_only()),
                    Span::styled(format!("{tag:<9}"), theme::non_default()),
                ];
                spans.extend(STRIP.iter().take(strip).map(|s| swatch(t.slots.get(s).unwrap(), 1)));
                ListItem::new(Line::from(spans)).style(theme::selected_text())
            })
            .collect();
        let block = pane(" themes ");
        self.hits.list = block.inner(area);
        let list = List::new(items)
            .block(block)
            .highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        self.list.select(Some(self.themes.cursor));
        f.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_theme_detail(&self, f: &mut Frame, area: Rect) {
        let (t, src) = self.themes.under_cursor();
        let room = (area.width as usize).saturating_sub(2 + LABEL);
        let dim = theme::muted();
        let field = |k: &str, v: &str| fields(k, v, Style::new(), room, dim);
        let mut lines = vec![
            Line::styled(t.label.clone(), Style::new().add_modifier(Modifier::BOLD)),
            Line::styled(format!("{} · {} · background {}", t.name, kind_name(t.kind), bg_name(t.background)), dim),
            Line::raw(""),
        ];
        let path = self.themes.path_of(&t.name).map(|p| self.themes.show(&p)).unwrap_or_default();
        lines.extend(match src {
            Source::Bundled => field("source", "bundled (read-only)"),
            Source::User => field("source", &format!("user  {path}")),
            Source::Override => field("source", &format!("override  {path}")),
        });
        let active = t.name == self.themes.active;
        lines.extend(fields("active", if active { "yes" } else { "no (Enter)" }, if active { theme::ok() } else { Style::new() }, room, dim));
        let dir = self.themes.dir.as_ref().map_or("none: nothing saves".to_string(), |d| self.themes.show(d));
        lines.extend(field("saves to", &dir));
        lines.extend(field("shape", &format!("{} (a: shape cycles it)", self.shape.name())));
        lines.push(Line::raw(""));
        lines.push(Line::styled("slots", Style::new().add_modifier(Modifier::BOLD)));
        let inner = area.width.saturating_sub(2) as usize;
        // Two columns when they fit: a swatch, the name, the hex.
        let cols = if inner >= 33 { 2 } else { 1 };
        let sw = if inner >= 36 { 2 } else { 1 };
        let rows = Slots::NAMES.len().div_ceil(cols);
        for r in 0..rows {
            let mut spans = Vec::new();
            for c in 0..cols {
                if let Some(n) = Slots::NAMES.get(c * rows + r) {
                    let col = t.slots.get(n).unwrap();
                    spans.extend([swatch(col, sw), Span::styled(format!(" {n:<7}"), dim), Span::raw(col.hex()), Span::raw(" ")]);
                }
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::raw(""));
        lines.extend(checks(&self.themes.roles(t), room));
        if !self.themes.set.rejected.is_empty() {
            lines.push(Line::raw(""));
            for (file, errs) in &self.themes.set.rejected {
                lines.extend(fields("rejected", &format!("{file}: {}", errs.first().map(|e| e.to_string()).unwrap_or_default()), theme::err(), room, dim));
            }
        }
        f.render_widget(Paragraph::new(lines).block(pane("theme")), area);
    }

    fn draw_slot_rows(&mut self, f: &mut Frame, area: Rect) {
        let e = self.themes.editor.as_ref().expect("editor open");
        let mut items: Vec<ListItem> = Slots::NAMES
            .iter()
            .map(|n| {
                let c = e.theme.slots.get(n).unwrap();
                let changed = e.saved.slots.get(n) != Some(c);
                let hex = Span::styled(c.hex(), if changed { theme::changed() } else { Style::new() });
                ListItem::new(Line::from(vec![Span::raw(format!(" {n:<11}")), swatch(c, 3), Span::raw("  "), hex]))
            })
            .collect();
        for (label, value, changed) in [
            ("kind", kind_name(e.theme.kind), e.theme.kind != e.saved.kind),
            ("background", bg_name(e.theme.background), e.theme.background != e.saved.background),
        ] {
            let st = if changed { theme::changed() } else { Style::new() };
            items.push(ListItem::new(Line::from(vec![Span::raw(format!(" {label:<16}")), Span::styled(value, st)])));
        }
        let items: Vec<ListItem> = items.into_iter().map(|i| i.style(theme::selected_text())).collect();
        let mark = if e.dirty() { " ●" } else { "" };
        let note = match (&e.origin, e.written) {
            (Some(o), false) => format!(" (copy of {o}, unwritten)"),
            _ => String::new(),
        };
        let block = pane(format!(" edit {}{mark}{note} ", e.theme.name));
        self.hits.list = block.inner(area);
        let row = e.row;
        let list = List::new(items)
            .block(block)
            .highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::SELECTED_MARK, theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        self.list.select(Some(row));
        f.render_stateful_widget(list, area, &mut self.list);
    }

    /// One slider: its label, a track of the channel's range in colour with
    /// the knob at the value, and the value. Records the track's rect.
    fn slider(&mut self, e: &Editor, ch: usize, at: Rect) -> Line<'static> {
        let (name, top, _, unit) = CHANNELS[ch];
        let hsl = e.hsl();
        let focused = e.focus == Focus::Slider(ch);
        let label = if focused { Span::styled(format!("▸{name} "), theme::accent().add_modifier(Modifier::BOLD)) } else { Span::styled(format!(" {name} "), theme::muted()) };
        let tw = at.width.saturating_sub(3 + 6).max(4) as usize;
        let knob = ((hsl[ch] / top) * (tw - 1) as f64).round() as usize;
        let mut spans = vec![label];
        for i in 0..tw {
            let mut v = hsl;
            v[ch] = i as f64 / (tw - 1) as f64 * top;
            let c = Rgb::from_hsl(v);
            if theme::colourless() {
                spans.push(Span::raw(if i == knob { "●" } else { "─" }));
            } else if i == knob {
                let ink = if contrast(c, Rgb(0, 0, 0)) > contrast(c, Rgb(255, 255, 255)) { Rgb(0, 0, 0) } else { Rgb(255, 255, 255) };
                spans.push(Span::styled("┃", Style::new().fg(theme::rgb(ink)).bg(theme::rgb(c)).add_modifier(Modifier::BOLD)));
            } else {
                spans.push(swatch(c, 1));
            }
        }
        spans.push(Span::raw(format!(" {:>3.0}{unit}", hsl[ch])));
        self.hits.sliders.push((Rect { x: at.x + 3, y: at.y, width: tw as u16, height: 1 }, ch));
        Line::from(spans)
    }

    fn draw_slot_editor(&mut self, f: &mut Frame, area: Rect) {
        let e = self.themes.editor.take().expect("editor open");
        let inner = area.inner(Margin::new(1, 1));
        let room = (inner.width as usize).saturating_sub(LABEL);
        let dim = theme::muted();
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let roles = self.themes.roles(&e.theme);
        let row_at = |lines: &Vec<Line>| Rect { y: inner.y + lines.len() as u16, height: 1, ..inner };
        let mut lines: Vec<Line> = Vec::new();
        match (e.slot(), e.color()) {
            (Some(slot), Some(c)) => {
                lines.push(Line::from(vec![Span::styled(format!("{slot}  "), bold), swatch(c, 2), Span::raw(format!(" {}", c.hex()))]));
                let (role, rc) = role_of(slot, &roles);
                // One line, so the sliders below never move.
                let how = if rc == c { vec![Span::raw("as is")] } else { vec![swatch(rc, 2), Span::raw(format!(" {} moved to read", rc.hex()))] };
                lines.push(Line::from([vec![Span::styled(format!("{role:<LABEL$}"), dim)], how].concat()));
                lines.push(Line::raw(""));
                for ch in 0..3 {
                    let at = row_at(&lines);
                    let l = self.slider(&e, ch, at);
                    lines.push(l);
                }
                self.hits.hex = Rect { width: 16, ..row_at(&lines) };
                lines.push(match &e.hex {
                    Some(buf) => Line::from(vec![Span::styled(" # ", theme::accent().add_modifier(Modifier::BOLD)), Span::raw(format!("{buf}▏"))]),
                    None => Line::from(vec![Span::styled(" # ", dim), Span::raw(c.hex()), Span::styled("  # or e types it", theme::hint())]),
                });
                lines.push(Line::raw(""));
                lines.push(Line::styled("in context", bold));
                let (bg, sel) = (theme::bg_color(), theme::shade_color());
                let rcol = theme::rgb(rc);
                lines.push(Line::from(vec![
                    Span::styled(format!(" {role} "), Style::new().fg(rcol).bg(bg)),
                    Span::styled(format!(" {:.1} ", contrast(rc, roles.bg)), dim),
                    Span::styled(format!(" {role} "), Style::new().fg(rcol).bg(sel)),
                    Span::styled(format!(" {:.1} on selection", contrast(rc, roles.selection_bg)), dim),
                ]));
                let row = Style::new().bg(sel);
                lines.push(Line::from(vec![
                    Span::styled("▌", row.patch(theme::accent())),
                    Span::styled(" model  ", row.patch(theme::body()).add_modifier(Modifier::BOLD)),
                    Span::styled("opus ", row.patch(theme::accent_dim())),
                    Span::styled("changed ", row.patch(theme::warn()).add_modifier(Modifier::BOLD)),
                    Span::styled("queued ", row.patch(theme::hot())),
                    Span::styled(format!("{role} "), row.fg(rcol)),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("  effort "),
                    Span::styled("high ", theme::ok()),
                    Span::styled("failed ", theme::err()),
                    Span::styled("info ", theme::info()),
                    Span::styled("ro", theme::read_only().add_modifier(Modifier::ITALIC)),
                ]));
                let mut segs = vec![Span::raw(" ")];
                for (label, g) in [(" browse ", Ground::Accent), (" edit ", Ground::Warn), (" review ", Ground::Info), (" apply ", Ground::Ok)] {
                    segs.extend(self.shape.lozenge(&[Seg::on(label, g).bold()]));
                    segs.push(Span::raw(" "));
                }
                lines.push(Line::from(segs));
            }
            _ => {
                let (name, value, other) = if e.row == Slots::NAMES.len() {
                    ("kind", kind_name(e.theme.kind), "dark | light: which end of bg and fg is ink")
                } else {
                    ("background", bg_name(e.theme.background), "terminal leaves the terminal's own; fill paints bg behind every cell")
                };
                lines.push(Line::from(vec![Span::styled(format!("{name}  "), bold), Span::raw(value)]));
                lines.extend(fields("", other, dim, room, dim));
                lines.extend(fields("", "Enter, ←→ or a click switches it", theme::hint(), room, dim));
            }
        }
        lines.push(Line::raw(""));
        lines.extend(checks(&roles, room));
        lines.push(Line::raw(""));
        let save_at = row_at(&lines);
        lines.push(Line::raw(""));
        let path = self.themes.path_of(&e.theme.name).map_or("no themes directory".into(), |p| self.themes.show(&p));
        lines.extend(fields("writes", &path, dim, room, dim));
        f.render_widget(Paragraph::new(lines).block(pane("slot")), area);
        if save_at.y < inner.y + inner.height {
            let state = if e.dirty() { Span::styled("unsaved", theme::changed()) } else { Span::styled("saved", theme::ok()) };
            let mut line = button_row(self.shape, save_at, None, &[(Btn::Save, "Save ^S".into())], &mut self.hits.buttons);
            line.spans.push(state);
            f.render_widget(Paragraph::new(line), save_at);
        }
        self.themes.editor = Some(e);
    }
}
