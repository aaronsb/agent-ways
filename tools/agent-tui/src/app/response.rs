//! The response modal (#778): what a command printed, in a scrollable text
//! area over the screen, closed by a key. One view serves every screen; an
//! action declares what it answers with ([`tree::Response`]), and the
//! outcome decides the verdict. A queued command that fails in an apply
//! opens it too, as an error. The text is the command's own output, so the
//! screen stays a view over the command.

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
    /// The first row in view, of the text as wrapped.
    pub(crate) scroll: usize,
    /// The last scroll that still fills the modal, as last drawn; no limit
    /// before the first draw. The draw owns it: only the draw knows how the
    /// text wraps.
    max: usize,
    /// The text wrapped at the widths last drawn: two, since text taller
    /// than the screen is measured at the modal's width and then drawn at
    /// the screen's, and one slot would wrap both on every frame.
    wrapped: Vec<(usize, Vec<Line<'static>>)>,
    /// How many times the text was wrapped.
    wraps: usize,
    /// The review tab it opened over, which closing it goes back to.
    pub(crate) back: Option<usize>,
}

/// The rows a page key moves.
const PAGE: usize = 10;
/// The most lines of output kept, half from the head and half from the
/// tail, and the most characters of one line: a command that prints
/// without end cannot fill memory or stall the draw.
const MAX_LINES: usize = 2000;
const MAX_LINE: usize = 1000;

impl Shown {
    /// The modal for a command that ended with `outcome`, or `None` when the
    /// bottom bar is enough: a write that succeeded. A verification fails
    /// only when its command ran to an exit; one that never ran, or that a
    /// signal ended, is an error.
    pub(crate) fn of(label: &str, command: &str, kind: tree::Response, outcome: &Result<(), String>, printed: Option<Printed>) -> Option<Shown> {
        let exited = printed.as_ref().is_some_and(|p| p.code.is_some());
        let verdict = match (kind, outcome.is_ok()) {
            (tree::Response::Write, true) => return None,
            (tree::Response::Verify, true) => Verdict::Pass,
            (tree::Response::Verify, false) if exited => Verdict::Fail,
            (tree::Response::Report, true) => Verdict::Report,
            (_, false) => Verdict::Error,
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
                    lines.push(match outcome {
                        // Nothing printed and a failure: why it failed.
                        Err(e) if p.code.is_none() => plain(e),
                        _ => "(it printed nothing)".into(),
                    });
                }
            }
            // A job that keeps no output: the outcome is all there is.
            None => lines.push(match outcome {
                Ok(()) => "(it printed nothing)".into(),
                Err(e) => plain(e),
            }),
        }
        // Past the cap, the head and the tail: a summary a command prints
        // last, as lint does, stays.
        if lines.len() > MAX_LINES {
            let gone = lines.len() - MAX_LINES;
            let tail = lines.split_off(lines.len() - MAX_LINES / 2);
            lines.truncate(MAX_LINES / 2);
            lines.push(format!("… {gone} line{} not shown", if gone == 1 { "" } else { "s" }));
            lines.extend(tail);
        }
        for l in &mut lines {
            if let Some((cut, _)) = l.char_indices().nth(MAX_LINE) {
                l.truncate(cut);
                l.push('…');
            }
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
        Some(Shown { label: label.to_string(), command: command.to_string(), verdict, lines, scroll: 0, max: usize::MAX, wrapped: Vec::new(), wraps: 0, back: None })
    }

    /// One key: scroll, or close. False closes the modal.
    pub(crate) fn key(&mut self, k: KeyEvent) -> bool {
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => return false,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1).min(self.max),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(PAGE),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(PAGE).min(self.max),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll = self.max,
            _ => {}
        }
        true
    }

    fn title(&self) -> String {
        format!("{}: {}", self.label, self.verdict.word())
    }

    /// The command line, a blank, then the lines.
    fn text(&self) -> Vec<Line<'static>> {
        let mut out = vec![Line::styled(format!("$ {}", self.command), theme::hint()), Line::raw("")];
        out.extend(self.lines.iter().map(|l| Line::raw(l.clone())));
        out
    }

    /// The text wrapped to `width` columns, wrapped again only when the
    /// width changes.
    fn wrapped(&mut self, width: usize) -> &[Line<'static>] {
        let i = match self.wrapped.iter().position(|(w, _)| *w == width) {
            Some(i) => i,
            None => {
                if self.wrapped.len() == 2 {
                    self.wrapped.remove(0);
                }
                self.wraps += 1;
                self.wrapped.push((width, crate::wrap::wrap_lines(&self.text(), width)));
                self.wrapped.len() - 1
            }
        };
        &self.wrapped[i].1
    }
}

