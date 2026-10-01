use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::Terminal;

use super::depth::{nearest_16, nearest_256};
use super::*;

const MIN_TEXT_FLOOR: f64 = 4.5;

fn bundled() -> Vec<Theme> {
    BUNDLED.iter().map(|(n, s)| parse(s).unwrap_or_else(|e| panic!("{n}: {e:?}"))).collect()
}

#[test]
fn bundled_themes_parse_and_match_their_file_names() {
    for ((stem, _), t) in BUNDLED.iter().zip(bundled()) {
        assert_eq!(*stem, t.name);
    }
    assert_eq!(bundled().iter().filter(|t| t.kind == Kind::Light).count(), 1);
    assert!(bundled().iter().any(|t| t.background == Background::Terminal));
}

#[test]
fn every_role_reads_on_its_ground_in_every_bundled_theme() {
    for t in bundled() {
        let r = Roles::derive(&t);
        let (mut worst, mut worst_text) = (f64::MAX, f64::MAX);
        for (name, fg, grounds, min) in r.readable() {
            for g in grounds {
                let c = contrast(fg, g);
                worst = worst.min(c);
                if min == MIN_TEXT_FLOOR {
                    worst_text = worst_text.min(c);
                }
                assert!(c >= min, "{}: {name} {} on {} is {c:.2}, needs {min}", t.name, fg.hex(), g.hex());
            }
        }
        println!("{:<18} min contrast {worst:.2} (muted/faded floor 3), {worst_text:.2} (text floor 4.5)", t.name);
    }
}

#[test]
fn contrast_matches_known_values() {
    assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
    assert!((contrast(Rgb(255, 255, 255), Rgb(255, 255, 255)) - 1.0).abs() < 1e-9);
    // #767676 on white is the classic 4.54:1.
    assert!((contrast(Rgb(0x76, 0x76, 0x76), Rgb(255, 255, 255)) - 4.54).abs() < 0.01);
}

#[test]
fn derivations_follow_dottheme() {
    let nord = parse(BUNDLED[1].1).unwrap();
    let r = Roles::derive(&nord);
    let s = nord.slots;
    assert_eq!(r.rule, Rgb::blend(s.dim, s.subtle, 50));
    assert_eq!(r.track, s.subtle);
    assert_eq!(r.track_past, Rgb::blend(s.dim, s.subtle, 45));
    assert_eq!((r.ink, r.text), (s.bg, s.fg));
    let paper = Roles::derive(&parse(BUNDLED[6].1).unwrap());
    assert_eq!((paper.ink, paper.text), (Rgb(0x2b, 0x2a, 0x26), Rgb(0xf6, 0xf1, 0xe7)));
    // blend: integer division per channel, as the shell does it.
    assert_eq!(Rgb::blend(Rgb(255, 0, 3), Rgb(0, 255, 0), 50), Rgb(127, 127, 1));
}

#[test]
fn an_override_replaces_the_derived_role() {
    let mut t = bundled().remove(1);
    t.overrides.muted = Some(Rgb(1, 2, 3));
    assert_eq!(Roles::derive(&t).muted, Rgb(1, 2, 3));
}

#[test]
fn nearest_256_known_values() {
    assert_eq!(nearest_256(Rgb(255, 0, 0)), 196);
    assert_eq!(nearest_256(Rgb(0, 0, 0)), 16);
    assert_eq!(nearest_256(Rgb(255, 255, 255)), 231);
    assert_eq!(nearest_256(Rgb(0x5a, 0xc8, 0xfa)), 81); // cube (95,215,255)
    assert_eq!(nearest_256(Rgb(128, 128, 128)), 244); // grey ramp beats the cube
}

#[test]
fn nearest_16_known_values() {
    assert_eq!(nearest_16(Rgb(250, 10, 10)), Color::LightRed);
    assert_eq!(nearest_16(Rgb(10, 10, 10)), Color::Black);
    assert_eq!(nearest_16(Rgb(200, 200, 200)), Color::Gray);
    assert_eq!(nearest_16(Rgb(0, 200, 0)), Color::Green);
}

#[test]
fn depth_selects_the_colour_form() {
    let c = Rgb(0x5a, 0xc8, 0xfa);
    assert_eq!(color(c, ColorDepth::TrueColor), Some(Color::Rgb(0x5a, 0xc8, 0xfa)));
    assert_eq!(color(c, ColorDepth::Ansi256), Some(Color::Indexed(81)));
    assert!(matches!(color(c, ColorDepth::Ansi16), Some(Color::LightCyan | Color::Cyan)));
    assert_eq!(color(c, ColorDepth::None), None);
}

