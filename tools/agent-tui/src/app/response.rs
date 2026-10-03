//! The response modal (#778): what a reading action's command printed, in
//! a scrollable text area over the screen, closed by a key. One view serves
//! every screen; an action declares what it answers with
//! ([`tree::Response`]), and the outcome decides the verdict. The text is
//! the command's own output, so the screen stays a view over the command.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Padding, Paragraph};
use ratatui::Frame;

use super::render::{modal_rect, pane};
use super::theme::{self, Ground};
use super::{App, Mode};
use crate::adapter::Printed;
use crate::tree;

/// How the modal reads: the verdict its border and title carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Pass,
    Fail,
    Report,
    Error,
}

impl Verdict {
    /// The word on the title and the bottom bar's lozenge.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::Report => "report",
            Verdict::Error => "error",
        }
    }

    pub(crate) fn ground(self) -> Ground {
        match self {
            Verdict::Pass => Ground::Ok,
            Verdict::Fail | Verdict::Error => Ground::Err,
            Verdict::Report => Ground::Accent,
        }
    }

    fn border(self) -> Style {
        match self {
            Verdict::Pass => theme::ok(),
            Verdict::Fail | Verdict::Error => theme::err(),
            Verdict::Report => theme::modal_border(),
        }
    }
}

/// An open response modal.
#[derive(Debug, Clone)]
pub(crate) struct Shown {
    /// The action's label.
    pub(crate) label: String,
    /// The command line as it ran.
    pub(crate) command: String,
    pub(crate) verdict: Verdict,
    /// What the command printed, one entry per line, then how it exited.
    pub(crate) lines: Vec<String>,
    /// The first line in view.
    pub(crate) scroll: u16,
}

/// The lines a page key moves.
const PAGE: u16 = 10;

impl Shown {
    /// The modal for a reading action that ended with `outcome`, or `None`
    /// when the bottom bar is enough: a write that succeeded.
    pub(crate) fn of(label: &str, command: &str, kind: tree::Response, outcome: &Result<(), String>, printed: Option<Printed>) -> Option<Shown> {
        let verdict = match (kind, outcome.is_ok()) {
            (tree::Response::Write, true) => return None,
            (tree::Response::Verify, true) => Verdict::Pass,
            (tree::Response::Verify, false) => Verdict::Fail,
            (tree::Response::Report, true) => Verdict::Report,
            (tree::Response::Write | tree::Response::Report, false) => Verdict::Error,
        };
        let mut lines = Vec::new();
        match &printed {
            // An error leads with stderr, where a command says what went wrong.
            Some(p) => {
                let (first, second) = if verdict == Verdict::Error { (&p.stderr, &p.stdout) } else { (&p.stdout, &p.stderr) };
                for text in [first, second] {
                    lines.extend(plain(text).lines().map(str::to_string));
                }
                while lines.last().is_some_and(|l| l.trim().is_empty()) {
                    lines.pop();
                }
                if lines.is_empty() {
                    lines.push("(it printed nothing)".into());
                }
            }
            // A job that keeps no output: the outcome is all there is.
            None => lines.push(match outcome {
                Ok(()) => "(it printed nothing)".into(),
                Err(e) => plain(e),
            }),
        }
        // How it exited, below the text, except under a report that ended well.
        if verdict != Verdict::Report {
            let exit = match printed.as_ref().map(|p| p.code) {
                Some(Some(c)) => format!("exit {c}"),
                Some(None) => "ended by a signal".into(),
                None if outcome.is_ok() => "exit 0".into(),
                None => String::new(),
            };
            if !exit.is_empty() {
                lines.extend([String::new(), exit]);
            }
        }
        Some(Shown { label: label.to_string(), command: command.to_string(), verdict, lines, scroll: 0 })
    }

    /// One key: scroll, or close. False closes the modal.
    pub(crate) fn key(&mut self, k: KeyEvent) -> bool {
        let last = self.lines.len().saturating_sub(1) as u16;
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => return false,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1).min(last),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(PAGE),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(PAGE).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll = last,
            _ => {}
        }
        true
    }

    fn title(&self) -> String {
        format!("{}: {}", self.label, self.verdict.word())
    }
}

