//! The test half of the engine (ADR-504 §12): a headless render of the real
//! app into a [`Buffer`], a text frame format that holds each cell's glyph,
//! colours and modifiers, and golden-frame checks against reviewed frames.
//!
//! A golden check is exact. A frame that drifts fails with a report of the
//! rows and styles that changed. Recording is explicit: with
//! `AGENT_TUI_BLESS=1` the frames are written and the check still fails,
//! naming each frame it wrote, so a bless never passes silently and a
//! re-run without it is what turns the check green. A missing golden fails
//! the same way. `AGENT_TUI_DUMP=<dir>` also writes every frame checked to
//! `<dir>`, for review as images or text.
//!
//! Interaction goes through the app's real key and mouse handlers.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use crate::adapter::{Adapter, Write};
use crate::app::App;
use crate::screen::Screen;
use crate::tree::{Queued, Store};

/// One frame of `app` at `w` by `h`, drawn as the run loop draws it.
pub fn render(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("a test backend");
    term.draw(|f| app.draw(f)).expect("a frame");
    term.backend().buffer().clone()
}

/// One frame of a [`Screen`] at `w` by `h`, drawn as [`crate::screen::run_screen`] draws
/// it: its palette set, the frame drawn, the theme's ground filled.
pub fn render_screen<S: Screen + ?Sized>(screen: &mut S, w: u16, h: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("a test backend");
    term.draw(|f| crate::screen::frame(screen, f)).expect("a frame");
    term.backend().buffer().clone()
}

/// Send each key to a [`Screen`]'s key handler. Returns false if any ended
/// the session; the keys after it are not sent.
pub fn drive<S: Screen + ?Sized>(screen: &mut S, keys: &[KeyEvent]) -> bool {
    keys.iter().all(|k| screen.key(*k))
}

/// The glyphs of a frame, one line per row.
pub fn text(buf: &Buffer) -> String {
    rows(buf).join("\n")
}

/// The glyphs of each row.
pub fn rows(buf: &Buffer) -> Vec<String> {
    let w = buf.area.width as usize;
    buf.content().chunks(w.max(1)).map(|r| r.iter().map(|c| c.symbol()).collect()).collect()
}

/// Where `needle` first appears in the frame, as (column, row).
pub fn find(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
    rows(buf).iter().enumerate().find_map(|(y, line)| line.find(needle).map(|b| (line[..b].chars().count() as u16, y as u16)))
}

/// Press each key, with no modifiers. Returns false if any ended the session.
pub fn press(app: &mut App, keys: &[KeyCode]) -> bool {
    keys.iter().all(|k| app.key(KeyEvent::new(*k, KeyModifiers::NONE)))
}

/// Type `s` one character at a time.
pub fn type_str(app: &mut App, s: &str) -> bool {
    s.chars().all(|c| app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)))
}

