//! The tab bar over a pane: drawing it, and the keys and clicks that choose
//! a tab or open its menu.
//!
//! - Ctrl+1-9 shows a tab; on the tab already shown it opens the tab's menu.
//! - F2 or Ctrl+T gives the bar the focus: Left and Right move along it,
//!   showing each tab they land on, Enter opens the menu, Esc (or F2 or
//!   Ctrl+T again) gives the focus back. Any other key gives it back and
//!   goes where it would have gone. The focused tab is drawn in reverse
//!   video behind a `▸`, and the bottom bar's lozenge reads `TABS`.
//! - Ctrl+N, where the pane names an action for it
//!   ([`crate::Pane::new_item_label`]), runs it from the compose box or
//!   from the tab bar.
//! - A click on a tab shows it; a click on the tab already shown, or a right
//!   click, opens its menu.
//! - An action slot ([`PaneTab::action`], such as `≡` or `+`) has no number
//!   and is never shown as a tab: choosing it opens its menu. Numbers count
//!   the tabs alone, so a slot before the first tab leaves it tab 1.
//!
//! Alt+1-9 still shows a tab where the terminal passes Alt+digits on;
//! Konsole and GNOME Terminal keep them for their own tabs.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::*;

/// Which modifier with a digit jumps to a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    /// Ctrl where the terminal reports Ctrl+digits, Alt where it does not.
    Auto,
    Ctrl,
    Alt,
    Both,
    None,
}

impl Jump {
    /// The setting's spelling: `auto`, `ctrl`, `alt`, `both` or `none`.
    pub fn parse(s: &str) -> Option<Jump> {
        Some(match s {
            "auto" => Jump::Auto,
            "ctrl" => Jump::Ctrl,
            "alt" => Jump::Alt,
            "both" => Jump::Both,
            "none" => Jump::None,
            _ => return None,
        })
    }
}

/// How a pane's tabs are reached from the keyboard: the pane says, from
/// its settings ([`crate::Pane::tab_keys`]), and the shell asks each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabKeys {
    pub jump: Jump,
    /// A jump to the tab already shown, or a click on it, opens its menu.
    pub menu_on_repeat: bool,
    /// F2 gives the tab bar the focus.
    pub f2: bool,
    /// Ctrl+T gives the tab bar the focus.
    pub ctrl_t: bool,
}

impl Default for TabKeys {
    fn default() -> TabKeys {
        TabKeys { jump: Jump::Auto, menu_on_repeat: true, f2: true, ctrl_t: true }
    }
}

impl TabKeys {
    /// Whether Ctrl+digits jump, given whether the terminal reports them.
    pub fn ctrl(&self, enhanced: bool) -> bool {
        match self.jump {
            Jump::Auto => enhanced,
            Jump::Ctrl | Jump::Both => true,
            Jump::Alt | Jump::None => false,
        }
    }

    /// Whether Alt+digits jump.
    pub fn alt(&self, enhanced: bool) -> bool {
        match self.jump {
            Jump::Auto => !enhanced,
            Jump::Alt | Jump::Both => true,
            Jump::Ctrl | Jump::None => false,
        }
    }

    /// The focus key as the footer names it, if any.
    pub fn focus_label(&self) -> Option<&'static str> {
        match (self.f2, self.ctrl_t) {
            (true, _) => Some("F2"),
            (false, true) => Some("Ctrl+T"),
            (false, false) => None,
        }
    }
}

/// What the bottom bar says while the tab bar has the focus, after its
/// `TABS` lozenge; `new` is the pane's Ctrl+N action, if it has one.
fn strip_hint(new: Option<&str>) -> String {
    match new {
        Some(n) => format!("← → move · Enter menu · Ctrl+N {n} · Esc back"),
        None => "← → move · Enter menu · Esc back".into(),
    }
}

fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| s.width() as u16).sum()
}