/// Text as the modal draws it: escape sequences and control characters
/// dropped, tabs as spaces. `NO_COLOR` is set for the commands the screens
/// run, but a command may still print an escape.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut escape = false;
    for c in text.chars() {
        if escape {
            // A sequence ends at its first letter, as an SGR's `m` does.
            escape = !c.is_ascii_alphabetic();
        } else if c == '\u{1b}' {
            escape = true;
        } else if c == '\t' {
            out.push_str("    ");
        } else if c == '\n' || !c.is_control() {
            out.push(c);
        }
    }
    out
}

impl App {
    /// Open the modal over the screen, or hold it while something else is
    /// open: [`App::tick`] opens it when the screen is back to browsing.
    pub(super) fn show_response(&mut self, s: Shown) {
        if matches!(self.mode, Mode::Browse) {
            self.mode = Mode::Response(Box::new(s));
        } else {
            self.held = Some(s);
        }
    }

    /// A held modal, opened once the screen is browsing again.
    pub(super) fn open_held(&mut self) {
        if matches!(self.mode, Mode::Browse) {
            if let Some(s) = self.held.take() {
                self.mode = Mode::Response(Box::new(s));
            }
        }
    }

    pub(super) fn response_key(&mut self, mut s: Box<Shown>, k: KeyEvent) {
        if s.key(k) {
            self.mode = Mode::Response(s);
        }
    }

    /// The modal: the command line, then what it printed, wrapped to the
    /// modal and scrolled. The border and title carry the verdict. Its rect
    /// is recorded so a click outside it closes it. Gives the scroll as
    /// drawn, held to the last page.
    pub(super) fn draw_response(&mut self, f: &mut Frame, area: Rect, s: &Shown) -> u16 {
        let mut lines = vec![Line::styled(format!("$ {}", s.command), theme::hint()), Line::raw("")];
        lines.extend(s.lines.iter().map(|l| Line::raw(l.clone())));
        let widest = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        // Borders and a column of padding each side.
        let w = (widest + 4).clamp(64, area.width.saturating_sub(4).max(64));
        let text = crate::wrap::wrap_lines(&lines, w.saturating_sub(4) as usize);
        let r = modal_rect(area, w, text.len() as u16 + 2);
        let text = if r.width == w { text } else { crate::wrap::wrap_lines(&lines, r.width.saturating_sub(4) as usize) };
        let room = r.height.saturating_sub(2);
        let scroll = s.scroll.min((text.len() as u16).saturating_sub(room));
        self.hits.menu = Some((r, Vec::new()));
        f.render_widget(Clear, r);
        let border = s.verdict.border();
        let block = pane(s.title()).border_style(border).title_style(border.add_modifier(Modifier::BOLD)).padding(Padding::horizontal(1));
        f.render_widget(Paragraph::new(text).scroll((scroll, 0)).block(block), r);
        scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Response;
    use ratatui::crossterm::event::KeyModifiers;

    fn printed(code: i32, stdout: &str, stderr: &str) -> Option<Printed> {
        Some(Printed { code: Some(code), stdout: stdout.into(), stderr: stderr.into() })
    }

    fn press(s: &mut Shown, k: KeyCode) -> bool {
        s.key(KeyEvent::new(k, KeyModifiers::NONE))
    }

    #[test]
    fn the_kind_and_the_outcome_decide_the_verdict() {
        let ok: Result<(), String> = Ok(());
        let bad: Result<(), String> = Err("exit 1: no".into());
        let v = |kind, out: &Result<(), String>| Shown::of("x", "ways x", kind, out, None).map(|s| s.verdict);
        assert_eq!(v(Response::Write, &ok), None, "a write that worked leaves the bottom bar to say so");
        assert_eq!(v(Response::Write, &bad), Some(Verdict::Error));
        assert_eq!(v(Response::Verify, &ok), Some(Verdict::Pass));
        assert_eq!(v(Response::Verify, &bad), Some(Verdict::Fail));
        assert_eq!(v(Response::Report, &ok), Some(Verdict::Report));
        assert_eq!(v(Response::Report, &bad), Some(Verdict::Error));
    }

    #[test]
    fn the_text_is_what_the_command_printed_then_how_it_exited() {
        let s = Shown::of("check", "ways agent key check", Response::Verify, &Ok(()), printed(0, "provider: anthropic\nresult: accepted\n", "")).unwrap();
        assert_eq!(s.lines, ["provider: anthropic", "result: accepted", "", "exit 0"]);
        // An error leads with stderr.
        let s = Shown::of("plan", "ways target plan x", Response::Report, &Err("exit 2: bad".into()), printed(2, "partial\n", "bad\n")).unwrap();
        assert_eq!(s.lines, ["bad", "partial", "", "exit 2"]);
        // A report that ended well is its text alone; nothing printed says so.
        let s = Shown::of("plan", "p", Response::Report, &Ok(()), printed(0, "", "")).unwrap();
        assert_eq!(s.lines, ["(it printed nothing)"]);
        // Escapes and control characters never reach the screen.
        let s = Shown::of("c", "c", Response::Report, &Ok(()), printed(0, &format!("{}[31mred{}[0m\tx\u{7}", '\u{1b}', '\u{1b}'), "")).unwrap();
        assert_eq!(s.lines, ["red    x"]);
        // A job that keeps no output shows its outcome.
        let s = Shown::of("c", "c", Response::Verify, &Err("killed".into()), None).unwrap();
        assert_eq!(s.lines, ["killed"]);
    }

    #[test]
    fn arrows_and_pages_scroll_within_the_text_and_esc_enter_q_close() {
        let text: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let mut s = Shown::of("r", "r", Response::Report, &Ok(()), printed(0, &text, "")).unwrap();
        assert!(press(&mut s, KeyCode::Down) && s.scroll == 1);
        assert!(press(&mut s, KeyCode::Char('j')) && s.scroll == 2);
        assert!(press(&mut s, KeyCode::Up) && s.scroll == 1);
        assert!(press(&mut s, KeyCode::PageDown) && s.scroll == 11);
        assert!(press(&mut s, KeyCode::PageUp) && s.scroll == 1);
        assert!(press(&mut s, KeyCode::PageUp) && s.scroll == 0, "no scrolling above the top");
        assert!(press(&mut s, KeyCode::End) && s.scroll == 29);
        assert!(press(&mut s, KeyCode::PageDown) && s.scroll == 29, "no scrolling past the last line");
        assert!(press(&mut s, KeyCode::Home) && s.scroll == 0);
        assert!(press(&mut s, KeyCode::Char('x')), "another key leaves it open");
        for k in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('q')] {
            assert!(!press(&mut s.clone(), k), "{k:?} closes it");
        }
    }