/// Key events from a script of tokens, so a headless run (a `--snap`, a
/// test of the real binary) drives the real key handler. A token is one
/// character, a key name (`enter`, `esc`, `tab`, `backtab`, `up`, `down`,
/// `left`, `right`, `home`, `end`, `pgup`, `pgdn`, `bksp`, `space`),
/// `ctrl-<c>` or `shift-<key>`, or `text:<chars>` to type the characters.
pub fn parse_keys<'a>(tokens: impl IntoIterator<Item = &'a str>) -> Result<Vec<KeyEvent>, String> {
    let mut out = Vec::new();
    for t in tokens {
        if let Some(text) = t.strip_prefix("text:") {
            out.extend(text.chars().map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
            continue;
        }
        let (mods, name) = match (t.strip_prefix("ctrl-"), t.strip_prefix("shift-")) {
            (Some(n), _) => (KeyModifiers::CONTROL, n),
            (_, Some(n)) => (KeyModifiers::SHIFT, n),
            _ => (KeyModifiers::NONE, t),
        };
        let code = match name {
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pgup" => KeyCode::PageUp,
            "pgdn" => KeyCode::PageDown,
            "bksp" => KeyCode::Backspace,
            "space" => KeyCode::Char(' '),
            n if n.chars().count() == 1 => KeyCode::Char(n.chars().next().expect("one character")),
            other => return Err(format!("unknown key `{other}`")),
        };
        out.push(KeyEvent::new(code, mods));
    }
    Ok(out)
}

/// Tick a running apply until it has ended and review has closed it out.
/// A command in flight is waited on in real time, up to a minute.
pub fn finish_apply(app: &mut App) {
    let start = std::time::Instant::now();
    while app.applying() {
        if start.elapsed() > std::time::Duration::from_secs(60) {
            panic!("the apply never ended");
        }
        app.tick();
        if app.applying() {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

fn color(c: Color) -> String {
    match c {
        Color::Reset => "-".into(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(n) => format!("i{n}"),
        other => format!("{other:?}"),
    }
}

fn mods(m: Modifier) -> String {
    if m.is_empty() {
        return "-".into();
    }
    format!("{m:?}").trim_start_matches("Modifier(").trim_end_matches(')').replace(" | ", "|")
}

/// A frame as text: a header, the glyphs of each row between bars (so
/// trailing spaces show), then one line per run of cells that share a
/// style other than the terminal default, as `row cols fg bg modifiers`.
pub fn frame(buf: &Buffer) -> String {
    let (w, h) = (buf.area.width, buf.area.height);
    let mut out = format!("agent-tui frame {w}x{h}\n");
    for r in rows(buf) {
        out.push('|');
        out.push_str(&r);
        out.push_str("|\n");
    }
    out.push_str("styles\n");
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let c = &buf[(x, y)];
            let key = (c.fg, c.bg, c.modifier);
            let start = x;
            while x < w && {
                let d = &buf[(x, y)];
                (d.fg, d.bg, d.modifier) == key
            } {
                x += 1;
            }
            if key != (Color::Reset, Color::Reset, Modifier::empty()) {
                out.push_str(&format!("{y:>3} {start:>3}-{:<3} {} {} {}\n", x - 1, color(key.0), color(key.1), mods(key.2)));
            }
        }
    }
    out
}

/// What differs between two frames in [`frame`]'s format, or `None` when
/// they are the same. Rows are compared by glyph and listed with both
/// versions; style runs are compared as lines, the missing and the extra.
pub fn compare(expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    let split = |s: &str| {
        let (glyphs, styles) = s.split_once("\nstyles\n").unwrap_or((s, ""));
        (glyphs.lines().map(str::to_string).collect::<Vec<_>>(), styles.lines().map(str::to_string).collect::<BTreeSet<_>>())
    };
    let ((eg, es), (ag, as_)) = (split(expected), split(actual));
    let mut out = String::new();
    if eg.first() != ag.first() {
        out += &format!("  size: expected {}, got {}\n", eg.first().map_or("", |s| s), ag.first().map_or("", |s| s));
    }
    let n = eg.len().max(ag.len());
    let mut shown = 0;
    let mut differing = 0;
    for i in 1..n {
        let (e, a) = (eg.get(i).map_or("", |s| s), ag.get(i).map_or("", |s| s));
        if e != a {
            differing += 1;
            if shown < 8 {
                out += &format!("  row {:>3} expected {e}\n          got      {a}\n", i - 1);
                shown += 1;
            }
        }
    }
    if differing > shown {
        out += &format!("  … {} more rows differ\n", differing - shown);
    }
    let missing: Vec<_> = es.difference(&as_).collect();
    let extra: Vec<_> = as_.difference(&es).collect();
    if !missing.is_empty() || !extra.is_empty() {
        out += &format!("  styles: {} runs gone, {} runs new\n", missing.len(), extra.len());
        for m in missing.iter().take(6) {
            out += &format!("    - {m}\n");
        }
        for x in extra.iter().take(6) {
            out += &format!("    + {x}\n");
        }
    }
    Some(out)
}

/// Golden frames in one directory, checked by name. [`Goldens::finish`]
/// reports every drifted, missing and blessed frame at once and fails if
/// there is any.
pub struct Goldens {
    dir: PathBuf,
    bless: bool,
    dump: Option<PathBuf>,
    drift: Vec<String>,
    blessed: Vec<String>,
}

impl Goldens {
    /// Frames under `dir`, as `<name>.frame`. Bless and dump follow the
    /// environment.
    pub fn new(dir: impl Into<PathBuf>) -> Goldens {
        let on = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
        let dump = std::env::var_os("AGENT_TUI_DUMP").filter(|v| !v.is_empty()).map(PathBuf::from);
        Goldens { dir: dir.into(), bless: on("AGENT_TUI_BLESS"), dump, drift: Vec::new(), blessed: Vec::new() }
    }

    /// Record frames instead of checking them, whatever the environment says.
    pub fn blessing(mut self, on: bool) -> Goldens {
        self.bless = on;
        self
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.frame"))
    }

    /// Check one frame against its golden, or record it when blessing.
    pub fn check(&mut self, name: &str, buf: &Buffer) {
        self.check_text(name, &frame(buf));
    }

    /// Check a frame already in [`frame`]'s format, such as a `--snap` of a
    /// real binary.
    pub fn check_text(&mut self, name: &str, actual: &str) {
        let actual = actual.to_string();
        if let Some(d) = &self.dump {
            let _ = std::fs::create_dir_all(d);
            let _ = std::fs::write(d.join(format!("{name}.frame")), &actual);
        }
        let path = self.path(name);
        if self.bless {
            let same = std::fs::read_to_string(&path).is_ok_and(|e| e == actual);
            if !same {
                std::fs::create_dir_all(&self.dir).expect("the golden directory");
                std::fs::write(&path, &actual).expect("a golden frame");
                self.blessed.push(path.display().to_string());
            }
            return;
        }
        match std::fs::read_to_string(&path) {
            Err(_) => self.drift.push(format!("{name}: no golden at {}; AGENT_TUI_BLESS=1 records it for review", path.display())),
            Ok(expected) => {
                if let Some(report) = compare(&expected, &actual) {
                    self.drift.push(format!("{name} drifted from {}:\n{report}", path.display()));
                }
            }
        }
    }

    /// Fail with every drifted, missing or blessed frame.
    pub fn finish(self) {
        let mut out = String::new();
        if !self.drift.is_empty() {
            out += &format!("{} golden frame(s) do not match:\n{}\n", self.drift.len(), self.drift.join("\n"));
        }
        if !self.blessed.is_empty() {
            out += &format!(
                "blessed {} frame(s); review them, then re-run without AGENT_TUI_BLESS to check:\n  {}\n",
                self.blessed.len(),
                self.blessed.join("\n  ")
            );
        }
        if !out.is_empty() {
            panic!("{out}");
        }
    }

    /// Whether this run records frames.
    pub fn is_blessing(&self) -> bool {
        self.bless
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// An adapter for tests: every write and run succeeds and is logged, except
/// the step a test plants a failure at.
#[derive(Default, Clone)]
pub struct Dry {
    /// The 1-based apply step that fails.
    pub fail_at: Option<usize>,
    steps: Rc<RefCell<usize>>,
    /// What was written and run, in order.
    pub log: Rc<RefCell<Vec<String>>>,
    /// The theme choice, once made.
    pub theme: Rc<RefCell<Option<String>>>,
}

impl Dry {
    /// Step `n` (from 1) of the next apply fails.
    pub fn failing(n: usize) -> Dry {
        Dry { fail_at: Some(n), ..Dry::default() }
    }

    fn step(&self) -> Result<(), String> {
        let mut s = self.steps.borrow_mut();
        *s += 1;
        if self.fail_at == Some(*s) {
            return Err(format!("planted failure at step {}", *s));
        }
        Ok(())
    }
}

impl Adapter for Dry {
    fn write(&mut self, file: &Path, values: &[Write]) -> Result<(), String> {
        self.step()?;
        let keys: Vec<String> = values.iter().map(|w| format!("{}={}", w.store.key, w.value)).collect();
        self.log.borrow_mut().push(format!("write {} {}", file.display(), keys.join(" ")));
        Ok(())
    }

    fn run(&mut self, q: &Queued) -> Result<(), String> {
        self.step()?;
        self.log.borrow_mut().push(format!("run {}{}", q.command, if q.stdin.is_some() { " (stdin)" } else { "" }));
        Ok(())
    }

    fn validate(&self, _: &Store, _: &str) -> Option<Result<String, String>> {
        None
    }

    fn choose_theme(&mut self, name: &str) -> Result<(), String> {
        *self.theme.borrow_mut() = Some(name.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn buf(lines: &[&str]) -> Buffer {
        Buffer::with_lines(lines.iter().copied())
    }

    #[test]
    fn identical_frames_compare_equal() {
        let a = frame(&buf(&["ab", "cd"]));
        assert_eq!(compare(&a, &a), None);
    }

    #[test]
    fn one_changed_cell_is_reported_by_row_and_by_style() {
        let a = buf(&["ab", "cd"]);
        let mut b = a.clone();
        b[(1, 1)].set_symbol("x");
        let r = compare(&frame(&a), &frame(&b)).expect("a drift");
        assert!(r.contains("row   1 expected |cd|") && r.contains("got      |cx|"), "{r}");
        let mut c = a.clone();
        c.set_style(Rect::new(0, 0, 1, 1), Style::new().fg(Color::Red).add_modifier(Modifier::BOLD));
        let r = compare(&frame(&a), &frame(&c)).expect("a style drift");
        assert!(r.contains("1 runs new") && r.contains("+   0   0-0   Red - BOLD"), "{r}");
    }

    #[test]
    fn a_shifted_frame_reports_every_row_it_moved() {
        let a = buf(&["ab ", "cd ", "ef "]);
        let b = buf(&[" ab", " cd", " ef"]);
        let r = compare(&frame(&a), &frame(&b)).expect("a drift");
        assert_eq!(r.matches("expected").count(), 3, "{r}");
    }

    #[test]
    fn a_size_change_is_named() {
        let r = compare(&frame(&buf(&["ab"])), &frame(&buf(&["abc"]))).expect("a drift");
        assert!(r.contains("expected agent-tui frame 2x1, got agent-tui frame 3x1"), "{r}");
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agent-tui-golden-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_missing_golden_fails_and_a_bless_records_it_and_still_fails() {
        let d = scratch("bless");
        let f = buf(&["ab"]);
        let mut g = Goldens::new(&d).blessing(false);
        g.check("one", &f);
        let missing = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.finish())).expect_err("a missing golden fails");
        assert!(missing.downcast_ref::<String>().is_some_and(|m| m.contains("no golden")));

        let mut g = Goldens::new(&d).blessing(true);
        g.check("one", &f);
        let blessed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.finish())).expect_err("a bless never passes");
        assert!(blessed.downcast_ref::<String>().is_some_and(|m| m.contains("blessed 1 frame")));
        assert_eq!(std::fs::read_to_string(d.join("one.frame")).unwrap(), frame(&f));

        // Checked again, it passes; a bless with nothing new passes too.
        let mut g = Goldens::new(&d).blessing(false);
        g.check("one", &f);
        g.finish();
        let mut g = Goldens::new(&d).blessing(true);
        g.check("one", &f);
        g.finish();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_drifted_golden_fails_with_its_report() {
        let d = scratch("drift");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("one.frame"), frame(&buf(&["ab"]))).unwrap();
        let mut g = Goldens::new(&d).blessing(false);
        g.check("one", &buf(&["ax"]));
        let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.finish())).expect_err("a drift fails");
        let m = e.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(m.contains("one drifted") && m.contains("|ax|"), "{m}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
