//! Other programs' themes, made from a theme's roles (ADR-513). Each target
//! maps roles to its own keys; a theme pins any of those keys in its
//! `[target.<name>]` table, and a pin wins over the mapping.
//!
//! The one target is Claude Code, whose custom theme is a JSON file of
//! `name`, `base` and `overrides` by colour token (code.claude.com,
//! terminal-config, "Create a custom theme").

use crate::derive::{contrast, Roles, MIN_MUTED, MIN_TEXT};
use crate::model::{Kind, Rgb, Theme};

/// Every Claude Code colour token the custom theme reference lists.
pub const CLAUDE_CODE_KEYS: &[&str] = &[
    "claude", "text", "inverseText", "inactive", "subtle", "suggestion", "permission", "remember",
    "success", "error", "warning", "merged",
    "promptBorder", "planMode", "autoAccept", "bashBorder", "ide", "fastMode", "effortUltra",
    "diffAdded", "diffRemoved", "diffAddedDimmed", "diffRemovedDimmed", "diffAddedWord", "diffRemovedWord",
    "userMessageBackground", "userMessageBackgroundHover", "bashMessageBackgroundColor", "memoryBackgroundColor",
    "selectionBg", "composerSidebarBackground",
    "rate_limit_fill", "rate_limit_empty", "briefLabelYou", "briefLabelClaude",
    "claudeShimmer", "warningShimmer", "permissionShimmer", "promptBorderShimmer", "inactiveShimmer", "fastModeShimmer",
    "red_FOR_SUBAGENTS_ONLY", "blue_FOR_SUBAGENTS_ONLY", "green_FOR_SUBAGENTS_ONLY", "yellow_FOR_SUBAGENTS_ONLY",
    "purple_FOR_SUBAGENTS_ONLY", "orange_FOR_SUBAGENTS_ONLY", "pink_FOR_SUBAGENTS_ONLY", "cyan_FOR_SUBAGENTS_ONLY",
    "rainbow_red", "rainbow_orange", "rainbow_yellow", "rainbow_green", "rainbow_blue", "rainbow_indigo", "rainbow_violet",
    "rainbow_red_shimmer", "rainbow_orange_shimmer", "rainbow_yellow_shimmer", "rainbow_green_shimmer",
    "rainbow_blue_shimmer", "rainbow_indigo_shimmer", "rainbow_violet_shimmer",
];

/// The targets a theme file may name, each with the keys it takes.
pub const TARGETS: [(&str, &[&str]); 1] = [("claude-code", CLAUDE_CODE_KEYS)];

/// The keys `target` takes; none for a target agent-ways does not know.
pub fn keys(target: &str) -> Option<&'static [&'static str]> {
    TARGETS.iter().find(|(n, _)| *n == target).map(|(_, k)| *k)
}

/// Claude Code draws on the terminal's own background, which it does not
/// paint and agent-ways cannot know. A theme's text roles are lifted to
/// read on the theme's bg, where a terminal set from the same palette sits,
/// and on this band of common terminal grounds of the theme's kind.
pub fn ground_band(kind: Kind) -> [Rgb; 2] {
    match kind {
        Kind::Dark => [Rgb(0x00, 0x00, 0x00), Rgb(0x2a, 0x2a, 0x2a)],
        Kind::Light => [Rgb(0xff, 0xff, 0xff), Rgb(0xe8, 0xe8, 0xe8)],
    }
}

/// What an exported colour must read against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Floor {
    /// Text on the terminal: this ratio on the theme's bg, its selection
    /// ground and the ground band.
    Ground(f64),
    /// A background Claude Code paints: the body text reads on it at this ratio.
    Under(f64),
    /// Text on one painted colour, at this ratio.
    On(Rgb, f64),
    /// No floor: decoration whose base token carries the reading.
    None,
}

/// A Claude Code custom theme.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeCode {
    /// The label `/theme` lists.
    pub name: String,
    /// The built-in preset it starts from: `dark` or `light`.
    pub base: &'static str,
    /// Each token set, in mapping order, then the theme's pins for tokens
    /// the mapping leaves to the base.
    pub overrides: Vec<(&'static str, Rgb)>,
}

/// The roles a Claude Code theme is drawn from: the theme's own, with no
/// gradient, which Claude Code does not paint, and lifted onto the band.
pub fn claude_code_roles(t: &Theme) -> Roles {
    let flat = Theme { gradient: None, ..t.clone() };
    Roles::derive_on(&flat, &ground_band(t.kind))
}

/// `c` shaded `pct` percent over `bg`, less where the body text would not
/// read on it at `min`.
fn shade(c: Rgb, bg: Rgb, pct: u32, body: Rgb, min: f64) -> Rgb {
    (0..=pct).rev().map(|p| Rgb::blend(c, bg, p)).find(|x| contrast(body, *x) >= min).unwrap_or(bg)
}