    /// A job that has ended and kept what it printed.
    struct Done(Option<Result<(), String>>, Printed);

    impl crate::adapter::Job for Done {
        fn poll(&mut self) -> Option<Result<(), String>> {
            self.0.take()
        }
        fn printed(&mut self) -> Option<Printed> {
            Some(self.1.clone())
        }
    }

    /// Commands print `report` lines and exit 0, except a line holding
    /// `fail`, which exits 1.
    struct Prints(usize);

    impl crate::Adapter for Prints {
        fn write(&mut self, _: &std::path::Path, _: &[crate::Write]) -> Result<(), String> {
            Ok(())
        }
        fn run(&mut self, _: &tree::Queued) -> Result<(), String> {
            unreachable!("reading actions are started")
        }
        fn start(&mut self, q: &tree::Queued) -> Box<dyn crate::adapter::Job> {
            let stdout: String = (1..=self.0).map(|i| format!("{} line {i}\n", q.command)).collect();
            if q.command.contains("fail") {
                Box::new(Done(Some(Err("exit 1: refused".into())), Printed { code: Some(1), stdout, stderr: "refused\n".into() }))
            } else {
                Box::new(Done(Some(Ok(())), Printed { code: Some(0), stdout, stderr: String::new() }))
            }
        }
    }

    fn app(lines: usize) -> App {
        use crate::tree::{Action, Arg, Kind, Node, Setting};
        let actions = vec![
            Action::new("check", "probe check").reads().verifies(),
            Action::new("refuse", "probe fail").reads().verifies(),
            Action::new("plan", "probe plan {}").arg(Arg::Text("dir".into())).reads().reports(),
            Action::new("touch", "probe touch").reads(),
        ];
        let roots = vec![Node::group("keys", "", vec![Node::leaf("anthropic", "", Setting::new(Kind::Text, "present", "computed"))]).with_actions(actions).opened()];
        App::new("t", roots).adapter(Prints(lines))
    }

    fn key(app: &mut App, k: KeyCode) {
        app.key(KeyEvent::new(k, KeyModifiers::NONE));
    }

    fn shown(app: &App) -> &Shown {
        match &app.mode {
            Mode::Response(s) => s,
            _ => panic!("no response modal: {}", app.msg),
        }
    }