/// Text as the modal draws it, as a terminal would show it. An escape
/// sequence is dropped whole: a CSI to its final byte, an OSC, DCS, SOS, PM
/// or APC string with its payload (a hyperlink's URL among them) to BEL or
/// ST, any other escape to its final byte. A carriage return writes over
/// the line from its start; a tab is four spaces; other control characters
/// are dropped. `NO_COLOR` is set for the commands the screens run, but a
/// command may still print an escape.
fn plain(text: &str) -> String {
    enum St {
        Text,
        /// After ESC.
        Esc,
        /// In a CSI: parameter and intermediate bytes, to a final byte.
        Csi,
        /// In a string, to BEL or ST.
        Str,
        /// ESC inside a string: `\` ends it as ST.
        StrEsc,
        /// An escape's intermediate bytes, to its final byte.
        Inter,
    }
    fn put(line: &mut Vec<char>, col: &mut usize, c: char) {
        if *col < line.len() {
            line[*col] = c;
        } else {
            line.push(c);
        }
        *col += 1;
    }
    let mut out = String::with_capacity(text.len());
    // The line being written, and where the cursor is on it.
    let mut line: Vec<char> = Vec::new();
    let mut col = 0;
    let mut st = St::Text;
    for c in text.chars() {
        // A newline ends a stray ESC or a sequence left open, and stays.
        if c == '\n' {
            st = St::Text;
        }
        st = match st {
            St::Esc => match c {
                '[' => St::Csi,
                ']' | 'P' | 'X' | '^' | '_' => St::Str,
                '\u{20}'..='\u{2f}' => St::Inter,
                _ => St::Text,
            },
            St::Csi | St::Inter if c == '\u{1b}' => St::Esc,
            St::Csi => match c {
                '\u{20}'..='\u{3f}' => St::Csi,
                // A final byte ends it; anything else breaks it, and goes too.
                _ => St::Text,
            },
            St::Inter => match c {
                '\u{20}'..='\u{2f}' => St::Inter,
                _ => St::Text,
            },
            St::Str => match c {
                '\u{7}' | '\u{9c}' => St::Text,
                '\u{1b}' => St::StrEsc,
                _ => St::Str,
            },
            St::StrEsc => match c {
                '\\' => St::Text,
                '\u{1b}' => St::StrEsc,
                _ => St::Str,
            },
            St::Text => match c {
                '\u{1b}' => St::Esc,
                '\u{9b}' => St::Csi,
                '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => St::Str,
                '\n' => {
                    out.extend(line.drain(..));
                    out.push('\n');
                    col = 0;
                    St::Text
                }
                '\r' => {
                    col = 0;
                    St::Text
                }
                '\t' => {
                    for _ in 0..4 {
                        put(&mut line, &mut col, ' ');
                    }
                    St::Text
                }
                c if c.is_control() => St::Text,
                c => {
                    put(&mut line, &mut col, c);
                    St::Text
                }
            },
        };
    }
    out.extend(line);
    out
}

impl App {
    /// Open the modal over browse, or over an idle review, which closing it
    /// goes back to. While anything else is open it is held, and opens after
    /// the key, click or tick that leaves the screen there.
    pub(super) fn show_response(&mut self, mut s: Shown) {
        match self.mode {
            Mode::Browse => self.mode = Mode::Response(Box::new(s)),
            Mode::Review { tab, run: None, discard: false } => {
                s.back = Some(tab);
                self.mode = Mode::Response(Box::new(s));
            }
            _ => self.held = Some(s),
        }
    }

    /// A held modal, opened once the screen can take it.
    pub(super) fn open_held(&mut self) {
        if matches!(self.mode, Mode::Browse | Mode::Review { run: None, discard: false, .. }) {
            if let Some(s) = self.held.take() {
                self.show_response(s);
            }
        }
    }

    pub(super) fn response_key(&mut self, mut s: Box<Shown>, k: KeyEvent) {
        if s.key(k) {
            self.mode = Mode::Response(s);
        } else if let Some(tab) = s.back {
            self.mode = Mode::Review { tab, run: None, discard: false };
        }
    }

