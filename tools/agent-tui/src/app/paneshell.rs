//! The shell around a [`Pane`]: its tab bar, the frame between the bars,
//! the bottom bar with the footer, and the keys and clicks the shell takes
//! before the pane gets the rest. The overlays (the key help, the exit
//! guard, the response modal) are the tree shell's own.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use super::pane::{footer_spans, Binding, Keyed, Tone};
use super::render::{modal_rect, pane as bordered};
use super::theme::{self, Ground, Seg};
use super::*;

fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| s.width() as u16).sum()
}

impl App {
    /// The pane's unsaved work, for the exit guard.
    pub(super) fn pane_unsaved(&self) -> Option<String> {
        self.pane.as_ref()?.unsaved()
    }

    fn owns_text(&self) -> bool {
        self.pane.as_ref().is_some_and(|p| p.owns_text())
    }

    /// The shell's own keys around the pane, in the form they take there:
    /// Alt chords beside a pane that owns text.
    pub fn shell_bindings(&self) -> Vec<Binding> {
        let text = self.owns_text();
        let k = |plain: &str, chord: &str| if text { chord.to_string() } else { plain.to_string() };
        vec![
            Binding::new(k("1-9", "M-1-9"), "tabs"),
            Binding::new(k("?", "F1"), "keys"),
            Binding::help(k("m", "M-m"), "mouse on or off (off lets the terminal select text)"),
            Binding::help(k("q Esc ^C", "Esc ^C"), "quit; asks first over unsaved work"),
            Binding::help("click", "a tab shows it; the wheel scrolls"),
        ]
    }

    /// The pane's bindings, then the shell's.
    pub fn bindings(&self) -> Vec<Binding> {
        let own = self.pane.as_ref().map(|p| p.bindings()).unwrap_or_default();
        own.into_iter().chain(self.shell_bindings()).collect()
    }

    /// One frame of a pane screen: the tab bar, the pane, an overlay, the
    /// bottom bar.
    pub(super) fn draw_pane_frame(&mut self, f: &mut Frame) {
        let [bar, main, status] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        self.draw_pane_tabs(f, bar);
        self.hits.pane = main;
        if let Some(p) = &mut self.pane {
            p.draw(f, main);
        }
        match &self.mode {
            Mode::Help { scroll } => {
                let scroll = *scroll;
                self.draw_pane_help(f, main, scroll);
            }
            Mode::Guard { .. } => self.draw_guard(f, main),
            Mode::Response(_) => self.draw_response(f, main),
            _ => {}
        }
        self.draw_status(f, status);
    }

