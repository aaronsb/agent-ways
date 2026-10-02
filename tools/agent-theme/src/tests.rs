use crate::oklab::lch;
use crate::*;

fn bundled() -> Vec<Theme> {
    BUNDLED.iter().map(|(n, s)| parse(s).unwrap_or_else(|e| panic!("{n}: {e:?}"))).collect()
}

fn nord_src() -> &'static str {
    BUNDLED[1].1
}

// ── Parsing ────────────────────────────────────────────────────

#[test]
fn bundled_themes_parse_and_match_their_file_names() {
    for ((stem, _), t) in BUNDLED.iter().zip(bundled()) {
        assert_eq!(*stem, t.name);
    }
    assert_eq!(bundled()[0].name, "agent-ways", "the default is listed first");
    assert_eq!(bundled().iter().filter(|t| t.kind == Kind::Light).count(), 1);
    assert!(bundled().iter().any(|t| t.background == Background::Fill));
}

#[test]
fn a_dotfiles_palette_loads_unchanged() {
    // The dotfiles layout as written: bare keys, quoted values, comments,
    // blank lines, and the tool bindings, which are accepted and ignored.
    let t = parse(nord_src()).unwrap();
    assert_eq!((t.name.as_str(), t.label.as_str(), t.kind), ("nord", "Nord", Kind::Dark));
    assert_eq!(t.slots.bg, Rgb(0x2e, 0x34, 0x40));
    assert_eq!(t.slots.alt, Rgb(0x88, 0xc0, 0xd0));
    assert_eq!(t.background, Background::Terminal, "THEME_BACKGROUND defaults to terminal");
    assert_eq!(t.overrides, Overrides::default());
    assert!(nord_src().contains("THEME_VIVID"), "the fixture carries the tool bindings");
}

#[test]
fn agent_ways_keys_pin_roles_and_set_the_background() {
    let src = format!("{}\nTHEME_BACKGROUND=\"fill\"\nTHEME_HOT=\"#010203\"\nTHEME_RULE=\"#040506\"\nTHEME_FADED=\"#070809\"\nTHEME_SELECTION=\"#0a0b0c\"\n", nord_src());
    let t = parse(&src).unwrap();
    assert_eq!(t.background, Background::Fill);
    let o = t.overrides;
    assert_eq!((o.hot, o.rule, o.faded, o.selection), (Some(Rgb(1, 2, 3)), Some(Rgb(4, 5, 6)), Some(Rgb(7, 8, 9)), Some(Rgb(10, 11, 12))));
    let r = Roles::derive(&t);
    assert_eq!((r.hot, r.rule, r.faded_text, r.selection_bg), (Rgb(1, 2, 3), Rgb(4, 5, 6), Rgb(7, 8, 9), Rgb(10, 11, 12)));
}

#[test]
fn validate_names_the_exact_problem() {
    assert!(validate(nord_src()).is_empty());
    let has = |src: String, needle: &str| {
        let e = validate(&src);
        assert!(e.iter().any(|x| x.to_string().contains(needle)), "{needle:?} not in {e:?}");
    };
    let nord = |from: &str, to: &str| nord_src().replace(from, to);
    has(nord("THEME_BG=\"#2e3440\"", "THEME_BG=\"#2e344\""), "line 6: `#2e344` is not a #rrggbb colour (key `THEME_BG`)");
    has(nord("THEME_ALT=", "THEME_COLOUR="), "unknown key `THEME_COLOUR`");
    has(nord("THEME_ALT=\"#88c0d0\"\n", ""), "missing slot `THEME_ALT`");
    has(nord("THEME_KIND=\"dark\"", "THEME_KIND=\"dusk\""), "THEME_KIND `dusk` must be dark or light");
    has(nord("THEME_KIND=\"dark\"", "MODE=\"dark\""), "line 4: unknown key `MODE`");
    has(nord("THEME_NAME=\"nord\"", "THEME_NAME=\"Nord Dark\""), "must be lowercase");
    has(nord("THEME_DIM=\"#4c566a\"", "THEME_DIM=#4c566a"), "double-quoted");
    has(nord("THEME_DIM=\"#4c566a\"", "THEME_DIM=3"), "`THEME_DIM` is a number");
    has(format!("{}\n[slots]\nbg = \"#000000\"\n", nord_src()), "`slots` is a table");
    has(nord("THEME_BACKGROUND", "X").replace("THEME_KIND=\"dark\"", "THEME_KIND=\"dark\"\nTHEME_BACKGROUND=\"paint\""), "must be terminal or fill");
    // Several problems are all reported, not just the first.
    assert!(validate("THEME_KIND=\"x\"\nFOO=\"y\"\n").len() >= 4);
}