#[test]
fn depth_from_env() {
    use ColorDepth::{Ansi16, Ansi256, TrueColor};
    assert_eq!(ColorDepth::from_env(Some("1"), Some("truecolor"), None), ColorDepth::None);
    assert_eq!(ColorDepth::from_env(Some(""), Some("truecolor"), None), TrueColor);
    assert_eq!(ColorDepth::from_env(None, Some("24bit"), None), TrueColor);
    assert_eq!(ColorDepth::from_env(None, None, Some("xterm-256color")), Ansi256);
    assert_eq!(ColorDepth::from_env(None, None, Some("linux")), Ansi16);
}

#[test]
fn round_trip_every_bundled_theme_and_an_override() {
    let mut all = bundled();
    all[0].overrides.rule = Some(Rgb(9, 9, 9));
    for t in all {
        assert_eq!(parse(&to_toml(&t)).unwrap(), t, "{}", t.name);
    }
}

fn nord_with(edit: impl Fn(&str) -> String) -> String {
    edit(BUNDLED[1].1)
}

#[test]
fn validate_names_the_exact_problem() {
    assert!(validate(BUNDLED[1].1).is_empty());
    let has = |src: String, needle: &str| {
        let e = validate(&src);
        assert!(e.iter().any(|x| x.to_string().contains(needle)), "{needle} not in {e:?}");
    };
    has(nord_with(|s| s.replace("bg = \"#2e3440\"", "bg = \"#2e344\"")), "line 8: `#2e344` is not a #rrggbb colour (key `bg`)");
    has(nord_with(|s| s.replace("alt = ", "colour = ")), "unknown slot `colour`");
    has(nord_with(|s| s.replace("alt = \"#88c0d0\"\n", "")), "missing slot `alt`");
    has(nord_with(|s| s.replace("kind = \"dark\"", "kind = \"dusk\"")), "kind `dusk` must be dark or light");
    has(nord_with(|s| s.replace("kind = \"dark\"", "mode = \"dark\"")), "line 4: unknown key `mode`");
    has(nord_with(|s| format!("{s}\n[overrides]\nglow = \"#000000\"\n")), "unknown override `glow`");
    has(nord_with(|s| s.replace("[slots]", "[colours]")), "unknown section [colours]");
    has(nord_with(|s| s.replace("name = \"nord\"", "name = \"Nord Dark\"")), "must be lowercase");
    has(nord_with(|s| s.replace("dim = \"#4c566a\"", "dim = #4c566a")), "double-quoted");
    // Several problems are all reported, not just the first.
    assert!(validate("kind = \"x\"\nfoo = \"y\"\n").len() >= 4);
}

#[test]
fn trailing_comments_are_allowed() {
    let src = nord_with(|s| s.replace("kind = \"dark\"", "kind = \"dark\"   # note"));
    assert_eq!(parse(&src).unwrap().kind, Kind::Dark);
}

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("themes-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn user_files_add_and_override_bundled_themes() {
    let dir = scratch_dir("load");
    let mut mine = bundled().remove(1);
    mine.label = "My Nord".into();
    std::fs::write(dir.join("nord.toml"), to_toml(&mine)).unwrap();
    let mut extra = mine.clone();
    extra.name = "mine".into();
    std::fs::write(dir.join("mine.toml"), to_toml(&extra)).unwrap();
    std::fs::write(dir.join("broken.toml"), "name = \"broken\"\n").unwrap();
    std::fs::write(dir.join("ignored.txt"), "x").unwrap();

    let set = ThemeSet::load(Some(&dir));
    assert_eq!(set.get("nord").unwrap().label, "My Nord");
    let sources: Vec<_> = set.list().map(|(t, s)| (t.name.clone(), s)).collect();
    assert_eq!(sources[1], ("nord".to_string(), Source::Override));
    assert_eq!(sources.last().unwrap(), &("mine".to_string(), Source::User));
    assert_eq!(set.list().count(), BUNDLED.len() + 1);
    assert_eq!(set.rejected.len(), 1);
    assert!(set.rejected[0].0.ends_with("broken.toml"));
    assert!(set.get("broken").is_none());

    assert_eq!(ThemeSet::load(None).list().count(), BUNDLED.len());
    assert_eq!(ThemeSet::load(Some(&dir.join("missing"))).list().count(), BUNDLED.len());
    let _ = std::fs::remove_dir_all(&dir);
}

// Visual check: `cargo test --manifest-path ... swatch -- --ignored`, then
// cells2png on each themes-<name>.cells. THEME_SNAP_DIR picks the folder.
fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, st: Style) {
    buf.set_string(x, y, s, st);
}

