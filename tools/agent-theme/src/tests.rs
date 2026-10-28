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
    let light: Vec<String> = bundled().into_iter().filter(|t| t.kind == Kind::Light).map(|t| t.name).collect();
    assert_eq!(light, ["paper", "miami", "tampa", "crystal-mountain"]);
    assert!(bundled().iter().any(|t| t.background == Background::Fill));
    assert!(bundled()[..7].iter().all(|t| t.gradient.is_none() && t.decor == Decor::default()), "the first seven set no chrome");
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

// ── Gradient and decoration ────────────────────────────────────

fn theme(name: &str) -> Theme {
    bundled().into_iter().find(|t| t.name == name).unwrap()
}

#[test]
fn the_chrome_keys_parse() {
    let miami = theme("miami");
    let g = miami.gradient.unwrap();
    assert_eq!((g.axis, g.stops()), (Axis::Vertical, &[Rgb(0xd4, 0xef, 0xff), Rgb(0xff, 0xf4, 0xee), Rgb(0xff, 0xe6, 0xee)][..]));
    let d = miami.decor;
    assert_eq!((d.border, d.rules, d.mark, d.presence, d.ornament), (Some(Border::Double), Some(["─", "═"]), Some("▌"), Some(["◆", "◇"]), Some("🦩")));
    let tampa = theme("tampa");
    assert_eq!(tampa.gradient.unwrap().axis, Axis::Horizontal);
    assert_eq!(tampa.decor.border, Some(Border::Glyphs(["╭", "╮", "╰", "╯", "~", "·", "│", "│"])));
    assert_eq!(tampa.decor.ornament, Some("⚡"));
    assert_eq!(theme("miami-nights").decor.border, Some(Border::Thick));
    let terra = theme("terra").decor;
    assert_eq!((terra.border, terra.rules, terra.mark, terra.presence, terra.ornament), (Some(Border::Glyphs(["╭", "╮", "╰", "╯", "─", "┈", "│", "│"])), Some(["┈", "∿"]), Some("❧"), Some(["✿", "❀"]), Some("✾")));
    let crystal = theme("crystal-mountain").decor;
    assert_eq!((crystal.border, crystal.rules, crystal.mark, crystal.presence, crystal.ornament), (Some(Border::Plain), Some(["⌃", "⋀"]), Some("⧫"), Some(["✦", "✧"]), Some("❄")));
    let claude = theme("maximum-claude").decor;
    assert_eq!((claude.border, claude.rules, claude.mark, claude.presence, claude.ornament), (Some(Border::Rounded), Some(["╌", "━"]), Some("❯"), Some(["✻", "✼"]), Some("✻")));
}

#[test]
fn the_chrome_keys_round_trip() {
    let mut t = theme("tampa");
    t.decor.mark = Some("\"");
    t.decor.presence = Some(["\\", "'"]);
    t.gradient = Gradient::new(Axis::Vertical, &[Rgb(1, 2, 3), Rgb(4, 5, 6), Rgb(7, 8, 9), Rgb(10, 11, 12)]);
    assert_eq!(parse(&to_text(&t)).unwrap(), t);
    for b in Border::NAMES {
        t.decor.border = Border::named(b);
        assert_eq!(parse(&to_text(&t)).unwrap(), t, "{b}");
    }
}

#[test]
fn chrome_errors_name_the_line_and_key() {
    let has = |src: String, needle: String| {
        let e = validate(&src);
        assert!(e.iter().any(|x| x.to_string().contains(&needle)), "{needle:?} not in {e:?}");
    };
    let src = theme_src("miami");
    let miami = |from: &str, to: &str| src.replace(from, to);
    // The 1-based line a key sits on in miami.theme.
    let at = |key: &str| src.lines().position(|l| l.starts_with(key)).unwrap() + 1;
    let g = at("THEME_GRADIENT=");
    has(miami("vertical #d4efff", "diagonal #d4efff"), format!("line {g}: THEME_GRADIENT axis `diagonal` must be vertical or horizontal"));
    has(miami("vertical #d4efff #fff4ee #ffe6ee", "vertical #d4efff"), format!("line {g}: THEME_GRADIENT takes 2 to 4 stops; `vertical #d4efff` has 1"));
    has(miami("#ffe6ee\"", "#ffe6ee #000000 #111111\""), "THEME_GRADIENT takes 2 to 4 stops; `vertical #d4efff #fff4ee #ffe6ee #000000 #111111` has 5".into());
    has(miami("#ffe6ee\"", "#ffe6e\""), format!("line {g}: `#ffe6e` is not a #rrggbb colour (key `THEME_GRADIENT`)"));
    has(miami("THEME_BACKGROUND=\"fill\"", "THEME_BACKGROUND=\"terminal\""), format!("line {g}: THEME_GRADIENT needs THEME_BACKGROUND=\"fill\""));
    let b = at("THEME_BORDER=");
    has(miami("THEME_BORDER=\"double\"", "THEME_BORDER=\"double\"\nTHEME_BORDER_GLYPHS=\"╭╮╰╯~·││\""), format!("line {}: THEME_BORDER and THEME_BORDER_GLYPHS cannot both be set", b + 1));
    has(miami("THEME_BORDER=\"double\"", "THEME_BORDER=\"dashed\""), format!("line {b}: THEME_BORDER `dashed` must be one of plain, rounded, double, thick"));
    has(miami("THEME_RULE_GLYPHS=\"─═\"", "THEME_RULE_GLYPHS=\"─═━\""), format!("line {}: `THEME_RULE_GLYPHS` takes 2 glyphs; `─═━` has 3", at("THEME_RULE_GLYPHS=")));
    has(miami("THEME_MARK=\"▌\"", "THEME_MARK=\"🌴\""), format!("line {}: `THEME_MARK` glyph `🌴` is 2 columns wide; it must be 1", at("THEME_MARK=")));
    has(miami("THEME_ORNAMENT=\"🦩\"", "THEME_ORNAMENT=\"🦩🌴\""), format!("line {}: `THEME_ORNAMENT` takes 1 glyph; `🦩🌴` has 2", at("THEME_ORNAMENT=")));
    has(miami("THEME_PRESENCE=\"◆◇\"", "THEME_PRESENCE=\"◆\u{301}\u{200b}\""), "`THEME_PRESENCE` glyph `\\u{200b}` is 0 columns wide; it must be 1".into());
}

