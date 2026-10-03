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
    /// Key and mouse tokens (`agent_tui::testkit::parse_events`).
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

/// Show a screen on agent-tui's shell: on the terminal until it closes,
/// or headless, its key and mouse script played through the real
/// handlers with a frame drawn before each event.
pub(crate) fn show(mut app: agent_tui::App, open: &Open) -> Result<()> {
    if !open.headless() {
        let session = agent_tui::run(app)?;
        if let Some(sig) = session.signal {
            // The terminal is restored; end as the signal would have.
            std::process::exit(128 + sig);
        }
        return Ok(());
    }
    let events = agent_tui::testkit::parse_events(open.keys.iter().flat_map(|k| k.split_whitespace())).map_err(|e| anyhow::anyhow!("--keys: {e}"))?;
    let size = match &open.snap {
        Some(size) => Some(
            size.split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
                .filter(|(w, h)| *w > 0 && *h > 0)
                .ok_or_else(|| anyhow::anyhow!("--snap {size}: WIDTHxHEIGHT, such as 100x30"))?,
        ),
        None => None,
    };
    // A frame before each event, as the terminal draws one before it reads
    // the next and as the settings shell's `--keys` does: what an event
    // does can depend on what was drawn, such as a page's height or where
    // a row sits for a click.
    agent_tui::testkit::play(&mut app, &events, size);
    if let Some((w, h)) = size {
        print!("{}", agent_tui::testkit::frame(&agent_tui::testkit::render(&mut app, w, h)));
    }
    Ok(())
}

/// A bordered pane in the theme, as the settings screens draw theirs.
pub(crate) fn pane(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default().borders(Borders::ALL).border_style(theme::rule()).title(title).title_style(theme::title())
}
