//! Replay data structures: events, active ways, frames, and playback state.

use crate::cmd::render::WayRow;

#[cfg(feature = "tui")]
use anyhow::Result;
#[cfg(feature = "tui")]
use crossterm::{cursor, execute, terminal};

#[cfg(feature = "tui")]
use super::drilldown::WhyIndex;

// ── Data structures ───────────────────────────────────────────

/// A way event from events.jsonl.
pub(crate) struct WayEvent {
    pub(super) ts: String,
    pub(super) event: String,
    pub(super) way: String,
    pub(super) trigger: String,
    pub(super) check: String,
}

/// An active way at a given frame.
#[derive(Clone)]
pub(crate) struct ActiveWay {
    pub(crate) id: String,
    pub(crate) trigger: String,
    pub(crate) epoch_fired: u64,
    pub(crate) token_pos: u64,
    pub(crate) check_fires: u64,
    pub(crate) is_new: bool,
    pub(crate) is_redisclosed: bool,
    pub(crate) refire_threshold_k: u64,
}

impl WayRow for ActiveWay {
    fn id(&self) -> &str { &self.id }
    fn epoch_fired(&self) -> u64 { self.epoch_fired }
    fn token_pos(&self) -> u64 { self.token_pos }
    fn trigger(&self) -> &str { &self.trigger }
    fn check_fires(&self) -> u64 { self.check_fires }
    fn refire_threshold_k(&self) -> u64 { self.refire_threshold_k }
}

/// A single frame in the replay.
pub(crate) struct Frame {
    pub(crate) epoch: u64,
    pub(crate) timestamp: String,
    pub(crate) elapsed_secs: u64,
    pub(crate) token_position_k: u64,
    pub(crate) ways: Vec<ActiveWay>,
    pub(crate) new_events: Vec<String>,
    /// Which compaction window (1-based) this frame belongs to. A long session is
    /// segmented at each `session_start` boundary; epoch/distance restart per window
    /// and the accumulated ways reset, so the latest window mirrors `ways list`. The
    /// boundary itself surfaces as a `⎯ compaction ⎯` entry in `new_events`.
    /// Set unconditionally by frame reconstruction, but only *read* by the tui
    /// header (`window W/M`), so it's dead in a non-tui build.
    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    pub(crate) window: u64,
}

/// Which view the replay shows. `Timeline` is the cumulative frame table; `WhyFired`
/// is the drill-down that joins the current frame's ways to the `SessionIntrospection`
/// model by `way_id` (ADR-154 §1 boundary — no epoch alignment).
#[cfg(feature = "tui")]
#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Timeline,
    WhyFired,
}

/// Playback state.
#[cfg(feature = "tui")]
pub(super) struct Player {
    pub(super) frames: Vec<Frame>,
    pub(super) current: usize,
    pub(super) playing: bool,
    pub(super) speed_idx: usize,
    pub(super) session_id: String,
    pub(super) project_name: String,
    pub(super) context_window_k: u64,
    /// Total compaction windows across the frames (for the `window W/M` header).
    pub(super) windows: u64,
    pub(super) term_width: u16,
    pub(super) term_height: u16,
    /// Current view; the drill-down state below is only meaningful in `WhyFired`.
    pub(super) view: View,
    /// The per-way "why" index, built lazily on first entry to the drill-down so a
    /// plain replay never pays for the model + transcript read.
    #[cfg(feature = "tui")]
    pub(super) why_index: Option<WhyIndex>,
    /// The selected way of the current frame — highlighted in the Timeline table
    /// (↑/↓ move it, Enter opens its why-fired detail) and focused in the drill-down.
    /// Shared so Enter/Tab carry the selection between the two views.
    pub(super) why_selected: usize,
    /// Scroll offset into the focused way's detail panel.
    pub(super) why_detail_scroll: usize,
    /// `live` mode re-reads the event log on a tick and follows the newest frame
    /// (ADR-154 §3). `following` is on until the user scrolls back to inspect an
    /// earlier frame; End/`G` resumes it. `events_sig` is the stat-gate: skip the
    /// re-parse while the event sources' combined (length, mtime) is unchanged.
    pub(super) live: bool,
    pub(super) following: bool,
    #[cfg(feature = "tui")]
    pub(super) events_sig: (u64, u64),
}

#[cfg(feature = "tui")]
pub(super) const SPEEDS: &[(u64, &str)] = &[
    (2000, "2.0s"),
    (1000, "1.0s"),
    (500, "0.5s"),
    (250, "0.25s"),
    (100, "0.1s"),
];

// ── Drop guard for raw terminal mode ──────────────────────────

#[cfg(feature = "tui")]
pub(super) struct TermGuard;

#[cfg(feature = "tui")]
impl TermGuard {
    pub(super) fn enter() -> Result<Self> {
        let mut stdout = std::io::stdout();
        terminal::enable_raw_mode()?;
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(Self)
    }
}

#[cfg(feature = "tui")]
impl Drop for TermGuard {
    fn drop(&mut self) {
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}