#[test]
fn trailing_comments_are_allowed() {
    let src = nord_src().replace("THEME_KIND=\"dark\"", "THEME_KIND=\"dark\"   # note");
    assert_eq!(parse(&src).unwrap().kind, Kind::Dark);
}

#[test]
fn round_trip_every_bundled_theme_and_an_override() {
    let mut all = bundled();
    all[0].overrides.rule = Some(Rgb(9, 9, 9));
    all[1].label = "Quote \" and \\ slash".into();
    for t in all {
        assert_eq!(parse(&to_text(&t)).unwrap(), t, "{}", t.name);
    }
}

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("agent-theme-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn user_files_add_and_override_bundled_themes() {
    let dir = scratch_dir("load");
    let mut mine = bundled().remove(1);
    mine.label = "My Nord".into();
    std::fs::write(dir.join("nord.theme"), to_text(&mine)).unwrap();
    let mut extra = mine.clone();
    extra.name = "mine".into();
    std::fs::write(dir.join("mine.toml"), to_text(&extra)).unwrap();
    std::fs::write(dir.join("broken.theme"), "THEME_NAME=\"broken\"\n").unwrap();
    std::fs::write(dir.join("ignored.txt"), "x").unwrap();
    std::fs::write(active_file(&dir), "\n  mine \n").unwrap();

    let set = ThemeSet::load(Some(&dir));
    assert_eq!(set.get("nord").unwrap().label, "My Nord");
    let sources: Vec<_> = set.list().map(|(t, s)| (t.name.clone(), s)).collect();
    assert_eq!(sources[1], ("nord".to_string(), Source::Override));
    assert_eq!(sources.last().unwrap(), &("mine".to_string(), Source::User));
    assert_eq!(set.list().count(), BUNDLED.len() + 1);
    assert_eq!(set.rejected.len(), 1);
    assert!(set.rejected[0].0.ends_with("broken.theme"));
    assert!(set.get("broken").is_none());
    assert_eq!(active_name(&dir).as_deref(), Some("mine"));

    assert_eq!(ThemeSet::load(None).list().count(), BUNDLED.len());
    assert_eq!(ThemeSet::load(Some(&dir.join("missing"))).list().count(), BUNDLED.len());
    assert_eq!(active_name(&dir.join("missing")), None);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Derivation and floors ──────────────────────────────────────

#[test]
fn every_role_reads_on_its_ground_in_every_bundled_theme() {
    for t in bundled() {
        let r = Roles::derive(&t);
        assert!(r.unreadable().is_empty(), "{}: {:?}", t.name, r.unreadable());
        for (name, fg, grounds, min) in r.readable() {
            for g in grounds {
                let c = contrast(fg, g);
                assert!(c >= min, "{}: {name} {} on {} is {c:.2}, needs {min}", t.name, fg.hex(), g.hex());
            }
        }
    }
}

#[test]
fn status_roles_are_distinct_in_every_bundled_theme() {
    for t in bundled() {
        let r = Roles::derive(&t);
        assert!(r.too_close().is_empty(), "{}: {:?}", t.name, r.too_close());
    }
}

#[test]
fn the_floors_rescue_a_theme_that_fails_them_as_written() {
    // Every status slot the same dull grey, barely off the ground: as
    // written nothing reads and nothing is distinct.
    let mut t = parse(nord_src()).unwrap();
    let grey = Rgb(0x3a, 0x3f, 0x4a);
    for n in ["accent", "info", "ok", "warn", "err", "alt", "dim"] {
        t.slots.set(n, grey);
    }
    assert!(contrast(grey, t.slots.bg) < 1.5);
    let r = Roles::derive(&t);
    for (name, fg, grounds, min) in r.readable() {
        for g in grounds {
            assert!(contrast(fg, g) >= min, "{name} {} on {} below {min}", fg.hex(), g.hex());
        }
    }
    assert!(r.too_close().is_empty(), "{:?}", r.too_close());
    assert!(contrast(r.muted, t.slots.bg) >= MIN_MUTED);
}

#[test]
fn a_pinned_role_is_taken_as_given() {
    let mut t = parse(nord_src()).unwrap();
    t.overrides.hot = Some(t.slots.bg);
    assert_eq!(Roles::derive(&t).hot, t.slots.bg, "an override skips the lift");
}

#[test]
fn derivations_follow_dottheme() {
    let nord = parse(nord_src()).unwrap();
    let r = Roles::derive(&nord);
    let s = nord.slots;
    assert_eq!(r.rule, Rgb::blend(s.dim, s.subtle, 50));
    assert_eq!(r.track, s.subtle);
    assert_eq!(r.track_past, Rgb::blend(s.dim, s.subtle, 45));
    assert_eq!((r.ink, r.text), (s.bg, s.fg));
    assert_eq!(r.selection_bg, Rgb::blend(s.accent, s.bg, 13));
    let paper = Roles::derive(&parse(BUNDLED[6].1).unwrap());
    assert_eq!((paper.ink, paper.text), (Rgb(0x2b, 0x2a, 0x26), Rgb(0xf6, 0xf1, 0xe7)));
    assert_eq!(Rgb::blend(Rgb(255, 0, 3), Rgb(0, 255, 0), 50), Rgb(127, 127, 1));
}

#[test]
fn the_lift_keeps_hue_and_chroma_on_nord() {
    // An sRGB blend toward fg turned err, warn and accent into one dusty pink.
    let nord = parse(nord_src()).unwrap();
    let r = Roles::derive(&nord);
    for (name, slot, role) in [("err", nord.slots.err, r.err), ("warn", nord.slots.warn, r.warn), ("accent", nord.slots.accent, r.accent)] {
        let (a, b) = (lch(slot), lch(role));
        let turn = (a.h - b.h).abs().to_degrees();
        assert!(turn.min(360.0 - turn) < 20.0, "{name} turned {turn:.0}°: {} → {}", slot.hex(), role.hex());
        assert!(b.c >= a.c * 0.9, "{name} lost chroma: {:.3} → {:.3}", a.c, b.c);
    }
}

#[test]
fn contrast_and_oklab_known_values() {
    assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
    assert!((contrast(Rgb(0x76, 0x76, 0x76), Rgb(255, 255, 255)) - 4.54).abs() < 0.01);
    let red = lch(Rgb(255, 0, 0));
    assert!((red.l - 0.628).abs() < 1e-3 && (red.c - 0.2577).abs() < 1e-3);
    assert!(delta_e(Rgb(10, 20, 30), Rgb(10, 20, 30)) == 0.0);
}

// ── Painter ────────────────────────────────────────────────────

#[test]
fn the_terminal_palette_writes_the_ansi_codes_agent_ways_always_used() {
    let p = Painter::terminal(ColorDepth::TrueColor);
    let esc = |s: &str| format!("\x1b[{s}m");
    assert_eq!(p.sgr(Role::Ok), esc("32"));
    assert_eq!(p.sgr(Role::Err), esc("31"));
    assert_eq!(p.sgr(Style::new().role(Role::Warn).bold()), esc("1;33"));
    assert_eq!(p.sgr(Style::new().role(Role::Accent).bold().underline()), esc("1;4;36"));
    assert_eq!(p.sgr(Role::Muted), esc("2"));
    assert_eq!(p.sgr(Role::Selection), esc("7"));
    assert_eq!(p.sgr(Style::new().bold()), esc("1"));
    assert_eq!(p.sgr(Role::Body), "");
    assert_eq!(p.paint(Role::Ok, "ok"), format!("{}ok{RESET}", esc("32")));
    assert_eq!(p.paint(Role::Body, "plain"), "plain", "a style that changes nothing adds no reset");
}

#[test]
fn fixed_colours_keep_their_form_and_reduce_by_depth() {
    let gradient = Style::new().fg(Color::Indexed(209));
    assert_eq!(Painter::terminal(ColorDepth::TrueColor).sgr(gradient), "\x1b[38;5;209m");
    assert_eq!(Painter::terminal(ColorDepth::Ansi256).sgr(gradient), "\x1b[38;5;209m");
    let coral = Color::rgb(0xff, 0x6b, 0x6b);
    assert_eq!(Painter::terminal(ColorDepth::TrueColor).sgr(coral), "\x1b[38;2;255;107;107m");
    assert_eq!(Painter::terminal(ColorDepth::Ansi256).sgr(coral), format!("\x1b[38;5;{}m", nearest_256(Rgb(0xff, 0x6b, 0x6b))));
    assert_eq!(Painter::terminal(ColorDepth::Ansi16).sgr(coral), format!("\x1b[{}m", 90 + nearest_16(Rgb(0xff, 0x6b, 0x6b)) - 8));
}

#[test]
fn no_color_drops_colour_and_keeps_emphasis() {
    let p = Painter::terminal(ColorDepth::NoColor);
    assert_eq!(p.sgr(Role::Ok), "");
    assert_eq!(p.paint(Role::Err, "x"), "x");
    assert_eq!(p.sgr(Style::new().role(Role::Warn).bold()), "\x1b[1m");
    assert_eq!(p.sgr(Color::rgb(1, 2, 3)), "");
    assert_eq!(p.sgr(Role::Muted), "\x1b[2m");
    assert_eq!(p.sgr(Role::Selection), "\x1b[7m", "emphasis is reverse video");
    // A theme makes no difference once colour is off.
    let themed = Painter::themed(&bundled()[1], ColorDepth::NoColor);
    assert_eq!(themed.sgr(Role::Selection), "\x1b[7m");
    assert_eq!(themed.sgr(Role::Ok), "");
}

#[test]
fn plain_writes_nothing_at_all() {
    let p = Painter::plain();
    assert_eq!(p.paint(Style::new().role(Role::Warn).bold().reverse(), "x"), "x");
    assert_eq!(p.reset(), "");
    assert_eq!(p.pair(Role::Muted), (String::new(), ""));
}

#[test]
fn a_theme_draws_its_derived_roles() {
    let nord = bundled().remove(1);
    let r = Roles::derive(&nord);
    let p = Painter::themed(&nord, ColorDepth::TrueColor);
    let rgb = |c: Rgb| format!("38;2;{};{};{}", c.0, c.1, c.2);
    assert_eq!(p.sgr(Role::Ok), format!("\x1b[{}m", rgb(r.ok)));
    assert_eq!(p.sgr(Style::new().role(Role::Warn).bold()), format!("\x1b[1;{}m", rgb(r.warn)));
    assert_eq!(p.sgr(Role::Muted), format!("\x1b[{}m", rgb(r.muted)));
    let s = r.selection_bg;
    assert_eq!(p.sgr(Role::Selection), format!("\x1b[{};48;2;{};{};{}m", rgb(r.body), s.0, s.1, s.2));
    // At 256 colours the same roles go to the nearest index.
    let p256 = Painter::themed(&nord, ColorDepth::Ansi256);
    assert_eq!(p256.sgr(Role::Ok), format!("\x1b[38;5;{}m", nearest_256(r.ok)));
    assert!(!p.fills_background(), "nord keeps the terminal's ground");
    assert!(Painter::themed(&bundled()[6], ColorDepth::TrueColor).fills_background());
}

#[test]
fn a_scoped_painter_overrides_the_process_one_on_this_thread() {
    {
        let _g = scoped(Painter::plain());
        assert_eq!(paint(Role::Err, "x"), "x");
        {
            let _inner = scoped(Painter::terminal(ColorDepth::Ansi16));
            assert_eq!(sgr(Role::Err), "\x1b[31m");
        }
        assert_eq!(sgr(Role::Err), "", "the inner guard restored the outer");
    }
    let _g = scoped(Painter::terminal(ColorDepth::TrueColor));
    assert_eq!(pair(Role::Muted), ("\x1b[2m".to_string(), RESET));
}

#[test]
fn ratatui_output_maps_roles_and_attributes() {
    use ratatui_core::style::{Color as RC, Modifier};
    let p = Painter::terminal(ColorDepth::TrueColor);
    let s = p.ratatui(Style::new().role(Role::Warn).bold());
    assert_eq!(s.fg, Some(RC::Yellow));
    assert!(s.add_modifier.contains(Modifier::BOLD));
    assert!(p.ratatui(Role::Selection).add_modifier.contains(Modifier::REVERSED));
    let nord = bundled().remove(1);
    let r = Roles::derive(&nord);
    let t = Painter::themed(&nord, ColorDepth::TrueColor).ratatui(Role::Err);
    assert_eq!(t.fg, Some(RC::Rgb(r.err.0, r.err.1, r.err.2)));
    assert_eq!(Painter::themed(&nord, ColorDepth::NoColor).ratatui(Role::Err).fg, None);
    let paper = bundled().remove(6);
    let base = Painter::themed(&paper, ColorDepth::TrueColor).ratatui_base();
    assert_eq!(base.bg, Some(RC::Rgb(paper.slots.bg.0, paper.slots.bg.1, paper.slots.bg.2)));
    assert_eq!(Painter::themed(&nord, ColorDepth::TrueColor).ratatui_base(), ratatui_core::style::Style::new());
}

// ── Theme selection (ADR-504, note of 2026-10-01) ──────────────

#[test]
fn a_chosen_theme_is_used_at_truecolor_and_256() {
    let nord = bundled().remove(1);
    let r = Roles::derive(&nord);
    for depth in [ColorDepth::TrueColor, ColorDepth::Ansi256] {
        let p = Painter::select(Some(&nord), depth);
        assert_eq!(p.roles(), Some(&r), "{depth:?}");
        assert_eq!(p, Painter::themed(&nord, depth));
    }
    assert_eq!(Painter::select(Some(&nord), ColorDepth::Ansi256).sgr(Role::Ok), format!("\x1b[38;5;{}m", nearest_256(r.ok)));
}

#[test]
fn a_chosen_theme_falls_back_to_the_16_colour_default_whole() {
    let nord = bundled().remove(1);
    let p = Painter::select(Some(&nord), ColorDepth::Ansi16);
    assert_eq!(p, Painter::terminal(ColorDepth::Ansi16), "the default theme, not nord reduced to 16");
    assert!(p.roles().is_none());
    assert_eq!(p.sgr(Role::Ok), "\x1b[32m");
    assert_eq!(p.sgr(Role::Muted), "\x1b[2m");
}

#[test]
fn no_colour_ignores_the_chosen_theme() {
    let nord = bundled().remove(1);
    let p = Painter::select(Some(&nord), ColorDepth::NoColor);
    assert_eq!(p, Painter::terminal(ColorDepth::NoColor));
    assert_eq!(p.sgr(Role::Err), "");
    assert_eq!(p.sgr(Role::Selection), "\x1b[7m");
}

#[test]
fn no_choice_is_the_terminal_palette_at_every_depth() {
    for depth in [ColorDepth::TrueColor, ColorDepth::Ansi256, ColorDepth::Ansi16, ColorDepth::NoColor] {
        assert_eq!(Painter::select(None, depth), Painter::terminal(depth));
    }
}

#[test]
fn active_in_reads_the_choice_and_warns_when_it_cannot_be_honoured() {
    let dir = scratch_dir("active");
    let tc = ColorDepth::TrueColor;
    // No choice: the default, silently.
    assert_eq!(Painter::active_in(&dir, tc), (Painter::terminal(tc), None));
    // A bundled theme by name, and its 16-colour fallback.
    std::fs::write(active_file(&dir), "dracula\n").unwrap();
    let dracula = ThemeSet::bundled().get("dracula").unwrap().clone();
    assert_eq!(Painter::active_in(&dir, tc), (Painter::themed(&dracula, tc), None));
    assert_eq!(Painter::active_in(&dir, ColorDepth::Ansi16).0, Painter::terminal(ColorDepth::Ansi16));
    // A name that does not exist.
    std::fs::write(active_file(&dir), "nope\n").unwrap();
    let (p, w) = Painter::active_in(&dir, tc);
    assert_eq!(p, Painter::terminal(tc));
    assert!(w.unwrap().contains("`nope` not found"));
    // A broken user override of a bundled name: the bundled one, with a warning.
    std::fs::write(active_file(&dir), "nord\n").unwrap();
    std::fs::write(dir.join("nord.theme"), "THEME_NAME=\"nord\"\n").unwrap();
    let nord = ThemeSet::bundled().get("nord").unwrap().clone();
    let (p, w) = Painter::active_in(&dir, tc);
    assert_eq!(p, Painter::themed(&nord, tc));
    let w = w.unwrap();
    assert!(w.contains("nord.theme did not load") && w.contains("bundled"), "{w}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hex_must_be_six_hex_digits() {
    assert_eq!(Rgb::from_hex("#0aFf10"), Some(Rgb(0x0a, 0xff, 0x10)));
    for bad in ["#+f+f+f", "#-1-1-1", "#12345g", "#12345", "1234567", "#ｆｆｆ", "# 12345"] {
        assert_eq!(Rgb::from_hex(bad), None, "{bad}");
    }
    let src = nord_src().replace("THEME_BG=\"#2e3440\"", "THEME_BG=\"#+f+f+f\"");
    assert!(validate(&src).iter().any(|e| e.to_string().contains("`#+f+f+f` is not a #rrggbb colour")));
}

#[test]
fn a_grey_status_slot_stays_grey() {
    // Every status slot black: the hue of black is rounding noise, so the
    // roles that must move apart may move only in lightness.
    let mut t = parse(nord_src()).unwrap();
    for n in ["accent", "info", "ok", "warn", "err", "alt"] {
        t.slots.set(n, Rgb(0, 0, 0));
    }
    let r = Roles::derive(&t);
    for (name, c) in r.status() {
        assert!(lch(c).c < 0.02, "{name} was tinted: {}", c.hex());
    }
}