fn sheet(t: &Theme) -> (Buffer, u16, u16) {
    let r = Roles::derive(t);
    let (w, h) = (86u16, 29u16);
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| {
        let area = Rect::new(0, 0, w, h);
        let c = |x: Rgb| Color::Rgb(x.0, x.1, x.2);
        let buf = f.buffer_mut();
        buf.set_style(area, Style::new().bg(c(r.bg)).fg(c(r.body)));
        put(buf, 1, 0, &format!("{} ({:?}, background {:?})", t.label, t.kind, t.background), Style::new().fg(c(r.accent_dim)).bg(c(r.bg)));
        put(buf, 1, 1, "role", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        put(buf, 20, 1, "on bg", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        put(buf, 44, 1, "on selection_bg", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        for (i, (name, fg, grounds, _)) in r.readable().iter().take(10).enumerate() {
            let y = 2 + i as u16;
            put(buf, 1, y, name, Style::new().fg(c(r.body)).bg(c(r.bg)));
            for (x0, g) in [(20u16, r.bg), (44, r.selection_bg)] {
                let label = format!(" {name} {} {:.1} ", fg.hex(), contrast(*fg, g));
                put(buf, x0, y, &format!("{label:<23}"), Style::new().fg(c(*fg)).bg(c(g)));
            }
            let _ = grounds;
        }
        let mut y = 13;
        put(buf, 1, y, "lozenge text on dark segment", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        y += 1;
        for (x, name, col) in [(1u16, " text ", r.text), (14, " faded_ink ", r.faded_ink), (30, " faded_text ", r.faded_text)] {
            put(buf, x, y, &format!("{name:<12}"), Style::new().fg(c(col)).bg(c(r.dark_seg)));
        }
        y += 2;
        put(buf, 1, y, "mode segments and badge", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        y += 1;
        let mut x = 1;
        for (n, p) in [("badge", r.badge), ("browse", r.mode_browse), ("edit", r.mode_edit), ("review", r.mode_review), ("apply", r.mode_apply)] {
            let label = format!(" {n} ");
            put(buf, x, y, &label, Style::new().fg(c(p.fg)).bg(c(p.bg)).bold());
            x += label.len() as u16 + 1;
        }
        y += 2;
        put(buf, 1, y, "decorative: rule, track, track_past", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        y += 1;
        put(buf, 1, y, &"─".repeat(30), Style::new().fg(c(r.rule)).bg(c(r.bg)));
        put(buf, 33, y, &" ".repeat(12), Style::new().bg(c(r.track)));
        put(buf, 45, y, &" ".repeat(12), Style::new().bg(c(r.track_past)));
        y += 2;
        put(buf, 1, y, "a tree row, as the TUI draws it", Style::new().fg(c(r.muted)).bg(c(r.bg)));
        y += 1;
        let row = Style::new().bg(c(r.selection_bg));
        put(buf, 1, y, &" ".repeat(60), row);
        put(buf, 1, y, "▌", row.fg(c(r.accent)));
        put(buf, 3, y, "model", row.fg(c(r.body)).bold());
        put(buf, 20, y, "opus", row.fg(c(r.accent_dim)));
        put(buf, 30, y, "changed", row.fg(c(r.warn)).bold());
        put(buf, 40, y, "queued", row.fg(c(r.hot)));
        put(buf, 50, y, "ro", row.fg(c(r.muted)).italic());
        y += 1;
        put(buf, 1, y, "  effort", Style::new().fg(c(r.body)).bg(c(r.bg)));
        put(buf, 20, y, "high", Style::new().fg(c(r.ok)).bg(c(r.bg)));
        put(buf, 30, y, "error text", Style::new().fg(c(r.err)).bg(c(r.bg)));
        put(buf, 44, y, "info", Style::new().fg(c(r.info)).bg(c(r.bg)));
        put(buf, 52, y, "alt", Style::new().fg(c(r.alt)).bg(c(r.bg)));
    })
    .unwrap();
    (term.backend().buffer().clone(), w, h)
}

#[test]
#[ignore = "writes swatch sheets for visual review"]
fn swatch_sheets() {
    let out = std::path::PathBuf::from(std::env::var("THEME_SNAP_DIR").unwrap_or_else(|_| "target/snap".into()));
    std::fs::create_dir_all(&out).unwrap();
    for t in bundled() {
        let (buf, w, h) = sheet(&t);
        let cells: Vec<String> = buf.content().iter().map(|c| format!("{}\t{:?}\t{:?}\t{:?}", c.symbol(), c.fg, c.bg, c.modifier)).collect();
        std::fs::write(out.join(format!("themes-{}.cells", t.name)), format!("{w} {h}\n{}\n", cells.join("\n"))).unwrap();
    }
}