#[test]
fn event_glyph_keys_parse_round_trip_and_default_to_today_s_set() {
    let d = Events::DEFAULT;
    let today = [d.head, d.mark, d.matched, d.injected, d.suppressed, d.redisclosed, d.would_block, d.blocked, d.refire_held, d.cap_held, d.due[0], d.due[1], d.approaching, d.halfway, d.check];
    assert_eq!(today, ["●", "┼", "◆", "◇", "⊝", "↩", "◌", "⊘", "◷", "⊟", "●", "○", "◐", "◔", "✓"]);
    assert!(bundled()[..7].iter().all(|t| t.decor.events == Events::DEFAULT), "the first seven keep today's set");
    let tampa = theme("tampa").decor.events;
    assert_eq!((tampa.head, tampa.mark, tampa.matched, tampa.injected, tampa.suppressed, tampa.redisclosed), ("↯", "┊", "⚔", "✦", "☈", "⟲"));
    assert_eq!((tampa.blocked, tampa.due), ("⊘", ["●", "○"]), "a key left out keeps the default");
    let six = |name: &str| {
        let e = theme(name).decor.events;
        [e.head, e.mark, e.matched, e.injected, e.suppressed, e.redisclosed]
    };
    assert_eq!(six("terra"), ["☼", "┆", "❁", "⁕", "☙", "↻"]);
    assert_eq!(six("crystal-mountain"), ["✶", "┊", "❆", "❅", "⋄", "⟲"]);
    assert_eq!(six("maximum-claude"), ["✺", "┊", "❋", "✱", "✧", "↻"]);
    let mut t = theme("miami-nights");
    t.decor.events.due = ["◉", "◎"];
    t.decor.events.check = "\"";
    let text = to_text(&t);
    assert!(text.contains("THEME_EVENT_DUE=\"◉◎\"") && !text.contains("THEME_EVENT_BLOCKED"), "only what differs from the default is written");
    assert_eq!(parse(&text).unwrap(), t);
}

#[test]
fn event_glyph_errors_name_the_line_and_key() {
    let has = |src: String, needle: &str| {
        let e = validate(&src);
        assert!(e.iter().any(|x| x.to_string().contains(needle)), "{needle:?} not in {e:?}");
    };
    let nights = |from: &str, to: &str| theme_src("miami-nights").replace(from, to);
    has(nights("THEME_EVENT_HEAD=\"◓\"", "THEME_EVENT_HEAD=\"◓◓\""), "`THEME_EVENT_HEAD` takes 1 glyph; `◓◓` has 2");
    has(nights("THEME_EVENT_HEAD=\"◓\"", "THEME_EVENT_DUE=\"●\""), "`THEME_EVENT_DUE` takes 2 glyphs; `●` has 1");
    has(nights("THEME_EVENT_HEAD=\"◓\"", "THEME_EVENT_HEAD=\"🌴\""), "`THEME_EVENT_HEAD` glyph `🌴` is 2 columns wide; it must be 1");
    has(nights("THEME_EVENT_HEAD=", "THEME_EVENT_TAIL="), "unknown key `THEME_EVENT_TAIL`");
}

// ── Words ─────────────────────────────────────────────────────

fn words(spinner: &[&str], done: &[&str]) -> Words {
    let own = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
    Words { spinner: own(spinner), done: own(done) }
}

#[test]
fn the_words_table_parses_and_the_decorated_themes_carry_one() {
    assert_eq!(theme("miami-nights").words, words(&["Cruising", "Gliding", "Glowing", "Pulsing", "Racing", "Drifting"], &["Cruised", "Glowed", "Docked", "Landed"]));
    assert_eq!(theme("tampa").words, words(&["Charging", "Striking", "Sailing", "Crossing", "Casting", "Plundering"], &["Struck", "Crossed", "Landed", "Plundered"]));
    assert_eq!(theme("miami").words, words(&["Sunning", "Surfing", "Strolling", "Sipping", "Sparkling", "Drifting"], &["Sunned", "Surfed", "Strolled", "Sipped"]));
    assert_eq!(theme("terra").words, words(&["Growing", "Rooting", "Weathering", "Flowing", "Sprouting", "Settling"], &["Grown", "Rooted", "Weathered", "Settled"]));
    assert_eq!(theme("crystal-mountain").words, words(&["Climbing", "Ascending", "Glinting", "Traversing", "Crystallizing", "Summiting"], &["Climbed", "Crossed", "Crystallized", "Summited"]));
    assert_eq!(theme("maximum-claude").words, words(&["Pondering", "Brewing", "Simmering", "Sparking", "Crafting", "Musing"], &["Brewed", "Crafted", "Pondered", "Sparked"]));
    assert!(bundled()[..7].iter().all(|t| t.words.is_empty()), "the first seven carry none");
    // One list alone is a table; the other stays empty.
    let t = parse(&format!("{}\n[words]\ndone = [\"Done\"]\n", nord_src())).unwrap();
    assert_eq!(t.words, words(&[], &["Done"]));
}

#[test]
fn the_words_table_round_trips() {
    let mut t = theme("tampa");
    let long = "x".repeat(24);
    t.words = words(&["Quote \" in", "Back\\slash", "Ünïcødé ⚡", &long], &["Done"]);
    let text = to_text(&t);
    assert!(text.contains("\n[words]\nspinner = ["), "{text}");
    assert_eq!(parse(&text).unwrap(), t);
    t.words = words(&["Only"], &[]);
    assert!(!to_text(&t).contains("done ="), "an empty list is left out");
    assert_eq!(parse(&to_text(&t)).unwrap(), t);
    t.words = Words::default();
    assert!(!to_text(&t).contains("[words]"));
    assert_eq!(parse(&to_text(&t)).unwrap(), t);
}

#[test]
fn a_dotted_target_key_names_the_table() {
    let n = nord_src().lines().count() + 2;
    for extra in ["target.claude-code.claude = \"#ff0000\"\n", "[target.claude-code.extra]\nclaude = \"#ff0000\"\n"] {
        let e = validate(&format!("{}\n{extra}", nord_src()));
        let want = format!("line {n}: `target` is a table; write `[target.claude-code]` and put each key under it");
        assert!(e.iter().any(|x| x.to_string() == want), "{want:?} not in {e:?}");
    }
}

