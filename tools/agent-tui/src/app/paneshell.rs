//! The shell around a [`Pane`]: its tab bar, the frame between the bars,
//! the bottom bar with the footer, and the keys and clicks the shell takes
//! before the pane gets the rest. The overlays (the key help, the exit
//! guard, the response modal, the picker) are the tree shell's own; a pane
//! opens the modal and the picker through [`Pane::take_open`].

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use super::pane::{footer_spans, Binding, Keyed, Open, Tone};
use super::pick::Pick;
use super::render::{modal_rect, pane as bordered};
use super::response::Shown;
use super::theme::{self, Seg};
use super::*;
use crate::adapter::Printed;
use crate::wrap::str_width;

fn width(spans: &[Span]) -> u16 {
    spans.iter().map(|s| s.width() as u16).sum()
}

/// `text` cut to `room` columns, ending in `…` when it was cut.
fn clip(text: &str, room: usize) -> String {
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

impl App {
    /// The pane's unsaved work, for the exit guard.
    pub(super) fn pane_unsaved(&self) -> Option<String> {
        self.pane.as_ref()?.unsaved()
    }

    pub(super) fn owns_text(&self) -> bool {
        self.pane.as_ref().is_some_and(|p| p.owns_text())
    }

    /// The shell's own keys around the pane, in the form they take there:
    /// Alt chords beside a pane that owns text. The key help comes first,
    /// so the footer always has room for the key that finds the rest.
    pub fn shell_bindings(&self) -> Vec<Binding> {
        let text = self.owns_text();
        let k = |plain: &str, chord: &str| if text { chord.to_string() } else { plain.to_string() };
        vec![
            Binding::new(k("?", "F1"), "keys"),
            Binding::new(k("1-9", "M-1-9"), "tabs"),
            Binding::help(self.mouse_key(), "mouse on or off (off lets the terminal select text)"),
            Binding::help("Shift-drag", "selects text while the mouse is on, in most terminals"),
            Binding::help("middle-click", "pastes only while the mouse is off"),
            Binding::help(k("q Esc ^C", "Esc ^C"), "quit; asks first over unsaved work"),
            Binding::help("click", "a tab shows it; the wheel scrolls"),
        ]
    }

    /// The key that turns the mouse on or off as the bar names it: `m`, or
    /// `M-m` beside text.
    pub(super) fn mouse_key(&self) -> &'static str {
        if self.owns_text() {
            "M-m"
        } else {
            "m"
        }
    }

    /// The key help, then the pane's bindings, then the rest of the shell's.
    pub fn bindings(&self) -> Vec<Binding> {
        let mut shell = self.shell_bindings().into_iter();
        let own = self.pane.as_ref().map(|p| p.bindings()).unwrap_or_default();
        shell.next().into_iter().chain(own).chain(shell).collect()
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
            Mode::Pick(p) => {
                let p = p.clone();
                self.draw_pick(f, main, &p);
            }
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
    /// first, so the shell's own parts never cut it short: the lozenge goes
    /// when the message needs its room, and a message longer than the bar
    /// ends in `…`.
    pub(super) fn pane_status(&mut self, area: Rect) -> Vec<Span<'static>> {
        let key = self.mouse_key();
        // As the settings tree names it; Shift-drag is in the key help.
        let mouse = if self.mouse { format!("mouse on ({key})") } else { format!("mouse off ({key})") };
        let footer: Vec<Binding> = self.bindings().into_iter().filter(|b| b.footer).collect();
        let Some(p) = &mut self.pane else { return Vec::new() };
        let lozenge = self.shape.lozenge(&[Seg::on(format!(" {} ", p.mode()), p.mode_ground()).bold()]);
        // What the shell said last (the mouse toggled) until the next key
        // reaches the pane; else the pane's own.
        let said = if self.msg.is_empty() { p.status() } else { Some((self.msg.clone(), Tone::Said)) };
        let said = said.filter(|(t, _)| !t.is_empty());
        let mut spans = Vec::new();
        match &said {
            Some((text, tone)) => {
                let style = match tone {
                    Tone::Err => theme::err(),
                    Tone::Said => theme::body(),
                    Tone::Back => theme::muted(),
                };
                if width(&lozenge) as usize + 1 + str_width(text) <= area.width as usize {
                    spans.extend(lozenge);
                }
                let room = (area.width as usize).saturating_sub(width(&spans) as usize + 1);
                spans.push(Span::raw(" "));
                spans.push(Span::styled(clip(text, room), style));
            }
            None => spans.extend(lozenge),
        }
        // The mouse, then the keys, each whole, while they fit. The key
        // help comes first among the keys, and takes the mouse's place when
        // only one of them fits.
        let mouse = Span::styled(mouse, theme::hint());
        let keys = |n: usize| footer_spans(footer[..n].iter().map(|b| (b.keys.as_str(), b.label.as_str())));
        let sep = |first: bool| if first && said.is_none() { Span::raw(" ") } else { theme::sep() };
        let first_keys = if footer.is_empty() { 0 } else { 1 };
        let with_mouse = width(&spans) + width(&[sep(true), mouse.clone(), theme::sep()]) + width(&keys(first_keys)) <= area.width;
        let mut first = true;
        if with_mouse || (footer.is_empty() && width(&spans) + width(&[sep(true), mouse.clone()]) <= area.width) {
            spans.extend([sep(true), mouse]);
            first = false;
        }
        let mut shown = 0;
        for n in 1..=footer.len() {
            if width(&spans) + width(&[sep(first)]) + width(&keys(n)) > area.width {
                break;
            }
            shown = n;
        }
        if shown > 0 {
            spans.push(sep(first));
            spans.extend(keys(shown));
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
        // The shell's letter and digit keys: plain, or the Alt chord, which
        // is the only form beside text.
        let shell = k.modifiers == KeyModifiers::ALT || (!text && plain);
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
        let keyed = p.key(k);
        self.take_pane_open();
        match keyed {
            Keyed::Done => true,
            Keyed::Pass if k.code == KeyCode::Esc => self.quit(),
            Keyed::Pass => true,
        }
    }

    /// The exit guard beside a pane that owns text, where keystrokes are in
    /// flight when it opens. `D` arms the quit and Enter (or ^C again)
    /// confirms it; no letter does, since letters are typing. Unarmed, Esc
    /// and Enter go back to the work, and a character or an editing key
    /// closes the guard and acts on the work. Armed, Esc disarms it, and a
    /// character or an editing key types the arming `D` first, then acts:
    /// typing on past an Esc reaches the draft as typed.
    pub(super) fn guard_beside_text(&mut self, k: KeyEvent, confirm: bool) -> bool {
        let plain = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let edits = plain
            && matches!(
                k.code,
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
            );
        match (confirm, k.code) {
            (true, KeyCode::Enter) => {
                self.discard_all();
                return false;
            }
            (true, KeyCode::Esc) => self.mode = Mode::Guard { confirm: false },
            (true, _) if edits => {
                // Back to typing: the D that armed the quit was typing too.
                self.pane_key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE));
                return self.pane_key(k);
            }
            (true, _) => self.mode = Mode::Guard { confirm: true },
            (false, KeyCode::Char('D')) if plain => self.mode = Mode::Guard { confirm: true },
            (false, KeyCode::Esc | KeyCode::Enter) => {}
            (false, _) if edits => return self.pane_key(k),
            (false, _) => self.mode = Mode::Guard { confirm: false },
        }
        true
    }

    /// Open what the pane asked for: a report or an error in the response
    /// modal, or a picker whose choice goes back to the pane.
    pub(super) fn take_pane_open(&mut self) {
        let Some(open) = self.pane.as_mut().and_then(|p| p.take_open()) else { return };
        match open {
            Open::Report { label, command, text } => {
                let printed = Printed { code: Some(0), stdout: text, stderr: String::new() };
                if let Some(s) = Shown::of(&label, &command, tree::Response::Report, &Ok(()), Some(printed)) {
                    self.show_response(s);
                }
            }
            Open::Error { label, command, text } => {
                if let Some(s) = Shown::of(&label, &command, tree::Response::Write, &Err(text), None) {
                    self.show_response(s);
                }
            }
            Open::Pick { id, title, options, multi, chosen } if matches!(self.mode, Mode::Browse) && !options.is_empty() => {
                self.mode = Mode::Pick(Pick::for_pane(id, title, options, multi, &chosen));
            }
            Open::Pick { .. } => {}
        }
    }

    /// The pane's picker set `values`.
    pub(super) fn pane_picked(&mut self, id: &str, values: Vec<String>) {
        if let Some(p) = &mut self.pane {
            p.picked(id, values);
        }
        self.take_pane_open();
    }

    /// Show the pane's tab `i`, when it has one.
    fn pane_tab(&mut self, i: usize) {
        let Some(p) = &mut self.pane else { return };
        if i < p.tabs().len() {
            p.set_tab(i);
        }
    }

    /// The mouse while a pane is shown: a click on a tab shows it; the
    /// wheel and a click inside the pane are the pane's. A middle click
    /// pastes nothing while the shell has the mouse, and the bar says so.
    pub(super) fn pane_mouse(&mut self, m: MouseEvent) {
        let at = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if self.hits.pane.contains(at) => {
                if let Some(p) = &mut self.pane {
                    p.wheel_at(m.kind == MouseEventKind::ScrollUp, at);
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
            MouseEventKind::Down(MouseButton::Middle) => {
                let key = self.mouse_key();
                self.msg = format!("middle-click pastes with the mouse off: {key}, paste, {key}");
                return;
            }
            _ => return,
        }
        self.take_pane_open();
    }

    /// One tick of the pane, on its own schedule.
    pub fn tick_pane(&mut self) {
        if let Some(p) = &mut self.pane {
            p.tick();
        }
        self.take_pane_open();
        self.open_held();
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

/// The shell as a [`crate::screen::Screen`], so a headless run and the
/// tests drive it as they drive any screen: its keys, its mouse, the
/// pane's tick and a running apply's. On the terminal it runs through
/// [`crate::run`], which also reports the mouse.
impl crate::screen::Screen for App {
    fn palette(&self) -> theme::Palette {
        self.pane_palette().unwrap_or_else(|| self.themes.palette(self.shown_theme()))
    }

    fn draw(&mut self, f: &mut Frame) {
        App::draw(self, f);
    }

    fn key(&mut self, k: KeyEvent) -> bool {
        App::key(self, k)
    }

    fn mouse(&mut self, m: MouseEvent) {
        App::mouse(self, m);
    }

    fn tick_every(&self) -> Option<std::time::Duration> {
        self.pane_tick_every()
    }

    fn tick(&mut self) {
        self.tick_pane();
        App::tick(self);
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
        /// What the next key opens, and what a picker set.
        open: Option<Open>,
        picked: Vec<String>,
    }

    impl Pane for Two {
        fn take_open(&mut self) -> Option<Open> {
            self.open.take()
        }
        fn picked(&mut self, id: &str, values: Vec<String>) {
            self.picked = values.into_iter().map(|v| format!("{id}={v}")).collect();
        }
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
        App::with_pane("two", Two { tab: 0, keys: 0, wheel: 0, dirty: false, text, open: None, picked: Vec::new() })
    }

    #[test]
    fn a_pane_opens_a_report_in_the_response_modal() {
        let mut a = app(false);
        a.pane_mut::<Two>().expect("the pane").open =
            Some(Open::Report { label: "peers".into(), command: "attend peers".into(), text: "@one\n@two".into() });
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        let text = testkit::text(&testkit::render(&mut a, 80, 16));
        assert!(text.contains("attend peers") && text.contains("@two"), "{text}");
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!testkit::text(&testkit::render(&mut a, 80, 16)).contains("@two"), "Esc closes it, not the screen");
    }

    #[test]
    fn a_pane_opens_a_picker_and_hears_the_choice() {
        let mut a = app(false);
        let open = Open::Pick { id: "to".into(), title: "send to".into(), options: vec!["a".into(), "b".into()], multi: false, chosen: vec!["a".into()] };
        a.pane_mut::<Two>().expect("the pane").open = Some(open);
        a.key(k(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(testkit::text(&testkit::render(&mut a, 80, 16)).contains("pick one: send to"));
        a.key(k(KeyCode::Down, KeyModifiers::NONE));
        a.key(k(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(two(&a).picked, ["to=b"]);
        assert_eq!(two(&a).keys, 1, "the picker's keys are not the pane's");
    }

    /// Armed, any key but y disarms the quit: D has to be followed by y.
    #[test]
    fn the_armed_quit_takes_only_y() {
        let mut a = app(false);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(a.key(k(KeyCode::Char('i'), KeyModifiers::NONE)));
        assert!(a.guarding() && a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)), "y alone does not quit");
        assert!(two(&a).dirty);
    }

    /// Beside text, the guard answers only its own keys; the rest is typed.
    #[test]
    fn beside_text_the_guard_lets_typing_through() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.guarding());
        for c in "Did you".chars() {
            assert!(a.key(k(KeyCode::Char(c), KeyModifiers::NONE)), "{c} keeps the screen open");
        }
        assert!(!a.guarding());
        assert_eq!(two(&a).keys, 7, "the arming D reached the pane too, before the i");
        assert!(two(&a).dirty);
        // No letter confirms beside text: D then y types "Dy".
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(a.key(k(KeyCode::Char('y'), KeyModifiers::NONE)));
        assert_eq!(two(&a).keys, 9);
        // Esc disarms, typing nothing; D then Enter quits, as does ^C armed.
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.guarding() && two(&a).keys == 9);
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(!a.key(k(KeyCode::Char('c'), KeyModifiers::CONTROL)), "^C again while armed quits");
        assert!(!two(&a).dirty);
    }

    /// Unarmed, an editing key closes the guard and acts on the work.
    #[test]
    fn beside_text_an_editing_key_closes_the_guard() {
        let mut a = app(true);
        a.pane_mut::<Two>().expect("the pane").dirty = true;
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        assert!(a.key(k(KeyCode::Backspace, KeyModifiers::NONE)));
        assert!(!a.guarding());
        assert_eq!(two(&a).keys, 1);
        a.key(k(KeyCode::Esc, KeyModifiers::NONE));
        a.key(k(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(!a.key(k(KeyCode::Enter, KeyModifiers::NONE)), "D then Enter quits");
    }

    #[test]
    fn a_pane_chooses_whether_the_mouse_starts_on() {
        assert!(app(false).mouse_on(), "the settings default");
        struct Off;
        impl Pane for Off {
            fn palette(&self) -> theme::Palette {
                theme::Palette::default()
            }
            fn tabs(&mut self) -> Vec<PaneTab> {
                Vec::new()
            }
            fn tab(&mut self) -> usize {
                0
            }
            fn set_tab(&mut self, _: usize) {}
            fn draw(&mut self, _: &mut Frame, _: Rect) {}
            fn key(&mut self, _: KeyEvent) -> Keyed {
                Keyed::Done
            }
            fn bindings(&self) -> Vec<Binding> {
                Vec::new()
            }
            fn mouse_default(&self) -> bool {
                false
            }
        }
        assert!(!App::with_pane("off", Off).mouse_on());
    }

    #[test]
    fn a_long_message_drops_the_lozenge_and_ends_in_an_ellipsis() {
        let mut a = app(false);
        a.msg = "x".repeat(70);
        let bar = testkit::rows(&testkit::render(&mut a, 76, 6)).pop().expect("a bar");
        assert!(!bar.contains("browse") && bar.contains(&"x".repeat(70)), "{bar}");
        a.msg = "y".repeat(90);
        let bar = testkit::rows(&testkit::render(&mut a, 76, 6)).pop().expect("a bar");
        assert!(bar.trim_end().ends_with('…'), "{bar}");
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