/// The default mapping from roles to Claude Code tokens, each with its floor.
pub fn claude_code_map(r: &Roles) -> Vec<(&'static str, Rgb, Floor)> {
    let text = Floor::Ground(MIN_TEXT);
    let under = |c: Rgb, pct: u32, min: f64| (shade(c, r.bg, pct, r.body, min), Floor::Under(min));
    // A shimmer is the lighter colour in the spinner's moving highlight,
    // passing over its base token, which carries the reading: decoration.
    let shimmer = |c: Rgb| (Rgb::blend(c, r.text, 50), Floor::None);
    let (added, removed) = (under(r.ok, 25, MIN_TEXT), under(r.err, 25, MIN_TEXT));
    let (added_dim, removed_dim) = (under(r.ok, 12, MIN_MUTED), under(r.err, 12, MIN_MUTED));
    let (added_word, removed_word) = (under(r.ok, 45, MIN_MUTED), under(r.err, 45, MIN_MUTED));
    let hover = under(r.accent, 20, MIN_TEXT);
    let bash_bg = under(r.hot, 13, MIN_TEXT);
    let memory_bg = under(r.alt, 13, MIN_TEXT);
    let rows: [(&'static str, (Rgb, Floor)); 35] = [
        ("claude", (r.accent, text)),
        ("text", (r.body, text)),
        ("inverseText", (r.badge.fg, Floor::On(r.badge.bg, MIN_TEXT))),
        ("inactive", (r.muted, Floor::Ground(MIN_MUTED))),
        // Faint borders, as the base presets' own subtle is.
        ("subtle", (r.rule, Floor::None)),
        ("suggestion", (r.info, text)),
        ("permission", (r.info, text)),
        ("remember", (r.alt, text)),
        ("success", (r.ok, text)),
        ("error", (r.err, text)),
        ("warning", (r.warn, text)),
        ("merged", (r.alt, text)),
        ("promptBorder", (r.accent_dim, text)),
        ("planMode", (r.info, text)),
        ("autoAccept", (r.alt, text)),
        ("bashBorder", (r.hot, text)),
        ("ide", (r.info, text)),
        ("fastMode", (r.hot, text)),
        ("briefLabelYou", (r.info, text)),
        ("briefLabelClaude", (r.accent, text)),
        ("diffAdded", added),
        ("diffRemoved", removed),
        ("diffAddedDimmed", added_dim),
        ("diffRemovedDimmed", removed_dim),
        ("diffAddedWord", added_word),
        ("diffRemovedWord", removed_word),
        ("userMessageBackground", (r.selection_bg, Floor::Under(MIN_TEXT))),
        ("userMessageBackgroundHover", hover),
        ("bashMessageBackgroundColor", bash_bg),
        ("memoryBackgroundColor", memory_bg),
        ("selectionBg", (r.selection_bg, Floor::Under(MIN_TEXT))),
        ("composerSidebarBackground", (r.bg, Floor::Under(MIN_TEXT))),
        ("claudeShimmer", shimmer(r.accent)),
        ("warningShimmer", shimmer(r.warn)),
        ("promptBorderShimmer", shimmer(r.accent_dim)),
    ];
    let more = [("permissionShimmer", shimmer(r.info)), ("inactiveShimmer", shimmer(r.muted)), ("fastModeShimmer", shimmer(r.hot))];
    rows.into_iter().chain(more).map(|(k, (c, f))| (k, c, f)).collect()
}

/// `t` as a Claude Code custom theme: the mapping, then the theme's pins.
pub fn claude_code(t: &Theme) -> ClaudeCode {
    let mut overrides: Vec<(&'static str, Rgb)> = claude_code_map(&claude_code_roles(t)).into_iter().map(|(k, c, _)| (k, c)).collect();
    for (key, c) in t.targets.pins("claude-code") {
        let Some(k) = CLAUDE_CODE_KEYS.iter().find(|k| **k == key) else { continue };
        match overrides.iter_mut().find(|(o, _)| o == k) {
            Some(slot) => slot.1 = c,
            None => overrides.push((k, c)),
        }
    }
    let base = match t.kind {
        Kind::Dark => "dark",
        Kind::Light => "light",
    };
    ClaudeCode { name: t.label.clone(), base, overrides }
}

/// Every exported colour of `t` below its floor, as (token, colour, ground,
/// ratio, floor). A pin is exported as given and checked at its token's
/// floor in the mapping; a pin of a token the mapping leaves to the base
/// has no floor here.
pub fn claude_code_unreadable(t: &Theme) -> Vec<(&'static str, Rgb, Rgb, f64, f64)> {
    let r = claude_code_roles(t);
    let mut grounds = vec![r.bg, r.selection_bg];
    grounds.extend(ground_band(t.kind));
    let floors: Vec<(&'static str, Floor)> = claude_code_map(&r).into_iter().map(|(k, _, f)| (k, f)).collect();
    let exported = claude_code(t).overrides;
    let body = exported.iter().find(|(k, _)| *k == "text").map_or(r.body, |(_, c)| *c);
    let mut out = Vec::new();
    for (k, c) in exported {
        let Some(&(_, floor)) = floors.iter().find(|(f, _)| *f == k) else { continue };
        let checks: Vec<(Rgb, Rgb, f64)> = match floor {
            Floor::Ground(min) => grounds.iter().map(|g| (c, *g, min)).collect(),
            Floor::Under(min) => vec![(body, c, min)],
            Floor::On(g, min) => vec![(c, g, min)],
            Floor::None => vec![],
        };
        for (fg, g, min) in checks {
            let ratio = contrast(fg, g);
            if ratio < min {
                out.push((k, fg, g, ratio, min));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled::ThemeSet;
    use crate::text::{parse, to_text};

    fn bundled() -> Vec<Theme> {
        ThemeSet::bundled().list().map(|(t, _)| t.clone()).collect()
    }

    const PALETTE: &str = r##"THEME_NAME="probe"
THEME_KIND="dark"
THEME_BG="#101010"
THEME_FG="#e0e0e0"
THEME_DIM="#808080"
THEME_SUBTLE="#303030"
THEME_ACCENT="#ff2a8a"
THEME_INFO="#2de2e6"
THEME_OK="#3dffa2"
THEME_WARN="#ffd319"
THEME_ERR="#ff5b3a"
THEME_ALT="#b967ff"
"##;

    #[test]
    fn every_bundled_theme_reads_on_the_ground_band() {
        for t in bundled() {
            let bad = claude_code_unreadable(&t);
            assert!(bad.is_empty(), "{}: {:?}", t.name, bad);
        }
        // The check sees a failure: the body is taken as given, not lifted.
        let dim = parse(&PALETTE.replace("#e0e0e0", "#303030")).unwrap();
        assert!(claude_code_unreadable(&dim).iter().any(|(k, ..)| *k == "text"));
    }

    #[test]
    fn a_pin_below_its_floor_is_reported() {
        // The pin is exported as given, and the check reads what is exported.
        let t = parse(&format!("{PALETTE}\n[target.claude-code]\ntext=\"#151515\"\n")).unwrap();
        assert_eq!(claude_code(&t).overrides.iter().find(|(k, _)| *k == "text").unwrap().1, Rgb(0x15, 0x15, 0x15));
        assert!(claude_code_unreadable(&t).iter().any(|(k, c, ..)| *k == "text" && *c == Rgb(0x15, 0x15, 0x15)), "{:?}", claude_code_unreadable(&t));
        // A readable pin passes.
        let fine = parse(&format!("{PALETTE}\n[target.claude-code]\ntext=\"#f0f0f0\"\n")).unwrap();
        assert!(claude_code_unreadable(&fine).is_empty(), "{:?}", claude_code_unreadable(&fine));
    }

    #[test]
    fn the_mapping_names_claude_code_keys_once_each() {
        let map = claude_code_map(&claude_code_roles(&bundled()[0]));
        for (i, (k, _, _)) in map.iter().enumerate() {
            assert!(CLAUDE_CODE_KEYS.contains(k), "{k}");
            assert!(map[i + 1..].iter().all(|(o, _, _)| o != k), "{k} twice");
        }
    }

    #[test]
    fn base_follows_the_kind() {
        for t in bundled() {
            let want = if t.kind == Kind::Dark { "dark" } else { "light" };
            assert_eq!(claude_code(&t).base, want, "{}", t.name);
        }
    }

    #[test]
    fn bundled_themes_carry_no_target_tables() {
        // A table line does not source as shell; the bundled files still do.
        for t in bundled() {
            assert!(t.targets.is_empty(), "{}", t.name);
        }
    }

    #[test]
    fn a_pin_wins_over_the_mapping_and_round_trips() {
        let src = format!("{PALETTE}\n[target.claude-code]\nclaude=\"#123456\"\nrainbow_red=\"#abcdef\"\n");
        let t = parse(&src).unwrap();
        let cc = claude_code(&t);
        assert_eq!(cc.overrides.iter().find(|(k, _)| *k == "claude").unwrap().1, Rgb(0x12, 0x34, 0x56));
        assert_eq!(cc.overrides.last().unwrap(), &("rainbow_red", Rgb(0xab, 0xcd, 0xef)));
        assert_eq!(parse(&to_text(&t)).unwrap(), t);
    }

    #[test]
    fn a_target_table_is_checked() {
        let cases = [
            ("[target.vim]\nx=\"#000000\"\n", 14, "unknown target `vim`"),
            ("[target.claude-code]\nclaud=\"#000000\"\n", 15, "`claud` is not a claude-code key"),
            ("[target.claude-code]\nclaude=\"red\"\n", 15, "`red` is not a #rrggbb colour"),
            ("[target.claude-code]\nclaude=1\n", 15, "must be a double-quoted"),
            ("[elsewhere]\nx=\"#000000\"\n", 14, "`elsewhere` is a table"),
        ];
        for (table, line, msg) in cases {
            let errs = parse(&format!("{PALETTE}\n{table}")).unwrap_err();
            assert!(errs.iter().any(|e| e.line == Some(line) && e.message.contains(msg)), "{table}: {errs:?}");
        }
    }

    #[test]
    fn a_plain_palette_exports_with_no_pins() {
        let t = parse(PALETTE).unwrap();
        assert!(t.targets.is_empty());
        assert_eq!(claude_code(&t).overrides.len(), claude_code_map(&claude_code_roles(&t)).len());
    }
}