    /// The open modal, drawn over `area`: the command line, then what it
    /// printed, wrapped to the modal and scrolled, the scroll held to the
    /// last page. The border and title carry the verdict. Its rect is
    /// recorded so a click outside it closes it.
    pub(super) fn draw_response(&mut self, f: &mut Frame, area: Rect) {
        let Mode::Response(s) = &mut self.mode else { return };
        let widest = s.text().iter().map(Line::width).max().unwrap_or(0);
        // Borders and a column of padding each side.
        let w = (widest + 4).clamp(64, (area.width as usize).saturating_sub(4).max(64)).min(u16::MAX as usize) as u16;
        let rows = s.wrapped(w.saturating_sub(4) as usize).len();
        let r = modal_rect(area, w, rows.saturating_add(2).min(u16::MAX as usize) as u16);
        let room = r.height.saturating_sub(2) as usize;
        let want = s.scroll;
        let text = s.wrapped(r.width.saturating_sub(4) as usize);
        let max = text.len().saturating_sub(room);
        let scroll = want.min(max);
        let shown: Vec<Line<'static>> = text[scroll..text.len().min(scroll + room)].to_vec();
        (s.scroll, s.max) = (scroll, max);
        let border = s.verdict.border();
        let block = pane(s.title()).border_style(border).title_style(border.add_modifier(Modifier::BOLD)).padding(Padding::horizontal(1));
        self.hits.menu = Some((r, Vec::new()));
        f.render_widget(Clear, r);
        f.render_widget(Paragraph::new(shown).block(block), r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Response;
    use ratatui::crossterm::event::KeyModifiers;

    /// ESC, spelled so no escape literal sits in the source.
    const E: char = '\u{1b}';

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
        let v = |kind, out: &Result<(), String>, p: Option<Printed>| Shown::of("x", "ways x", kind, out, p).map(|s| s.verdict);
        assert_eq!(v(Response::Write, &ok, printed(0, "", "")), None, "a write that worked leaves the bottom bar to say so");
        assert_eq!(v(Response::Write, &bad, printed(1, "", "")), Some(Verdict::Error));
        assert_eq!(v(Response::Verify, &ok, printed(0, "", "")), Some(Verdict::Pass));
        assert_eq!(v(Response::Verify, &bad, printed(1, "", "")), Some(Verdict::Fail));
        assert_eq!(v(Response::Report, &ok, printed(0, "", "")), Some(Verdict::Report));
        assert_eq!(v(Response::Report, &bad, printed(1, "", "")), Some(Verdict::Error));
        // A verification whose command never ran, or that a signal ended,
        // has no verdict to give: an error, not a fail.
        assert_eq!(v(Response::Verify, &Err("not a ways command: x".into()), None), Some(Verdict::Error));
        let signalled = Some(Printed { code: None, stdout: String::new(), stderr: String::new() });
        assert_eq!(v(Response::Verify, &Err("ended by a signal: x".into()), signalled), Some(Verdict::Error));
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
        // A job that keeps no output shows its outcome.
        let s = Shown::of("c", "c", Response::Verify, &Err("killed".into()), None).unwrap();
        assert_eq!(s.lines, ["killed"]);
    }

    #[test]
    fn output_past_the_caps_is_cut_and_says_so() {
        let text: String = (1..=MAX_LINES + 5).map(|i| format!("{i}\n")).chain(["summary: 3 findings\n".to_string()]).collect();
        let s = Shown::of("r", "r", Response::Report, &Ok(()), printed(0, &text, "")).unwrap();
        assert_eq!(s.lines.len(), MAX_LINES + 1);
        assert_eq!((s.lines[0].as_str(), s.lines[MAX_LINES / 2 - 1].as_str()), ("1", "1000"), "the head");
        assert_eq!(s.lines[MAX_LINES / 2], "… 6 lines not shown");
        assert_eq!(s.lines.last().unwrap(), "summary: 3 findings", "the tail, with the summary a command prints last");
        let s = Shown::of("r", "r", Response::Report, &Ok(()), printed(0, &"x".repeat(MAX_LINE + 50), "")).unwrap();
        assert_eq!(s.lines[0].chars().count(), MAX_LINE + 1);
        assert!(s.lines[0].ends_with('…'));
    }

    #[test]
    fn escapes_and_controls_are_dropped_whole_as_a_terminal_would() {
        // SGR, with parameters and with none.
        assert_eq!(plain(&format!("{E}[1;38;5;196mred{E}[m plain")), "red plain");
        // A private CSI with an intermediate byte, and cursor moves.
        assert_eq!(plain(&format!("a{E}[?25lb{E}[2 qc{E}[10;4Hd")), "abcd");
        // An OSC 8 hyperlink: the URL goes, the text stays, ended by BEL or ST.
        assert_eq!(plain(&format!("{E}]8;;https://x.example/a?b=c\u{7}link{E}]8;;{E}\\ done")), "link done");
        // A window title, ended by ST; a charset designation; a DCS.
        assert_eq!(plain(&format!("{E}]0;title{E}\\x{E}(By{E}Pq#0;2{E}\\z")), "xyz");
        // 8-bit CSI and OSC.
        assert_eq!(plain("a\u{9b}31mb\u{9d}0;t\u{9c}c"), "abc");
        // A carriage return writes over the line from its start; CRLF ends it.
        assert_eq!(plain("progress 10%\rprogress 100%\r\nnext\r\n"), "progress 100%\nnext\n");
        assert_eq!(plain("abcdef\rXY"), "XYcdef");
        // A newline after a stray ESC, or in a sequence left open, survives.
        assert_eq!(plain(&format!("a{E}\nb")), "a\nb");
        assert_eq!(plain(&format!("a{E}[12\nb{E}]0;never ended\nc")), "a\nb\nc");
        // A tab is four spaces; a bell and a backspace are dropped.
        assert_eq!(plain("a\tb\u{7}\u{8}"), "a    b");
    }

    #[test]
    fn arrows_and_pages_scroll_within_the_drawn_limit_and_esc_enter_q_close() {
        let text: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let mut s = Shown::of("r", "r", Response::Report, &Ok(()), printed(0, &text, "")).unwrap();
        // As a draw of 2 header rows and 30 lines in 3 rows would leave it.
        s.max = 29;
        assert!(press(&mut s, KeyCode::Down) && s.scroll == 1);
        assert!(press(&mut s, KeyCode::Char('j')) && s.scroll == 2);
        assert!(press(&mut s, KeyCode::Up) && s.scroll == 1);
        assert!(press(&mut s, KeyCode::PageDown) && s.scroll == 11);
        assert!(press(&mut s, KeyCode::PageUp) && s.scroll == 1);
        assert!(press(&mut s, KeyCode::PageUp) && s.scroll == 0, "no scrolling above the top");
        assert!(press(&mut s, KeyCode::End) && s.scroll == 29);
        assert!(press(&mut s, KeyCode::PageDown) && s.scroll == 29, "no scrolling past the last page");
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

    /// Commands print `n` lines of `width` columns and exit 0, except one
    /// whose line holds `fail`, which exits 1.
    struct Prints {
        n: usize,
        width: usize,
    }

    impl crate::Adapter for Prints {
        fn write(&mut self, _: &std::path::Path, _: &[crate::Write]) -> Result<(), String> {
            Ok(())
        }
        fn run(&mut self, _: &tree::Queued) -> Result<(), String> {
            unreachable!("commands are started")
        }
        fn start(&mut self, q: &tree::Queued) -> Box<dyn crate::adapter::Job> {
            let stdout: String = (1..=self.n).map(|i| format!("{:<1$}\n", format!("{} line {i}", q.command), self.width)).collect();
            if q.command.contains("fail") {
                Box::new(Done(Some(Err("exit 1: refused".into())), Printed { code: Some(1), stdout, stderr: "refused\n".into() }))
            } else {
                Box::new(Done(Some(Ok(())), Printed { code: Some(0), stdout, stderr: String::new() }))
            }
        }
    }

    fn app_of(n: usize, width: usize) -> App {
        use crate::tree::{Action, Arg, Kind, Node, Setting};
        let actions = vec![
            Action::new("check", "probe check").reads().verifies(),
            Action::new("refuse", "probe fail").reads().verifies(),
            Action::new("plan", "probe plan {}").arg(Arg::Text("dir".into())).reads().reports(),
            Action::new("touch", "probe touch").reads(),
            Action::new("wipe", "probe wipe {}").arg(Arg::Text("dir".into())).reads().confirm(),
        ];
        let leaf = Node::leaf("anthropic", "", Setting::new(Kind::Text, "present", "computed"));
        let roots = vec![Node::group("keys", "", vec![leaf]).with_actions(actions).opened()];
        App::new("t", roots).adapter(Prints { n, width })
    }

    fn app(n: usize) -> App {
        app_of(n, 0)
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

    fn screen(app: &mut App, w: u16, h: u16) -> String {
        crate::testkit::rows(&crate::testkit::render(app, w, h)).join("\n")
    }

    #[test]
    fn a_reading_action_opens_its_response_and_a_key_closes_it() {
        let mut a = app(3);
        a.pick(vec![0], 0);
        assert_eq!((shown(&a).verdict, shown(&a).lines.clone()), (Verdict::Pass, vec!["probe check line 1".into(), "probe check line 2".into(), "probe check line 3".into(), String::new(), "exit 0".into()]));
        assert_eq!(a.message(), "check: ok", "the bottom bar keeps the outcome");
        let text = screen(&mut a, 100, 30);
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
    fn a_reading_action_that_asks_first_still_asks() {
        let mut a = app(1);
        a.pick(vec![0], 4);
        crate::testkit::type_str(&mut a, "/tmp/x");
        key(&mut a, KeyCode::Enter);
        assert!(matches!(&a.mode, Mode::Confirm { queued } if queued.command == "probe wipe /tmp/x"), "it is confirmed, not run");
    }

    #[test]
    fn the_wheel_scrolls_and_a_click_outside_closes_but_one_inside_does_not() {
        use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut a = app(60);
        a.pick(vec![0], 0);
        let _ = screen(&mut a, 100, 30);
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
        let _ = screen(&mut a, 100, 30);
        key(&mut a, KeyCode::End);
        let _ = screen(&mut a, 100, 30);
        let s = shown(&a).scroll;
        // 2 rows of header, 60 printed, a blank and the exit line, in a
        // modal of the main area's 28 rows less its two borders.
        assert_eq!(s, 64 - 26);
        key(&mut a, KeyCode::Up);
        assert_eq!(shown(&a).scroll, s - 1, "one key up moves the view at once");
        let text = screen(&mut a, 100, 30);
        // The exit line went off the bottom; the line above the first came in.
        assert!(text.contains("probe check line 36") && !text.contains("exit 0"), "{text}");
    }

    #[test]
    fn text_taller_than_the_screen_is_wrapped_once_per_width_not_per_frame() {
        let mut a = app_of(60, 90);
        a.pick(vec![0], 0);
        let _ = screen(&mut a, 120, 30);
        let first = shown(&a).wraps;
        assert!(first > 0);
        for _ in 0..3 {
            let _ = screen(&mut a, 120, 30);
        }
        assert_eq!(shown(&a).wraps, first, "a second draw at the same size wraps nothing");
    }

    #[test]
    fn wrapped_output_scrolls_to_its_last_row_by_key() {
        // Lines wider than a 70-column screen wrap to two rows each: the
        // rows outnumber the lines, and End and Down still reach the exit.
        let mut a = app_of(30, 90);
        a.pick(vec![0], 0);
        let _ = screen(&mut a, 70, 20);
        key(&mut a, KeyCode::End);
        let text = screen(&mut a, 70, 20);
        assert!(text.contains("exit 0"), "End reaches the exit line:\n{text}");
        key(&mut a, KeyCode::Home);
        for _ in 0..200 {
            key(&mut a, KeyCode::Down);
            let _ = screen(&mut a, 70, 20);
        }
        let text = screen(&mut a, 70, 20);
        assert!(text.contains("exit 0"), "Down reaches the exit line:\n{text}");
        key(&mut a, KeyCode::Up);
        let text = screen(&mut a, 70, 20);
        assert!(!text.contains("exit 0"), "and one Up moves off it at once:\n{text}");
    }

    #[test]
    fn a_response_held_under_an_overlay_opens_on_the_key_that_closes_it() {
        let mut a = app(1);
        a.mode = Mode::Help { scroll: 0 };
        a.read_now(&[0], 0, "");
        assert!(matches!(a.mode, Mode::Help { .. }), "the help stays open");
        // No tick: the real loop ticks only while something runs.
        key(&mut a, KeyCode::Esc);
        assert_eq!(shown(&a).verdict, Verdict::Pass, "it opens as the help closes");
    }

    #[test]
    fn a_queued_command_that_fails_in_an_apply_opens_the_error_over_review() {
        let mut a = app(2);
        a.queue.push(tree::Queued::new("keys", "refuse", "probe fail", false));
        key(&mut a, KeyCode::Char('w'));
        key(&mut a, KeyCode::Char('a'));
        crate::testkit::finish_apply(&mut a);
        let s = shown(&a);
        assert_eq!((s.verdict, s.back, s.command.as_str()), (Verdict::Error, Some(0), "probe fail"));
        assert_eq!(s.lines, ["refused", "probe fail line 1", "probe fail line 2", "", "exit 1"]);
        assert!(a.message().contains("stopped at step 1 of 1"), "{}", a.message());
        key(&mut a, KeyCode::Esc);
        assert!(matches!(a.mode, Mode::Review { tab: 0, run: None, .. }), "closing it goes back to review");
        assert_eq!(a.queued().len(), 1, "the command stays queued");
    }
}