impl App {
    /// Whether the terminal took the keyboard enhancement, as the run loop
    /// found when it started; the pane is told too. A headless run, which
    /// has no terminal, leaves it off.
    pub fn set_keyboard_enhanced(&mut self, on: bool) {
        self.enhanced = on;
        if let Some(p) = &mut self.pane {
            p.set_keyboard_enhanced(on);
        }
    }

    /// The tab keys in effect: the pane's.
    pub fn tab_keys(&self) -> TabKeys {
        self.pane.as_ref().map(|p| p.tab_keys()).unwrap_or_default()
    }

    /// Whether the terminal reports Ctrl+digits as themselves.
    pub fn keyboard_enhanced(&self) -> bool {
        self.enhanced
    }

    /// Whether Ctrl+digits jump to a tab here.
    pub(super) fn ctrl_jump(&self) -> bool {
        self.tab_keys().ctrl(self.enhanced)
    }

    /// Whether the footer names Ctrl+digits: only where they arrive as
    /// themselves. A setting that asks for them in a terminal that folds
    /// them into other keys leaves the footer to the keys that work.
    pub(super) fn ctrl_shown(&self) -> bool {
        self.ctrl_jump() && self.enhanced
    }

    /// Whether the pane's tabs have menus.
    pub(super) fn has_tab_menus(&self) -> bool {
        self.pane.as_ref().is_some_and(|p| p.has_tab_menus())
    }

