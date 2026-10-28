//! The theme tab's Claude Code section: the mod that draws the theme in
//! Claude Code, the palette the theme maps to there, the mod's render
//! settings and the words in effect. The adapter supplies all but the
//! palette ([`crate::adapter::ClaudeCode`]). `c` opens the settings as rows
//! in place of the theme list, as `e` opens the slot rows, and a row's
//! value steps through its options and is kept at once, as a theme choice
//! is.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{HighlightSpacing, List, ListItem, Paragraph};
use ratatui::Frame;

use super::render::pane;
use super::review::{fields, LABEL};
use super::theme;
use super::themestate::DEFAULT;
use super::themeview::swatch;
use super::App;
use crate::adapter::ClaudeCode;
use agent_theme::{text_on, Rgb, Roles, Theme, MIN_TEXT};

/// The roles the palette preview shows, in its order.
const PALETTE: [&str; 6] = ["accent", "body", "ok", "warn", "err", "info"];

fn role(r: &Roles, name: &str) -> Rgb {
    match name {
        "accent" => r.accent,
        "ok" => r.ok,
        "warn" => r.warn,
        "err" => r.err,
        "info" => r.info,
        _ => r.body,
    }
}

/// The words in effect under `t`, each list the setting's when set, else
/// the theme's, and where each came from.
fn words(cc: &ClaudeCode, t: &Theme) -> [(&'static str, Vec<String>, &'static str); 2] {
    let pick = |own: &[String], theirs: &[String], key: &'static str| -> (Vec<String>, &'static str) {
        match (own.is_empty(), theirs.is_empty()) {
            (false, _) => (own.to_vec(), key),
            (true, false) => (theirs.to_vec(), "the theme's [words]"),
            (true, true) => (Vec::new(), "Claude Code's own"),
        }
    };
    let (s, sf) = pick(&cc.spinner, &t.words.spinner, "mods.ways-theme.spinner_words");
    let (d, df) = pick(&cc.done, &t.words.done, "mods.ways-theme.done_words");
    [("spinner", s, sf), ("done", d, df)]
}

impl App {
    /// Read the section again from the adapter; the row cursor stays in range.
    pub(super) fn refresh_claude_code(&mut self) {
        self.claude_code = self.adapter.claude_code();
        let n = self.claude_code.as_ref().map_or(0, |c| c.render.len());
        if let Some(row) = self.themes.claude.as_mut() {
            *row = (*row).min(n.saturating_sub(1));
        }
    }

    /// Open the section's settings in place of the theme list.
    pub(super) fn open_claude_code(&mut self) {
        match &self.claude_code {
            Some(cc) if !cc.render.is_empty() => {
                self.themes.claude = Some(0);
                self.msg = "Claude Code: Enter or ←→ changes a setting; it is kept at once".into();
            }
            _ => self.msg = "this application draws nothing in Claude Code".into(),
        }
    }