#[test]
fn words_errors_name_the_line() {
    let has = |table: &str, needle: &str| {
        let src = format!("{}\n{table}", nord_src());
        let e = validate(&src);
        assert!(e.iter().any(|x| x.to_string().contains(needle)), "{needle:?} not in {e:?}");
    };
    // The table's header is the line after the blank one the test adds.
    let n = nord_src().lines().count() + 2;
    let at = |k: usize| format!("line {}: ", n + k);
    has("[words]\nspinner = []\n", &format!("{}`words.spinner` takes 1 to 12 entries; it has 0", at(1)));
    let thirteen = vec!["\"a\""; 13].join(", ");
    has(&format!("[words]\ndone = [{thirteen}]\n"), &format!("{}`words.done` takes 1 to 12 entries; it has 13", at(1)));
    let long = "y".repeat(25);
    has(&format!("[words]\nspinner = [\"Go\", \"{long}\"]\n"), &format!("{}`words.spinner` entry `{long}` is 25 characters", at(1)));
    has("[words]\nspinner = [\"\"]\n", "`words.spinner` entry `` is 0 characters");
    has("[words]\nspinner = [\"Tab\\there\"]\n", &format!("{}`words.spinner` entry \"Tab\\there\" has a control character", at(1)));
    has("[words]\nspinner = [\"Go\\u2028on\"]\n", &format!("{}`words.spinner` entry \"Go\\u{{2028}}on\" has a control character, a line separator or a bidi control", at(1)));
    has("[words]\ndone = [\"\\u202EDone\"]\n", &format!("{}`words.done` entry \"\\u{{202e}}Done\" has a control character, a line separator or a bidi control", at(1)));
    has("[words]\ndone = [\"Ok\\u2066\"]\n", "has a control character, a line separator or a bidi control");
    has("[words]\nspinner = [\"   \"]\n", &format!("{}`words.spinner` entry \"   \" is blank", at(1)));
    has("[words]\nspinner = [\"Go\", 3]\n", &format!("{}`words.spinner` must be an array of double-quoted strings", at(1)));
    has("[words]\nspinner = \"Go\"\n", "`words.spinner` must be an array of double-quoted strings");
    has("[words]\nspinner = [\"Go\"]\nverbs = [\"Go\"]\n", &format!("{}unknown key `words.verbs`; the keys are spinner and done", at(2)));
    has("[words]\n", &format!("{}`[words]` sets neither spinner nor done", at(0)));
    // An array outside the table is a value of the wrong type.
    let e = validate(&nord_src().replace("THEME_LABEL=\"Nord\"", "THEME_LABEL=[\"Nord\"]"));
    assert!(e.iter().any(|x| x.to_string().contains("`THEME_LABEL` is an array; a value is a double-quoted string")), "{e:?}");
}

fn theme_src(name: &str) -> &'static str {
    BUNDLED.iter().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn a_gradient_runs_from_its_first_stop_to_its_last_through_oklab() {
    let (a, b, c) = (Rgb(0x0d, 0x02, 0x21), Rgb(0x24, 0x10, 0x46), Rgb(0x4a, 0x0e, 0x4e));
    let g = Gradient::new(Axis::Vertical, &[a, b, c]).unwrap();
    assert_eq!((g.at(0.0), g.at(0.5), g.at(1.0)), (a, b, c), "evenly spaced stops");
    let two = Gradient::new(Axis::Horizontal, &[Rgb(0, 0, 0), Rgb(255, 255, 255)]).unwrap();
    assert_eq!(two.at(0.5), Rgb(99, 99, 99), "OKLab's middle grey, not sRGB's 127");
    assert_eq!((g.cell(0, 35), g.cell(34, 35), g.cell(0, 1)), (a, c, a));
    assert_eq!(g.samples().len(), 2 * 64 + 1);
    assert!(Gradient::new(Axis::Vertical, &[a]).is_none() && Gradient::new(Axis::Vertical, &[a; 5]).is_none());
}

#[test]
fn the_ground_is_the_gradient_at_truecolor_and_flat_below() {
    let nights = theme("miami-nights");
    let stops = nights.gradient.unwrap().stops().to_vec();
    let p = Painter::themed(&nights, ColorDepth::TrueColor);
    let ground = p.ground(120, 35).unwrap();
    assert_eq!((ground.at(7, 0), ground.at(7, 34)), (Color::Rgb(stops[0]), Color::Rgb(stops[2])));
    assert_eq!(ground.at(0, 17), ground.at(119, 17), "a vertical gradient is flat across a row");
    let tampa = theme("tampa");
    let (first, last) = { let s = tampa.gradient.unwrap(); (s.stops()[0], *s.stops().last().unwrap()) };
    let ground = Painter::themed(&tampa, ColorDepth::TrueColor).ground(80, 25).unwrap();
    assert_eq!((ground.at(0, 3), ground.at(79, 3)), (Color::Rgb(first), Color::Rgb(last)), "a horizontal gradient runs left to right");
    assert_eq!(ground.at(40, 0), ground.at(40, 24), "and is flat down a column");
    let p256 = Painter::themed(&nights, ColorDepth::Ansi256);
    assert!(p256.gradient().is_none());
    let flat = p256.ground(120, 35).unwrap();
    assert_eq!((flat.at(0, 0), flat.at(119, 34)), (Color::Indexed(nearest_256(nights.slots.bg)), Color::Indexed(nearest_256(nights.slots.bg))));
    assert!(Painter::themed(&bundled()[1], ColorDepth::TrueColor).ground(10, 10).is_none(), "nord keeps the terminal's ground");
}

#[test]
fn text_roles_read_on_every_point_of_the_gradient() {
    // A ground that runs from nord's bg up to its dim slot: roles that read
    // on bg alone fall short toward the light end, and the floors lift them.
    // Body is the theme's own fg and is never lifted; it reads there as written.
    let mut t = parse(nord_src()).unwrap();
    t.background = Background::Fill;
    let flat = Roles::derive(&t);
    let g = Gradient::new(Axis::Vertical, &[t.slots.bg, Rgb(0x4c, 0x56, 0x6a)]).unwrap();
    assert!(flat.readable().iter().any(|(_, fg, _, min)| contrast(*fg, Rgb(0x4c, 0x56, 0x6a)) < *min), "the light end fails some role as derived without it");
    t.gradient = Some(g);
    let r = Roles::derive(&t);
    let (_, _, grounds, _) = &r.readable()[0];
    assert!(g.samples().iter().all(|s| grounds.contains(s)), "the gradient is part of the ground the floors check");
    assert!(r.unreadable().is_empty(), "{:?}", r.unreadable());
}

#[test]
fn the_painter_hands_out_the_theme_s_chrome() {
    use ratatui_core::symbols::border;
    let tampa = theme("tampa");
    let set = Painter::themed(&tampa, ColorDepth::TrueColor).ratatui_border().unwrap();
    assert_eq!((set.top_left, set.horizontal_top, set.horizontal_bottom, set.vertical_right), ("╭", "~", "·", "│"));
    assert_eq!(Painter::themed(&theme("miami"), ColorDepth::Ansi256).ratatui_border(), Some(border::DOUBLE));
    assert_eq!(Painter::themed(&bundled()[1], ColorDepth::TrueColor).ratatui_border(), None);
}

