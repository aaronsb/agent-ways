//! Hosting a screen that is not the settings shell (ADR-504 §3): how it is
//! opened, on the terminal or headless for snapshots and tests, in the look
//! the settings choose, and the bordered pane its tables sit in. The session
//! and projects screens share it.

use anyhow::{bail, Result};
use agent_theme::ColorDepth;
use agent_tui::ratatui::text::Line;
use agent_tui::ratatui::widgets::{Block, Borders};
use agent_tui::theme::{self, Palette, Shape};

/// How the screens are opened: on the terminal, or headless with keys fed
/// to the real key handler and a frame printed in the test kit's format.
#[derive(Debug, Default, Clone)]
pub struct Open {
    /// Key tokens (`agent_tui::testkit::parse_keys`).
    pub keys: Vec<String>,
    /// `WxH`: print the frame at that size.
    pub snap: Option<String>,
    /// truecolor, 256, 16 or none; the terminal's by default.
    pub depth: Option<String>,
}

impl Open {
    pub(crate) fn headless(&self) -> bool {
        !self.keys.is_empty() || self.snap.is_some()
    }
}

pub(crate) fn depth_of(s: Option<&str>) -> Result<ColorDepth> {
    Ok(match s {
        None => ColorDepth::detect(),
        Some("truecolor") => ColorDepth::TrueColor,
        Some("256") => ColorDepth::Ansi256,
        Some("16") => ColorDepth::Ansi16,
        Some("none") => ColorDepth::NoColor,
        Some(o) => bail!("--depth {o}: one of truecolor, 256, 16, none"),
    })
}

/// The palette and lozenge shape the settings choose (`theme.active`,
/// `theme.shape`, ADR-504 note of 2026-10-01), at `depth`.
pub(crate) fn look(depth: ColorDepth) -> (Palette, Shape) {
    let project = std::path::PathBuf::from(crate::util::project_dir());
    let layers = ways_core::settings::layers(&project);
    let value = |path: &[&str]| -> Option<String> {
        let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
        layers.iter().rev().find_map(|l| l.get(&path).and_then(|v| v.as_str().map(str::to_string)))
    };
    let shape = value(&["theme", "shape"]).map_or(Shape::PLAIN, |s| Shape::named(&s));
    let painter = match agent_theme::user_dir() {
        Some(dir) => agent_theme::Painter::named_in(value(&["theme", "active"]).as_deref(), &dir, depth).0,
        None => agent_theme::Painter::terminal(depth),
    };
    (Palette { painter }, shape)
}

/// Show a screen: on the terminal until it closes, or headless.
pub(crate) fn show(mut screen: impl agent_tui::screen::Screen, open: &Open) -> Result<()> {
    if !open.headless() {
        if let Some(sig) = agent_tui::screen::run_screen(&mut screen)? {
            // The terminal is restored; end as the signal would have.
            std::process::exit(128 + sig);
        }
        return Ok(());
    }
    let keys = agent_tui::testkit::parse_keys(open.keys.iter().flat_map(|k| k.split_whitespace())).map_err(|e| anyhow::anyhow!("--keys: {e}"))?;
    for k in keys {
        if !agent_tui::screen::Screen::key(&mut screen, k) {
            break;
        }
    }
    if let Some(size) = &open.snap {
        let (w, h) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .ok_or_else(|| anyhow::anyhow!("--snap {size}: WIDTHxHEIGHT, such as 100x30"))?;
        print!("{}", agent_tui::testkit::frame(&agent_tui::testkit::render_screen(&mut screen, w, h)));
    }
    Ok(())
}

/// A bordered pane in the theme, as the settings screens draw theirs.
pub(crate) fn pane(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default().borders(Borders::ALL).border_style(theme::rule()).title(title).title_style(theme::title())
}