    /// The pane's Ctrl+N action, when its tabs have menus and it names one.
    pub(super) fn new_item_label(&self) -> Option<&'static str> {
        self.pane.as_ref().filter(|p| p.has_tab_menus()).and_then(|p| p.new_item_label())
    }

    /// Tell the pane the shell took a key or a click before it.
    pub(super) fn interrupt_pane(&mut self) {
        if let Some(p) = &mut self.pane {
            p.interrupted();
        }
    }

    /// Whether Alt+digits jump to a tab here.
    pub(super) fn alt_jump(&self) -> bool {
        self.tab_keys().alt(self.enhanced)
    }

    /// Whether the tab bar has the focus.
    pub fn strip_focused(&self) -> bool {
        self.strip.is_some()
    }

    /// The tab bar's keys, taken before the pane's: true when taken.
    pub(super) fn pane_tab_key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers == KeyModifiers::CONTROL;
        let keys = self.tab_keys();
        let menus = self.has_tab_menus();
        let toggle = menus && ((keys.f2 && k.code == KeyCode::F(2)) || (keys.ctrl_t && ctrl && k.code == KeyCode::Char('t')));
        if ctrl && k.code == KeyCode::Char('n') && self.new_item_label().is_some() {
            self.strip_blur();
            self.interrupt_pane();
            if let Some(p) = &mut self.pane {
                p.new_item();
            }
            self.take_pane_open();
            return true;
        }
        if let Some(at) = self.strip {
            match k.code {
                KeyCode::Left => self.strip_move(at, false),
                KeyCode::Right => self.strip_move(at, true),
                KeyCode::Enter => {
                    self.strip_blur();
                    self.open_tab_menu(at);
                }
                KeyCode::Esc => self.strip_blur(),
                _ if toggle => self.strip_blur(),
                _ => {
                    self.strip_blur();
                    return false;
                }
            }
            return true;
        }
        if toggle {
            self.interrupt_pane();
            if let Some(p) = &mut self.pane {
                self.strip = Some(p.tab());
                self.msg = strip_hint(p.new_item_label());
            }
            return true;
        }
        match k.code {
            KeyCode::Char(c @ '1'..='9') if ctrl && self.ctrl_jump() => {
                if let Some(i) = self.numbered(c as usize - '0' as usize) {
                    self.pane_tab_or_menu(i);
                }
                true
            }
            _ => false,
        }
    }

    fn strip_blur(&mut self) {
        self.strip = None;
        self.msg.clear();
    }

    /// Move the bar's cursor one tab, wrapping, and show the tab it lands
    /// on; an action slot is only pointed at.
    fn strip_move(&mut self, at: usize, forward: bool) {
        let Some(p) = &mut self.pane else { return };
        let tabs = p.tabs();
        let n = tabs.len();
        if n == 0 {
            return;
        }
        let to = if forward { (at + 1) % n } else { (at + n - 1) % n };
        if !tabs[to].action {
            p.set_tab(to);
        }
        self.strip = Some(to);
    }

    /// The index of tab number `n` (from 1): action slots carry no number.
    pub(super) fn numbered(&mut self, n: usize) -> Option<usize> {
        let p = self.pane.as_mut()?;
        p.tabs().iter().enumerate().filter(|(_, t)| !t.action).nth(n.checked_sub(1)?).map(|(i, _)| i)
    }

    /// Show the pane's tab `i`, when it has one and it is a tab.
    pub(super) fn pane_tab(&mut self, i: usize) {
        let Some(p) = &mut self.pane else { return };
        if p.tabs().get(i).is_some_and(|t| !t.action) {
            p.set_tab(i);
        }
    }

    /// Ctrl+digit or a click on tab `i`: show it, or open its menu when it
    /// is shown already or is an action slot.
    pub(super) fn pane_tab_or_menu(&mut self, i: usize) {
        let Some(p) = &mut self.pane else { return };
        let Some(t) = p.tabs().get(i).cloned() else { return };
        let menus = p.has_tab_menus();
        if menus && (t.action || (p.tab() == i && p.tab_keys().menu_on_repeat)) {
            self.open_tab_menu(i);
        } else if p.tab() != i {
            p.set_tab(i);
        }
    }

    /// Open tab `i`'s menu, over the pane.
    pub(super) fn open_tab_menu(&mut self, i: usize) {
        if let Some(p) = &mut self.pane {
            if !p.has_tab_menus() {
                return;
            }
            p.tab_menu(i);
        }
        self.take_pane_open();
    }

    /// The pane's tabs as the shell draws its own: numbered lozenges, the
    /// shown one on the accent, each after its lead, then the trailer. Each
    /// tab, lead and all, is a click target. The tab under the bar's cursor
    /// is drawn in reverse video behind a `▸`; an action slot carries no number.
    pub(super) fn draw_pane_tabs(&mut self, f: &mut Frame, area: Rect) {
        self.hits.tabs.clear();
        let cursor = self.strip;
        let Some(p) = &mut self.pane else { return };
        let tabs = p.tabs();
        let active = p.tab();
        let trailer = p.trailer();
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut number = 0;
        for (i, t) in tabs.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let x = area.x + width(&spans);
            spans.extend(t.lead);
            if !t.action {
                number += 1;
            }
            let label = if t.action { format!(" {} ", t.name) } else { format!(" {number} {} ", t.name) };
            let at_cursor = cursor == Some(i);
            // The marker takes the label's leading space, so the bar does
            // not shift when the focus moves.
            let label = if at_cursor { format!("▸{}", label.strip_prefix(' ').unwrap_or(&label)) } else { label };
            let mut seg = crate::strip::tab_seg(label, i == active && !t.action);
            seg.style = crate::strip::target(seg.style, t.target);
            if at_cursor {
                seg.style = seg.style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            spans.extend(self.shape.lozenge(&[seg]));
            let w = area.x + width(&spans) - x;
            self.hits.tabs.push((Rect { x, y: area.y, width: w, height: 1 }.intersection(area), i));
        }
        spans.extend(trailer);
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// The key help's line on the keyboard: whether Ctrl+digits arrive.
    pub(super) fn keyboard_line(&self) -> Option<String> {
        let p = self.pane.as_ref()?;
        if !p.keyboard_enhancement() {
            return None;
        }
        Some(if self.enhanced {
            "keyboard: Ctrl+digits arrive (the terminal speaks the kitty keyboard protocol)".into()
        } else {
            "keyboard: this terminal does not report Ctrl+digits (no kitty keyboard protocol); use F2, Ctrl+T or Alt+1-9".into()
        })
    }
}