    /// The pane's tabs as the shell draws its own: numbered lozenges, the
    /// shown one on the accent, each after its lead, then the trailer. Each
    /// tab, lead and all, is a click target.
    fn draw_pane_tabs(&mut self, f: &mut Frame, area: Rect) {
        self.hits.tabs.clear();
        let Some(p) = &mut self.pane else { return };
        let tabs = p.tabs();
        let active = p.tab();
        let trailer = p.trailer();
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, t) in tabs.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let x = area.x + width(&spans);
            spans.extend(t.lead);
            let mut seg = crate::strip::tab_seg(format!(" {} {} ", i + 1, t.name), i == active);
            seg.style = crate::strip::target(seg.style, t.target);
            spans.extend(self.shape.lozenge(&[seg]));
            let w = area.x + width(&spans) - x;
            self.hits.tabs.push((Rect { x, y: area.y, width: w, height: 1 }.intersection(area), i));
        }
        spans.extend(trailer);
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// The bottom bar over a pane: its lozenge, the message, then the mouse
    /// and as many of the bindings as fit after it. The message comes
    /// first, so the shell's own parts never cut it short.
    pub(super) fn pane_status(&mut self, area: Rect) -> Vec<Span<'static>> {
        let mouse = format!("mouse {} ({})", if self.mouse { "on" } else { "off" }, if self.owns_text() { "M-m" } else { "m" });
        let footer: Vec<Binding> = self.bindings().into_iter().filter(|b| b.footer).collect();
        let Some(p) = &mut self.pane else { return Vec::new() };
        let mut spans = self.shape.lozenge(&[Seg::on(format!(" {} ", p.mode()), Ground::Accent).bold()]);
        // What the shell said last (the mouse toggled) until the next key
        // reaches the pane; else the pane's own.
        let said = if self.msg.is_empty() { p.status() } else { Some((self.msg.clone(), Tone::Said)) };
        let said = said.filter(|(t, _)| !t.is_empty());
        if let Some((text, tone)) = &said {
            let style = match tone {
                Tone::Err => theme::err(),
                Tone::Said => theme::body(),
                Tone::Back => theme::muted(),
            };
            spans.push(Span::raw(" "));
            spans.push(Span::styled(text.clone(), style));
        }
        // The mouse, then the keys, each whole, while they fit.
        let lead = if said.is_some() { theme::sep() } else { Span::raw(" ") };
        let mouse = Span::styled(mouse, theme::hint());
        if width(&spans) + width(&[lead.clone(), mouse.clone()]) > area.width {
            return spans;
        }
        spans.extend([lead, mouse]);
        let mut shown = 0;
        for n in 1..=footer.len() {
            let keys = footer_spans(footer[..n].iter().map(|b| (b.keys.as_str(), b.label.as_str())));
            if width(&spans) + width(&[theme::sep()]) + width(&keys) > area.width {
                break;
            }
            shown = n;
        }
        if shown > 0 {
            spans.push(theme::sep());
            spans.extend(footer_spans(footer[..shown].iter().map(|b| (b.keys.as_str(), b.label.as_str()))));
        }
        spans
    }

    /// The key help over a pane: every binding, then the pane's own help.
    fn draw_pane_help(&mut self, f: &mut Frame, area: Rect, scroll: u16) {
        let bindings = self.bindings();
        let col = bindings.iter().map(|b| b.keys.chars().count()).max().unwrap_or(0) + 2;
        let mut lines: Vec<Line> =
            bindings.iter().map(|b| Line::from(vec![Span::styled(format!("{:<col$}", b.keys), theme::accent()), Span::raw(b.label.clone())])).collect();
        if let Some(text) = self.pane.as_ref().and_then(|p| p.help()) {
            lines.push(Line::raw(""));
            lines.push(Line::styled(format!("help: {}", self.title), Style::new().add_modifier(Modifier::BOLD)));
            lines.extend(text.lines().map(|l| Line::raw(l.to_string())));
        }
        let widest = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let r = modal_rect(area, widest.max(56) + 2, lines.len() as u16 + 2);
        let room = r.height.saturating_sub(2);
        let scroll = scroll.min((lines.len() as u16).saturating_sub(room));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(bordered("keys · ↑↓ scroll · any other key closes").border_style(theme::modal_border())), r);
    }

    /// A key while a pane is shown. The shell takes its own first (a tab,
    /// the mouse, the key help); the pane gets the rest; what the pane
    /// passes on and quits, quits.
    pub(super) fn pane_key(&mut self, k: KeyEvent) -> bool {
        let text = self.owns_text();
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        // The shell's letter and digit keys: Alt chords beside text.
        let shell = if text { k.modifiers == KeyModifiers::ALT } else { plain };
        match k.code {
            KeyCode::F(1) => {
                self.mode = Mode::Help { scroll: 0 };
                return true;
            }
            KeyCode::Char('?') if !text && plain => {
                self.mode = Mode::Help { scroll: 0 };
                return true;
            }
            KeyCode::Char('q') if !text && plain => return self.quit(),
            KeyCode::Char('m') if shell => {
                self.toggle_mouse();
                // The bar names the state already; only what off means is said.
                if self.mouse {
                    self.msg.clear();
                } else {
                    self.msg = "the terminal selects text".into();
                }
                return true;
            }
            KeyCode::Char(c @ '1'..='9') if shell => {
                self.pane_tab(c as usize - '1' as usize);
                return true;
            }
            _ => {}
        }
        self.msg.clear();
        let Some(p) = &mut self.pane else { return true };
        match p.key(k) {
            Keyed::Done => true,
            Keyed::Pass if k.code == KeyCode::Esc => self.quit(),
            Keyed::Pass => true,
        }
    }

    /// Show the pane's tab `i`, when it has one.
    fn pane_tab(&mut self, i: usize) {
        let Some(p) = &mut self.pane else { return };
        if i < p.tabs().len() {
            p.set_tab(i);
        }
    }

    /// The mouse while a pane is shown: a click on a tab shows it; the
    /// wheel and a click inside the pane are the pane's.
    pub(super) fn pane_mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if self.hits.pane.contains(at) => {
                if let Some(p) = &mut self.pane {
                    p.wheel(m.kind == MouseEventKind::ScrollUp);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, i)) = self.hits.tabs.iter().find(|(r, _)| r.contains(at)) {
                    return self.pane_tab(i);
                }
                if self.hits.pane.contains(at) {
                    if let Some(p) = &mut self.pane {
                        p.click(at);
                    }
                }
            }
            _ => {}
        }
    }

    /// How often the pane wants its tick.
    pub fn pane_tick_every(&self) -> Option<std::time::Duration> {
        self.pane.as_ref().and_then(|p| p.tick_every())
    }

    /// The palette a pane screen draws in.
    pub(super) fn pane_palette(&self) -> Option<theme::Palette> {
        self.pane.as_ref().map(|p| p.palette())
    }
}

#[cfg(test)]
mod tests {
    use super::super::pane::{Pane, PaneTab};
    use super::*;
    use crate::testkit;

    /// A pane of two tabs: it counts its keys and the wheel, and holds
    /// unsaved work while `dirty` is set.
    struct Two {
        tab: usize,
        keys: usize,
        wheel: i32,
        dirty: bool,
        text: bool,
    }