    /// The bottom bar's lozenge label and hint while the section is open.
    pub(super) fn claude_code_hint(&self) -> &'static str {
        "  ↑↓ setting · Enter ←→ changes it · Esc back"
    }

    /// Step the setting under the cursor `by` options, and keep it.
    fn step_claude_code(&mut self, by: isize) {
        let Some(row) = self.themes.claude else { return };
        let Some(r) = self.claude_code.as_ref().and_then(|c| c.render.get(row)).cloned() else { return };
        if r.options.is_empty() {
            return;
        }
        let n = r.options.len() as isize;
        let at = r.options.iter().position(|o| *o == r.value).map_or(0, |i| i as isize + by);
        let next = r.options[at.rem_euclid(n) as usize].clone();
        match self.adapter.set_claude_code(&r.key, &next) {
            Ok(()) => {
                self.own_write();
                self.refresh_claude_code();
                self.msg = format!("{} = {next}", r.key);
            }
            Err(e) => self.msg = format!("rejected: {e}"),
        }
    }

    /// The section's keys while its rows are open. False ends the session.
    pub(super) fn claude_code_key(&mut self, k: KeyEvent) -> bool {
        let Some(row) = self.themes.claude else { return true };
        let last = self.claude_code.as_ref().map_or(0, |c| c.render.len().saturating_sub(1));
        match k.code {
            KeyCode::Esc => {
                self.themes.claude = None;
                self.msg = "Claude Code settings closed".into();
            }
            KeyCode::Up | KeyCode::Char('k') => self.themes.claude = Some(row.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.themes.claude = Some((row + 1).min(last)),
            KeyCode::Home | KeyCode::Char('g') => self.themes.claude = Some(0),
            KeyCode::End | KeyCode::Char('G') => self.themes.claude = Some(last),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ' | 'l') => self.step_claude_code(1),
            KeyCode::Left | KeyCode::Char('h') => self.step_claude_code(-1),
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Tab => self.switch_tab((self.tab + 1) % self.tabs()),
            KeyCode::BackTab => self.switch_tab((self.tab + self.tabs() - 1) % self.tabs()),
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < self.tabs() => self.switch_tab(c as usize - '1' as usize),
            KeyCode::Char('m') => self.toggle_mouse(),
            KeyCode::Char('?') => self.mode = super::Mode::Help { scroll: 0 },
            _ => {}
        }
        true
    }

    /// A click on row `i`: a second click on the selected row changes it.
    pub(super) fn claude_code_click(&mut self, i: usize) {
        let n = self.claude_code.as_ref().map_or(0, |c| c.render.len());
        if i >= n {
            return;
        }
        if self.themes.claude == Some(i) {
            self.step_claude_code(1);
        } else {
            self.themes.claude = Some(i);
        }
    }

    /// The palette `t` maps to in Claude Code, on one line after `label`:
    /// each role a swatch, named inside it where `room` allows.
    fn claude_code_palette(&self, t: &Theme, room: usize, named: bool) -> Line<'static> {
        let dim = theme::muted();
        let mut spans = vec![Span::styled(format!("{:<LABEL$}", "palette"), dim)];
        if t.name == DEFAULT {
            spans.push(Span::raw("Claude Code's own dark"));
            return Line::from(spans);
        }
        let r = self.themes.claude_code_roles(t);
        let wide: usize = PALETTE.iter().map(|n| n.len() + 2).sum();
        for n in PALETTE {
            let c = role(&r, n);
            if named && room >= wide && !theme::colourless() {
                let ink = text_on(c, &[r.bg, r.body], MIN_TEXT);
                spans.push(Span::styled(format!(" {n} "), Style::new().fg(theme::rgb(ink)).bg(theme::rgb(c))));
            } else {
                spans.extend([swatch(c, 2), Span::raw(" ")]);
            }
        }
        Line::from(spans)
    }

    /// The section's line in the theme's detail: the palette swatches, and
    /// whether the mod is active.
    pub(super) fn claude_code_brief(&self, t: &Theme, room: usize) -> Vec<Line<'static>> {
        let Some(cc) = &self.claude_code else { return Vec::new() };
        let mut line = self.claude_code_palette(t, room, false);
        line.spans[0].content = format!("{:<LABEL$}", "claude").into();
        let (word, st) = if cc.targets.iter().any(|(_, on, _)| *on) { ("active", theme::ok()) } else { ("not active", theme::read_only()) };
        line.spans.push(Span::styled(word, st));
        vec![Line::raw(""), line]
    }

    /// The rows of the render settings, in place of the theme list.
    pub(super) fn draw_claude_code_rows(&mut self, f: &mut Frame, area: Rect) {
        let Some(cc) = &self.claude_code else { return };
        let w = cc.render.iter().map(|r| r.label.chars().count()).max().unwrap_or(10) + 2;
        let items: Vec<ListItem> = cc
            .render
            .iter()
            .map(|r| {
                let st = if r.value == r.default { Style::new() } else { theme::non_default() };
                ListItem::new(Line::from(vec![Span::raw(format!(" {:<w$}", r.label)), Span::styled(r.value.clone(), st)])).style(theme::selected_text())
            })
            .collect();
        let block = pane(" claude code ", area);
        self.hits.list = block.inner(area);
        let list = List::new(items)
            .block(block)
            .highlight_style(theme::selected())
            .highlight_symbol(Line::styled(theme::selected_mark(), theme::accent()))
            .highlight_spacing(HighlightSpacing::Always);
        self.list.select(self.themes.claude);
        f.render_stateful_widget(list, area, &mut self.list);
    }

    /// The section for the theme under the cursor: the mod in each target,
    /// the palette, the selected setting, the words in effect, and the file
    /// a change is kept in.
    pub(super) fn draw_claude_code_detail(&self, f: &mut Frame, area: Rect) {
        let Some(cc) = &self.claude_code else { return };
        let (t, _) = self.themes.under_cursor();
        let room = (area.width as usize).saturating_sub(2 + LABEL);
        let dim = theme::muted();
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let mut lines = vec![Line::styled(format!("{} in Claude Code", t.label), bold), Line::raw("")];
        if cc.targets.is_empty() {
            lines.extend(fields("mod", "no enabled target", theme::read_only(), room, dim));
        }
        for (dir, on, state) in &cc.targets {
            let st = if *on { theme::ok() } else { theme::read_only() };
            lines.extend(fields("mod", &format!("{state} in {dir}"), st, room, dim));
        }
        lines.push(self.claude_code_palette(t, room, true));
        lines.push(Line::raw(""));
        if let Some(r) = self.themes.claude.and_then(|i| cc.render.get(i)) {
            lines.push(Line::from(vec![Span::styled(r.label.clone(), bold), Span::raw(format!("  {}  ", r.value)), Span::styled(r.options.join(" | "), dim)]));
            lines.extend(fields("", &r.doc, dim, room, dim));
            if r.key.ends_with("prompt_box") && r.value == "gradient" && t.gradient.is_none() {
                lines.extend(fields("", &format!("{} has no gradient, so it draws as frame", t.name), theme::warn(), room, dim));
            }
            lines.push(Line::raw(""));
        }
        for (name, list, from) in words(cc, t) {
            let shown = if list.is_empty() { "—".to_string() } else { list.join(" ") };
            lines.extend(fields(name, &shown, Style::new(), room, dim));
            lines.extend(fields("", &format!("from {from}"), dim, room, dim));
        }
        lines.push(Line::raw(""));
        lines.extend(fields("writes", &cc.file, dim, room, dim));
        f.render_widget(Paragraph::new(lines).block(pane("claude code", area)), area);
    }
}