#[test]
fn decoration_survives_16_colours_and_no_color_without_the_theme_s_colours() {
    use ratatui_core::symbols::border;
    let miami = theme("miami");
    for depth in [ColorDepth::Ansi16, ColorDepth::NoColor] {
        let p = Painter::select(Some(&miami), depth);
        assert_eq!(p.decor(), miami.decor, "{depth:?}");
        assert_eq!(p.ratatui_border(), Some(border::DOUBLE), "{depth:?}");
        assert!(p.roles().is_none() && p.gradient().is_none() && p.ground(80, 25).is_none(), "{depth:?}: no theme colour");
        let bare = Painter::terminal(depth);
        for r in [Role::Body, Role::Accent, Role::Ok, Role::Err, Role::Muted, Role::Selection] {
            assert_eq!(p.sgr(r), bare.sgr(r), "{depth:?} {r:?}: the terminal palette's");
        }
    }
    assert_eq!(Painter::select(Some(&miami), ColorDepth::NoColor).sgr(Role::Ok), "", "no colour at all");
    let nord = &bundled()[1];
    for depth in [ColorDepth::Ansi16, ColorDepth::NoColor] {
        assert_eq!(Painter::select(Some(nord), depth), Painter::terminal(depth), "an undecorated theme is the terminal palette whole");
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
    // The provisional `active` file of #711 is no longer read: the choice
    // is the `theme.active` settings key (ADR-503).
    std::fs::write(dir.join("active"), "mine\n").unwrap();

    let set = ThemeSet::load(Some(&dir));
    assert_eq!(set.get("nord").unwrap().label, "My Nord");
    let sources: Vec<_> = set.list().map(|(t, s)| (t.name.clone(), s)).collect();
    assert_eq!(sources[1], ("nord".to_string(), Source::Override));
    assert_eq!(sources.last().unwrap(), &("mine".to_string(), Source::User));
    assert_eq!(set.list().count(), BUNDLED.len() + 1);
    assert_eq!(set.rejected.len(), 1);
    assert!(set.rejected[0].0.ends_with("broken.theme"));
    assert!(set.get("broken").is_none());
    assert!(set.get("active").is_none(), "the old choice file is not a theme");

    assert_eq!(ThemeSet::load(None).list().count(), BUNDLED.len());
    assert_eq!(ThemeSet::load(Some(&dir.join("missing"))).list().count(), BUNDLED.len());
    // With the old choice file present and no name given, the default.
    assert_eq!(Painter::named_in(None, Some(&dir), ColorDepth::TrueColor), (Painter::terminal(ColorDepth::TrueColor), None));
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

/// The roles lifted for the ground, with their floors. Body is the theme's
/// fg as written.
fn lifted(r: &Roles) -> [(&'static str, Rgb, f64); 9] {
    [
        ("accent", r.accent, MIN_TEXT),
        ("accent_dim", r.accent_dim, MIN_TEXT),
        ("info", r.info, MIN_TEXT),
        ("ok", r.ok, MIN_TEXT),
        ("warn", r.warn, MIN_TEXT),
        ("err", r.err, MIN_TEXT),
        ("alt", r.alt, MIN_TEXT),
        ("hot", r.hot, MIN_TEXT),
        ("muted", r.muted, MIN_MUTED),
    ]
}

/// The first cell along the gradient of a frame `cells` long where a lifted
/// role falls below its floor.
fn first_dip(p: &Painter, cells: u16) -> Option<(&'static str, u16, f64)> {
    let ground = p.ground(cells, cells).unwrap();
    for n in 0..cells {
        let Color::Rgb(g) = ground.at(n, n) else { panic!("truecolor ground") };
        for (name, fg, min) in lifted(p.roles().unwrap()) {
            let c = contrast(fg, g);
            if c < min {
                return Some((name, n, c));
            }
        }
    }
    None
}

#[test]
fn a_role_reads_between_the_gradient_s_samples() {
    // Luminance dips between two OKLab samples here: as first sampled, warn
    // passed every sample and fell to 4.49:1 at row 64 of 128.
    let mut t = parse(nord_src()).unwrap();
    t.background = Background::Fill;
    t.slots.fg = Rgb::from_hex("#f4f4f4").unwrap();
    t.slots.warn = Rgb::from_hex("#7814d2").unwrap();
    t.gradient = Gradient::new(Axis::Vertical, &["#02442a", "#327a48", "#632f73"].map(|h| Rgb::from_hex(h).unwrap()));
    let p = Painter::themed(&t, ColorDepth::TrueColor);
    assert_eq!(first_dip(&p, 128), None);
}

/// A seeded xorshift, for themes drawn at random but the same every run.
struct Xs(u64);

impl Xs {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn rgb(&mut self) -> Rgb {
        let v = self.next();
        Rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8)
    }
    /// A colour in the dark or the light third of each channel.
    fn ground(&mut self, dark: bool) -> Rgb {
        let v = self.next();
        let c = |x: u64| if dark { (x % 110) as u8 } else { 145 + (x % 111) as u8 };
        Rgb(c(v), c(v >> 8), c(v >> 16))
    }
}

#[test]
fn every_rendered_cell_of_a_gradient_holds_where_its_samples_do() {
    let mut x = Xs(0x9e37_79b9_7f4a_7c15);
    let (mut themes, mut held) = (0, 0);
    while themes < 200 {
        let mut t = parse(nord_src()).unwrap();
        let dark = x.next().is_multiple_of(2);
        t.kind = if dark { Kind::Dark } else { Kind::Light };
        t.background = Background::Fill;
        t.slots.bg = x.ground(dark);
        t.slots.fg = x.ground(!dark);
        for n in ["dim", "subtle", "accent", "info", "ok", "warn", "err", "alt"] {
            t.slots.set(n, x.rgb());
        }
        let stops: Vec<Rgb> = (0..2 + x.next() % 3).map(|_| x.ground(dark)).collect();
        let axis = if x.next().is_multiple_of(2) { Axis::Vertical } else { Axis::Horizontal };
        t.gradient = Gradient::new(axis, &stops);
        themes += 1;
        let p = Painter::themed(&t, ColorDepth::TrueColor);
        // Where a sample fails, the floor was out of reach; the samples
        // promise nothing about the cells between.
        if lifted(p.roles().unwrap()).iter().any(|(_, fg, min)| p.roles().unwrap().grounds().iter().any(|g| contrast(*fg, *g) < *min)) {
            continue;
        }
        held += 1;
        // Every cell of every frame 1 to 200 cells long, each colour once.
        let mut seen = std::collections::HashSet::new();
        for cells in 1..=200u16 {
            let ground = p.ground(cells, cells).unwrap();
            for n in 0..cells {
                let Color::Rgb(g) = ground.at(n, n) else { panic!("truecolor ground") };
                if !seen.insert(g) {
                    continue;
                }
                for (name, fg, min) in lifted(p.roles().unwrap()) {
                    let c = contrast(fg, g);
                    assert!(c >= min, "theme {themes}: {name} is {c:.3}:1 on cell {n} of {cells}\n{}", to_text(&t));
                }
            }
        }
    }
    assert!(held >= 150, "{held} of {themes} themes reach their floors, too few to show the bound");
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
fn named_in_takes_the_choice_and_warns_when_it_cannot_be_honoured() {
    let dir = scratch_dir("active");
    let tc = ColorDepth::TrueColor;
    // No choice, or the default's name: the default, silently.
    assert_eq!(Painter::named_in(None, Some(&dir), tc), (Painter::terminal(tc), None));
    assert_eq!(Painter::named_in(Some(TERMINAL), Some(&dir), tc), (Painter::terminal(tc), None));
    // A bundled theme by name, and its 16-colour fallback.
    let dracula = ThemeSet::bundled().get("dracula").unwrap().clone();
    assert_eq!(Painter::named_in(Some("dracula"), Some(&dir), tc), (Painter::themed(&dracula, tc), None));
    assert_eq!(Painter::named_in(Some("dracula"), Some(&dir), ColorDepth::Ansi16).0, Painter::terminal(ColorDepth::Ansi16));
    // A name that does not exist.
    let (p, w) = Painter::named_in(Some("nope"), Some(&dir), tc);
    assert_eq!(p, Painter::terminal(tc));
    assert!(w.unwrap().contains("`nope` not found"));
    // A broken user override of a bundled name: the bundled one, with a warning.
    std::fs::write(dir.join("nord.theme"), "THEME_NAME=\"nord\"\n").unwrap();
    let nord = ThemeSet::bundled().get("nord").unwrap().clone();
    let (p, w) = Painter::named_in(Some("nord"), Some(&dir), tc);
    assert_eq!(p, Painter::themed(&nord, tc));
    let w = w.unwrap();
    assert!(w.contains("nord.theme did not load") && w.contains("bundled"), "{w}");
    let _ = std::fs::remove_dir_all(&dir);
    // No user directory, as with HOME unset: the bundled themes alone.
    assert_eq!(Painter::named_in(Some("dracula"), None, tc), (Painter::themed(&dracula, tc), None));
}

#[test]
fn an_underline_takes_the_theme_s_rule_glyph() {
    let ascii = "---  -----  ======";
    for p in [Painter::plain(), Painter::terminal(ColorDepth::TrueColor), Painter::themed(&bundled()[1], ColorDepth::TrueColor)] {
        assert_eq!(p.underline(ascii), ascii, "no rule glyph, the ASCII as written: {p:?}");
    }
    let tampa = theme("tampa");
    for depth in [ColorDepth::TrueColor, ColorDepth::Ansi16] {
        assert_eq!(Painter::select(Some(&tampa), depth).underline(ascii), "~~~  ~~~~~  ~~~~~~", "{depth:?}");
    }
}

#[test]
fn a_frame_with_no_cells_has_no_ground() {
    for name in ["miami-nights", "tampa", "paper"] {
        let p = Painter::themed(&theme(name), ColorDepth::TrueColor);
        assert_eq!((p.ground(80, 0), p.ground(0, 25)), (None, None), "{name}");
        assert_eq!(p.block("x\n", 0), "x\n", "{name}");
    }
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

// ── Report blocks ──────────────────────────────────────────────

/// `s` with its escape sequences taken out.
fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn bg(c: Rgb) -> String {
    format!("48;2;{};{};{}m", c.0, c.1, c.2)
}

#[test]
fn a_block_is_unchanged_where_no_ground_is_painted() {
    let report = "Status\n  \x1b[32mok\x1b[0m engine\n";
    let paper = theme("paper");
    for p in [
        Painter::plain(),
        Painter::terminal(ColorDepth::TrueColor),
        Painter::themed(&bundled()[1], ColorDepth::TrueColor),
        Painter::select(Some(&paper), ColorDepth::Ansi16),
        Painter::select(Some(&paper), ColorDepth::NoColor),
    ] {
        assert_eq!(p.block(report, 40), report, "{p:?}");
    }
}

#[test]
fn a_fill_theme_paints_every_line_out_to_the_width() {
    let paper = theme("paper");
    let p = Painter::themed(&paper, ColorDepth::TrueColor);
    let out = p.block("one\n\x1b[1mtwo\x1b[0m end\n", 12);
    assert!(out.ends_with('\n'), "the trailing newline is kept");
    let lines: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
    assert_eq!(lines.len(), 2);
    for l in &lines {
        assert_eq!(visible(l).chars().count(), 12, "{l:?}");
        assert!(l.contains(&bg(paper.slots.bg)) && l.ends_with(RESET), "{l:?}");
    }
    let body = Roles::derive(&paper).body;
    assert!(lines[0].starts_with(&format!("\x1b[38;2;{};{};{}m", body.0, body.1, body.2)), "text with no colour of its own is body");
    let after = lines[1].split("\x1b[0m").nth(1).unwrap();
    assert!(after.starts_with("\x1b[38;2;") && after.contains(&bg(paper.slots.bg)), "a reset mid-line puts the ground back: {after:?}");
    let flat = Painter::themed(&paper, ColorDepth::Ansi256).block("x", 4);
    assert!(flat.contains(&format!("48;5;{}m", nearest_256(paper.slots.bg))), "256 colours: the flat bg");
}

#[test]
fn a_vertical_gradient_runs_down_the_block_and_a_horizontal_one_across_it() {
    let nights = theme("miami-nights");
    let stops = nights.gradient.unwrap().stops().to_vec();
    let out = Painter::themed(&nights, ColorDepth::TrueColor).block("a\nb\nc", 6);
    let lines: Vec<&str> = out.split('\n').collect();
    assert!(lines[0].contains(&bg(stops[0])) && lines[2].contains(&bg(stops[2])), "{lines:?}");
    assert!(!lines[0].contains(&bg(stops[2])));
    let tampa = theme("tampa");
    let stops = tampa.gradient.unwrap().stops().to_vec();
    let row = Painter::themed(&tampa, ColorDepth::TrueColor).block("ab", 10);
    assert!(row.contains(&bg(stops[0])) && row.contains(&bg(*stops.last().unwrap())), "both ends of the row");
    assert_eq!(visible(&row), "ab        ");
}

#[test]
fn a_block_past_the_last_row_stays_on_the_last_ground() {
    let out = Painter::themed(&theme("miami-nights"), ColorDepth::TrueColor).block(&"x\n".repeat(65_537), 4);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines[65_536], lines[65_534], "the vertical gradient does not start over");
    assert_ne!(lines[65_536], lines[0]);
}

#[test]
fn a_line_longer_than_the_width_is_padded_to_the_end_of_its_last_row() {
    let tampa = theme("tampa");
    let p = Painter::themed(&tampa, ColorDepth::TrueColor);
    assert_eq!(visible(&p.block("abcdefghijklm", 10)), format!("abcdefghijklm{}", " ".repeat(7)));
    let first = tampa.gradient.unwrap().stops()[0];
    let wrapped = p.block("0123456789x", 10);
    assert!(wrapped.contains(&format!("{}x", bg(first))), "the wrapped row starts the horizontal ground over: {wrapped:?}");
    assert_eq!(p.block("text", 0), "text", "no width, nothing to paint");
}

#[test]
fn a_span_with_its_own_ground_keeps_it() {
    let paper = theme("paper");
    let p = Painter::themed(&paper, ColorDepth::TrueColor);
    let out = p.block("\x1b[7;48;5;196mhot\x1b[0m", 8);
    let span = out.split("\x1b[7;48;5;196m").nth(1).unwrap();
    assert!(span.starts_with("hot\x1b[0m"), "no theme ground inside the span: {span:?}");
    assert_eq!(visible(&out), "hot     ");
}

/// The parameters of the last SGR in `s` before `needle`.
fn sgr_before<'a>(s: &'a str, needle: &str) -> &'a str {
    let head = &s[..s.find(needle).unwrap_or_else(|| panic!("{needle:?} not in {s:?}"))];
    head.rsplit("\x1b[").next().unwrap().strip_suffix('m').unwrap_or_else(|| panic!("no SGR right before {needle:?}: {head:?}"))
}

#[test]
fn styled_text_keeps_its_style_across_a_line_break() {
    let p = Painter::themed(&theme("paper"), ColorDepth::Ansi256);
    let out = p.block("\x1b[31mred\nstill\x1b[0m\n", 8);
    assert_eq!(sgr_before(out.split('\n').nth(1).unwrap(), "still"), "31", "{out:?}");
    let own = p.block("\x1b[48;5;196mhot\nstill hot\x1b[0m", 12);
    let second = own.split('\n').nth(1).unwrap();
    assert_eq!(sgr_before(second, "still"), "48;5;196", "a span's own ground carries too: {second:?}");
    assert_eq!(visible(&own), format!("hot{}\nstill hot   ", " ".repeat(9)));
}

#[test]
fn the_padding_is_ground_alone() {
    let p = Painter::themed(&theme("paper"), ColorDepth::Ansi256);
    for open in ["7", "4", "9", "1;31", "4:3;58;5;1"] {
        let out = p.block(&format!("ab\x1b[{open}m\ncd"), 8);
        let first = out.split('\n').next().unwrap();
        let gap = &first[first.find(&format!("\x1b[{open}m")).unwrap()..first.find(' ').unwrap()];
        assert!(gap.contains(RESET), "{open} runs over the padding: {first:?}");
        assert_eq!(sgr_before(out.split('\n').nth(1).unwrap(), "cd"), open, "the next line opens with it again");
    }
}

#[test]
fn a_reset_keeps_the_parameters_after_it() {
    let p = Painter::themed(&theme("paper"), ColorDepth::Ansi256);
    assert_eq!(sgr_before(&p.block("a\x1b[0;31mred", 8), "red"), "31");
    assert_eq!(sgr_before(&p.block("a\x1b[1;0;4;31mred", 8), "red"), "4;31", "only what follows the last reset");
    assert_eq!(sgr_before(&p.block("a\x1b[38;5;0;31mred", 8), "red"), "38;5;0;31", "a 0 inside a colour is no reset");
}

#[test]
fn a_line_opens_with_the_net_style_in_one_sgr() {
    let p = Painter::themed(&theme("paper"), ColorDepth::Ansi256);
    let second = |s: &str| p.block(s, 8).split('\n').nth(1).unwrap().to_string();
    assert_eq!(sgr_before(&second("\x1b[1m\x1b[3m\x1b[31ma\nb"), "b"), "1;3;31");
    assert_eq!(sgr_before(&second("\x1b[1;31ma\x1b[22m\nb"), "b"), "31", "22 closes bold");
    assert_eq!(sgr_before(&second("\x1b[1;0;31ma\nb"), "b"), "31", "a reset closes what came before it");
    assert_eq!(sgr_before(&second("\x1b[31m\x1b[38;5;2ma\nb"), "b"), "38;5;2", "a later colour replaces the first");
    assert_eq!(sgr_before(&second("\x1b[4:3;58;5;1ma\nb"), "b"), "4:3;58;5;1", "the underline's style and colour");
    let closed = second("\x1b[1;2;3;4;5;7;8;9;31;48;5;1;58;5;1ma\x1b[22;23;24;25;27;28;29;39;49;59m\nb");
    assert!(!closed.contains("\x1b[1") && !closed.contains("48;5;1m"), "every closer closes its own: {closed:?}");
}

#[test]
fn a_style_opened_and_closed_on_every_line_costs_each_line_the_same() {
    let p = Painter::themed(&theme("paper"), ColorDepth::TrueColor);
    let n = 10_000;
    let plain = p.block(&"  red text\n".repeat(n), 80);
    let styled = "  \x1b[31mred\x1b[39m text\n".repeat(n);
    let start = std::time::Instant::now();
    let out = p.block(&styled, 80);
    let took = start.elapsed();
    assert!(out.len() < plain.len() + 3_000_000, "{} bytes over the plain band", out.len() - plain.len());
    if !cfg!(debug_assertions) {
        assert!(took < std::time::Duration::from_millis(200), "{n} lines took {took:?}");
    }
}

#[test]
fn the_default_foreground_is_the_body_colour() {
    let paper = theme("paper");
    let p = Painter::themed(&paper, ColorDepth::Ansi256);
    let body = Color::Rgb(Roles::derive(&paper).body).at(ColorDepth::Ansi256).unwrap().sgr_params(false);
    assert_eq!(sgr_before(&p.block("\x1b[31ma\x1b[39mb", 8), "b"), body);
    assert_eq!(sgr_before(&p.block("\x1b[39;31mb", 8), "b"), "39;31", "a later colour wins");
}

#[test]
fn a_carriage_return_never_reaches_the_band() {
    let p = Painter::themed(&theme("paper"), ColorDepth::TrueColor);
    let out = p.block("ab\r\nc\rd\n", 6);
    assert!(!out.contains('\r'), "{out:?}");
    assert_eq!(visible(&out), "ab    \ncd    \n");
}

#[test]
fn an_escape_sequence_is_one_zero_width_unit() {
    let p = Painter::themed(&theme("paper"), ColorDepth::TrueColor);
    for cut in ["\x1b", "\x1b[", "\x1b[3", "\x1b[38;5", "\x1b(", "\x1b]8;;http://x", "\x1bP1$r"] {
        assert_eq!(p.block(&format!("abc{cut}\nd"), 8), p.block("abc\nd", 8), "a sequence the line cuts off is dropped: {cut:?}");
    }
    assert_eq!(p.block("a\x1b[3\x1b[1mb", 8), p.block("a\x1b[1mb", 8), "a CSI that an ESC breaks into is dropped");
    for whole in ["\x1b(B", "\x1b#8", "\x1b]8;;http://x\x1b\\", "\x1b]0;title\x07", "\x1b[?25l"] {
        let plain = p.block("abc", 8);
        assert_eq!(p.block(&format!("ab{whole}c"), 8), plain.replacen("abc", &format!("ab{whole}c"), 1), "{whole:?}");
    }
}

#[test]
fn a_wide_character_at_the_row_s_end_moves_to_the_next_row() {
    let tampa = theme("tampa");
    let first = tampa.gradient.unwrap().stops()[0];
    let out = Painter::themed(&tampa, ColorDepth::TrueColor).block("abc中\n", 4);
    assert_eq!(visible(&out), "abc 中  \n");
    assert!(out.contains(&format!("{}中", bg(first))), "the next row starts the horizontal ground over: {out:?}");
}

#[test]
fn a_grapheme_is_measured_and_grounded_whole() {
    let p = Painter::themed(&theme("tampa"), ColorDepth::TrueColor);
    for g in ["👍🏽", "⚠\u{fe0f}", "👨\u{200d}👩\u{200d}👧"] {
        let mid = p.block(&format!("a{g}b"), 8);
        assert!(mid.contains(g), "no SGR inside {g:?}: {mid:?}");
        assert_eq!(visible(&mid), format!("a{g}b    "), "{g:?} is two cells");
        let edge = p.block(&format!("abc{g}"), 4);
        assert!(edge.contains(g), "no SGR inside {g:?}: {edge:?}");
        assert_eq!(visible(&edge), format!("abc {g}  "), "{g:?} does not fit the row's last cell");
    }
    assert_eq!(visible(&p.block("warn ⚠\u{fe0f} here", 20)), format!("warn ⚠\u{fe0f} here{}", " ".repeat(8)));
}

#[test]
fn empty_input_is_an_empty_band() {
    let p = Painter::themed(&theme("paper"), ColorDepth::TrueColor);
    assert_eq!(p.block("", 8), "");
    let blank = p.block("\n", 4);
    assert_eq!(visible(&blank), "    \n", "one blank line is a painted row");
}

// ── Waypoints ──────────────────────────────────────────────────

#[test]
fn waypoints_parse_from_the_bundled_themes_and_round_trip() {
    let nights = theme("miami-nights").decor.waypoints.unwrap();
    let glyphs: Vec<&str> = nights.as_slice().iter().map(|w| w.glyph).collect();
    assert_eq!(glyphs, ["🌴", "🦩", "🚤", "🐊", "🌆", "🎷", "📼", "🎹", "🌃", "🍸"]);
    assert!(nights.as_slice().iter().all(|w| w.fg.is_none()), "colour emoji carry no fg");
    assert_eq!((nights.width(), nights.of(12).glyph), (2, "🚤"), "two columns wide, cycled by cluster");
    assert!(bundled()[..7].iter().all(|t| t.decor.waypoints.is_none()), "the first seven keep the screens' set");
    for (name, want) in [
        ("terra", ["❧", "✿", "⁕", "✾", "❀", "⁂", "❁", "☼", "∿", "☙"]),
        ("crystal-mountain", ["❆", "⟡", "✶", "⧫", "❅", "✧", "⬡", "⋄", "✦", "❖"]),
        ("maximum-claude", ["✻", "✺", "❋", "✱", "✼", "❊", "✲", "✢", "✣", "✥"]),
    ] {
        let t = theme(name);
        let set = t.decor.waypoints.unwrap();
        let glyphs: Vec<&str> = set.as_slice().iter().map(|w| w.glyph).collect();
        assert_eq!((glyphs.as_slice(), set.width()), (&want[..], 1), "{name}: one column, no emoji");
        let roles = Roles::derive(&t);
        for w in set.as_slice() {
            let fg = w.fg.unwrap_or_else(|| panic!("{name}: {} has an fg", w.glyph));
            assert_eq!(roles.graphic(fg), fg, "{name}: {}'s fg reads as written", w.glyph);
        }
    }
    let mut t = theme("tampa");
    t.decor.waypoints = Waypoints::new(&[Waypoint { glyph: "◆", fg: Some(Rgb(1, 2, 3)) }, Waypoint { glyph: "\"", fg: None }]);
    let text = to_text(&t);
    assert!(text.contains("[[waypoints]]\nglyph = \"◆\"\nfg = \"#010203\"\n"), "{text}");
    assert_eq!(parse(&text).unwrap(), t);
    assert_eq!(Waypoints::new(&[Waypoint { glyph: "◆", fg: None }]).unwrap().width(), 1);
}

#[test]
fn waypoint_errors_name_the_line_and_entry() {
    let has = |src: String, needle: &str| {
        let e = validate(&src);
        assert!(e.iter().any(|x| x.to_string().contains(needle)), "{needle:?} not in {e:?}");
    };
    let base = theme_src("nord");
    let with = |tables: &str| format!("{base}\n{tables}");
    // The 1-based line of row `n` of the tables after the base theme.
    let line = |n: usize| base.lines().count() + 1 + n;
    let t = "[[waypoints]]\nglyph = \"◆\"\nbg = \"#000000\"\n";
    has(with(t), &format!("line {}: `bg` is not a waypoint key; an entry takes glyph and fg (waypoints entry 1)", line(3)));
    has(with("[[waypoints]]\nglyph = \"❤\u{fe0f}\"\n"), "carries a variation selector or a joiner, which leave its width to the font (waypoints entry 1)");
    has(with("[[waypoints]]\nglyph = \"👨\u{200d}👩\"\n"), "carries a variation selector or a joiner");
    for g in ["👍🏽", "🇺🇸", "\u{1F3FD}", "\u{1F1FA}"] {
        has(with(&format!("[[waypoints]]\nglyph = \"{g}\"\n")), "carries an emoji modifier or a regional indicator, which leave its width to the font");
    }
    has(with("[[waypoints]]\nglyph = \"ab\"\n"), "glyph `ab` is 2 graphemes; it takes one");
    has(with("[[waypoints]]\nglyph = \"字\"\n"), "glyph `字` is neither one column wide nor an emoji drawn two columns wide");
    has(with("[[waypoints]]\nglyph = \"◆\"\nfg = \"#12345\"\n"), "`#12345` is not a #rrggbb colour (waypoints entry 1, key `fg`)");
    has(with("[[waypoints]]\nfg = \"#123456\"\n"), "waypoints entry 1 has no glyph");
    has(with("waypoints = []\n"), &format!("line {}: waypoints takes 1 to 16 entries; it has none", line(1)));
    has(with(&"[[waypoints]]\nglyph = \"◆\"\n".repeat(17)), "waypoints takes 1 to 16 entries; this is entry 17");
    has(with("waypoints = \"◆\"\n"), &format!("line {}: `waypoints` is a string; it is an array of tables", line(1)));
    assert!(validate(&with(&"[[waypoints]]\nglyph = \"◆\"\n".repeat(16))).is_empty(), "sixteen is the most");
}

#[test]
fn a_waypoint_emoji_has_emoji_presentation() {
    let base = theme_src("nord");
    let glyph = |g: &str| validate(&format!("{base}\n[[waypoints]]\nglyph = {}\n", toml::Value::String(g.into())));
    for g in ["\u{1F202}", "\u{1F237}"] {
        let e = glyph(g);
        assert!(e.iter().any(|x| x.message.contains("neither one column wide nor an emoji drawn two columns wide")), "{g}: Emoji_Presentation=No: {e:?}");
    }
    assert!(!glyph("中").is_empty(), "two columns wide and no emoji");
    assert!(glyph("a\u{301}").is_empty(), "a combining sequence is one grapheme one column wide");
    for t in bundled() {
        for w in t.decor.waypoints.iter().flat_map(|w| w.as_slice()) {
            assert!(glyph(w.glyph).is_empty(), "{}: {}", t.name, w.glyph);
        }
    }
    let sets: Vec<(String, usize)> = bundled().into_iter().filter_map(|t| t.decor.waypoints.map(|w| (t.name, w.as_slice().len()))).collect();
    let ten = |n: &str| (n.to_string(), 10);
    assert_eq!(sets, [ten("miami"), ten("tampa"), ten("miami-nights"), ten("terra"), ten("crystal-mountain"), ten("maximum-claude")]);
}

#[test]
fn control_characters_are_refused_in_every_glyph() {
    let base = theme_src("nord");
    let keys = [("THEME_MARK", 1), ("THEME_ORNAMENT", 1), ("THEME_RULE_GLYPHS", 2), ("THEME_PRESENCE", 2), ("THEME_BORDER_GLYPHS", 8), ("THEME_EVENT_HEAD", 1), ("THEME_EVENT_DUE", 2)];
    for bad in ["\u{1b}", "\n", "\t", "\u{7f}", "\u{85}", "\u{2028}", "\u{2029}"] {
        for (key, count) in keys {
            let value = format!("{bad}{}", "x".repeat(count - 1));
            let e = validate(&format!("{base}\n{key}={}\n", toml::Value::String(value)));
            assert!(e.iter().any(|x| x.message.contains("is a control character, a line separator or a bidi control")), "{key} {bad:?}: {e:?}");
        }
        let e = validate(&format!("{base}\n[[waypoints]]\nglyph = {}\n", toml::Value::String(bad.into())));
        assert!(e.iter().any(|x| x.message.contains("is a control character, a line separator or a bidi control")), "waypoint {bad:?}: {e:?}");
    }
}

#[test]
fn glyph_width_follows_ratatui() {
    use ratatui_core::buffer::CellWidth;
    for g in ["a", "ｶﾞ", "\u{FF9E}", "中", "🌴", "a\u{301}", "◆"] {
        assert_eq!(crate::text::cell_width(g), g.cell_width() as usize, "{g}");
    }
    let base = theme_src("nord");
    let e = validate(&format!("{base}\nTHEME_MARK=\"ｶﾞ\"\n"));
    assert!(e.iter().any(|x| x.message.contains("is 2 columns wide; it must be 1")), "{e:?}");
    let e = validate(&format!("{base}\nTHEME_MARK=\"\u{200B}\"\n"));
    assert!(e.iter().any(|x| x.message.contains("glyph `\\u{200b}` is 0 columns wide")), "the glyph escaped: {e:?}");
}

#[test]
fn a_date_value_says_what_a_value_is() {
    let base = theme_src("nord");
    let src = format!("{base}\nTHEME_MARK=1979-05-27\n");
    let e = validate(&src);
    let line = base.lines().count() + 2;
    assert_eq!(e.iter().map(|x| x.to_string()).collect::<Vec<_>>(), [format!("line {line}: `THEME_MARK` is a date; a value is a double-quoted string")]);
}

#[test]
fn a_waypoint_fg_below_3_to_1_comes_out_lifted_and_drops_below_256_colours() {
    let mut t = theme("miami-nights");
    let faint = Rgb(0x20, 0x10, 0x40);
    t.decor.waypoints = Waypoints::new(&[Waypoint { glyph: "◆", fg: Some(faint) }, Waypoint { glyph: "▲", fg: None }]);
    let roles = Roles::derive(&t);
    assert!(roles.grounds().iter().any(|g| contrast(faint, *g) < MIN_MUTED), "the fg fails as written");
    let p = Painter::themed(&t, ColorDepth::TrueColor);
    let lifted = p.waypoints().unwrap().of(0).fg.unwrap();
    for g in roles.grounds() {
        assert!(contrast(lifted, g) >= MIN_MUTED, "{} on {} is {:.2}", lifted.hex(), g.hex(), contrast(lifted, g));
    }
    assert_eq!(p.waypoints().unwrap().of(1).fg, None, "no fg stays none");
    for depth in [ColorDepth::Ansi16, ColorDepth::NoColor] {
        let w = Painter::select(Some(&t), depth).waypoints().unwrap();
        assert_eq!((w.of(0).glyph, w.of(0).fg), ("◆", None), "{depth:?}: the glyph stays, the colour drops");
    }
}