    impl Pane for Two {
        fn palette(&self) -> theme::Palette {
            theme::Palette::default()
        }
        fn tabs(&mut self) -> Vec<PaneTab> {
            vec![PaneTab::new("one"), PaneTab::new("two")]
        }
        fn tab(&mut self) -> usize {
            self.tab
        }
        fn set_tab(&mut self, i: usize) {
            self.tab = i;
        }
        fn draw(&mut self, f: &mut Frame, area: Rect) {
            f.render_widget(Paragraph::new(format!("tab {} keys {}", self.tab, self.keys)), area);
        }
        fn key(&mut self, k: KeyEvent) -> Keyed {
            if k.code == KeyCode::Esc {
                return Keyed::Pass;
            }
            self.keys += 1;
            Keyed::Done
        }
        fn wheel(&mut self, up: bool) {
            self.wheel += if up { 1 } else { -1 };
        }
        fn bindings(&self) -> Vec<Binding> {
            vec![Binding::new("x", "count")]
        }
        fn owns_text(&self) -> bool {
            self.text
        }
        fn unsaved(&self) -> Option<String> {
            self.dirty.then(|| "a count".into())
        }
        fn discard(&mut self) {
            self.dirty = false;
        }
    }

    fn app(text: bool) -> App {
        App::with_pane("two", Two { tab: 0, keys: 0, wheel: 0, dirty: false, text })
    }

    fn two(a: &App) -> &Two {
        a.pane_ref().expect("the pane")
    }

    fn k(c: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(c, m)
    }

    #[test]
    fn the_shell_takes_plain_keys_beside_a_pane_without_text() {
        let mut a = app(false);
        assert!(a.key(k(KeyCode::Char('2'), KeyModifiers::NONE)));
        assert_eq!(two(&a).tab, 1);
        assert!(a.key(k(KeyCode::Char('9'), KeyModifiers::NONE)), "no ninth tab: nothing");
        assert_eq!(two(&a).tab, 1);
        a.key(k(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(!a.mouse_on());
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(two(&a).keys, 1, "the rest is the pane's");
        assert!(!a.key(k(KeyCode::Char('q'), KeyModifiers::NONE)), "q quits with nothing unsaved");
    }

    #[test]
    fn beside_text_the_shell_keys_are_alt_chords() {
        let mut a = app(true);
        a.key(k(KeyCode::Char('2'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('m'), KeyModifiers::NONE));
        a.key(k(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!((two(&a).tab, two(&a).keys), (0, 3), "plain keys are text");
        assert!(a.mouse_on());
        a.key(k(KeyCode::Char('2'), KeyModifiers::ALT));
        a.key(k(KeyCode::Char('m'), KeyModifiers::ALT));
        assert_eq!(two(&a).tab, 1);
        assert!(!a.mouse_on());
        let footer = testkit::rows(&testkit::render(&mut a, 100, 6)).pop().expect("a bar");
        assert!(footer.contains("mouse off (M-m)") && footer.contains("M-1-9 tabs") && footer.contains("F1 keys"), "{footer}");
    }

    #[test]
    fn quitting_over_unsaved_work_asks_first() {
        let mut a = app(false);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        assert!(a.key(k(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(a.guarding());
        let text = testkit::text(&testkit::render(&mut a, 80, 16));
        assert!(text.contains("1 unsaved") && text.contains("a count") && !text.contains("Review"), "{text}");
        a.key(k(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(!a.guarding(), "r has nothing to review: back to the pane");
        a.key(k(KeyCode::Char('c'), KeyModifiers::CONTROL));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(!a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)));
        assert!(!two(&a).dirty, "the work was dropped");
    }

    #[test]
    fn a_click_on_a_tab_and_the_wheel_over_the_pane() {
        let mut a = app(false);
        let buf = testkit::render(&mut a, 60, 8);
        let (x, y) = testkit::find(&buf, "2 two").expect("the tab");
        let ev = |kind, column, row| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
        a.mouse(ev(MouseEventKind::Down(MouseButton::Left), x, y));
        assert_eq!(two(&a).tab, 1);
        a.mouse(ev(MouseEventKind::ScrollUp, 3, 3));
        a.mouse(ev(MouseEventKind::ScrollUp, 3, 7));
        assert_eq!(two(&a).wheel, 1, "the bottom bar is not the pane");
    }

    #[test]
    fn the_key_help_lists_every_binding() {
        let mut a = app(false);
        a.key(k(KeyCode::Char('?'), KeyModifiers::NONE));
        let text = testkit::text(&testkit::render(&mut a, 90, 20));
        for b in a.bindings() {
            assert!(text.contains(&b.keys) && text.contains(&b.label), "{}: {text}", b.keys);
        }
        assert_eq!(crate::binding_conflicts(&a.bindings()), Vec::<String>::new());
    }
}