    #[test]
    fn a_reading_action_opens_its_response_and_a_key_closes_it() {
        let mut a = app(3);
        a.pick(vec![0], 0);
        assert_eq!((shown(&a).verdict, shown(&a).lines.clone()), (Verdict::Pass, vec!["probe check line 1".into(), "probe check line 2".into(), "probe check line 3".into(), String::new(), "exit 0".into()]));
        assert_eq!(a.message(), "check: ok", "the bottom bar keeps the outcome");
        let text = crate::testkit::rows(&crate::testkit::render(&mut a, 100, 30)).join("\n");
        assert!(text.contains("check: pass") && text.contains("$ probe check") && text.contains("probe check line 3"), "{text}");
        key(&mut a, KeyCode::Esc);
        assert!(matches!(a.mode, Mode::Browse));
        // A fail, with what it printed.
        a.pick(vec![0], 1);
        assert_eq!(shown(&a).verdict, Verdict::Fail);
        assert!(shown(&a).lines.contains(&"refused".to_string()));
        key(&mut a, KeyCode::Char('q'));
        assert!(matches!(a.mode, Mode::Browse));
        // A write that worked leaves the bottom bar to say so.
        a.pick(vec![0], 3);
        assert!(matches!(a.mode, Mode::Browse));
        assert_eq!(a.message(), "touch: ok");
    }

    #[test]
    fn a_reading_action_with_an_argument_runs_once_it_is_typed_and_queues_nothing() {
        let mut a = app(1);
        a.pick(vec![0], 2);
        assert!(matches!(a.mode, Mode::Arg { .. }));
        crate::testkit::type_str(&mut a, "~/x");
        key(&mut a, KeyCode::Enter);
        assert_eq!(shown(&a).verdict, Verdict::Report);
        assert_eq!(shown(&a).command, "probe plan ~/x");
        assert_eq!(a.pending(), 0, "nothing was queued");
    }

    #[test]
    fn the_wheel_scrolls_and_a_click_outside_closes_but_one_inside_does_not() {
        use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut a = app(60);
        a.pick(vec![0], 0);
        let _ = crate::testkit::render(&mut a, 100, 30);
        let (r, _) = a.hits.menu.clone().expect("the modal records its rect");
        let at = |kind, column, row| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
        a.mouse(at(MouseEventKind::ScrollDown, r.x + 2, r.y + 2));
        a.mouse(at(MouseEventKind::ScrollDown, r.x + 2, r.y + 2));
        assert_eq!(shown(&a).scroll, 2);
        a.mouse(at(MouseEventKind::ScrollUp, r.x + 2, r.y + 2));
        assert_eq!(shown(&a).scroll, 1);
        a.mouse(at(MouseEventKind::Down(MouseButton::Left), r.x + 2, r.y + 2));
        assert_eq!(shown(&a).scroll, 1, "a click inside leaves it open");
        a.mouse(at(MouseEventKind::Down(MouseButton::Left), 0, 0));
        assert!(matches!(a.mode, Mode::Browse), "a click outside closes it");
    }

    #[test]
    fn the_scroll_holds_at_the_last_page_as_drawn() {
        let mut a = app(60);
        a.pick(vec![0], 0);
        key(&mut a, KeyCode::End);
        let _ = crate::testkit::render(&mut a, 100, 30);
        let s = shown(&a).scroll;
        // 2 lines of header, 60 printed, a blank and the exit line, in a
        // modal of the main area's 28 rows less its two borders.
        assert_eq!(s, 64 - 26);
        key(&mut a, KeyCode::Up);
        assert_eq!(shown(&a).scroll, s - 1, "one key up moves the view at once");
        let text = crate::testkit::rows(&crate::testkit::render(&mut a, 100, 30)).join("\n");
        // The exit line went off the bottom; the line above the first came in.
        assert!(text.contains("probe check line 36") && !text.contains("exit 0"), "{text}");
    }

    #[test]
    fn a_response_ending_under_an_overlay_waits_for_the_screen_to_browse() {
        let mut a = app(1);
        a.mode = Mode::Help { scroll: 0 };
        a.read_now(&[0], 0, "");
        assert!(matches!(a.mode, Mode::Help { .. }), "the help stays open");
        a.tick();
        assert!(matches!(a.mode, Mode::Help { .. }));
        key(&mut a, KeyCode::Esc);
        a.tick();
        assert_eq!(shown(&a).verdict, Verdict::Pass, "it opens once the help has closed");
    }
}
