//! Interactive session replay — animate `ways list` state across a session's history.
//!
//! Reconstructs the progressive disclosure timeline from events.jsonl,
//! building cumulative frames at each epoch. Renders each frame using
//! the same visual format as `ways list`, with interactive controls.

#[cfg(feature = "tui")]
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};

use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::io::Write;

use crate::cmd::render::{self, WayRow};
use crate::session;
use crate::util::{detect_project_dir, home_dir, parse_ts_secs};

#[cfg(feature = "tui")]
use crate::cmd::compositor::{self, Panel};
#[cfg(feature = "tui")]
use ways_core::introspection::{MatchCriteria, SessionIntrospection};

// ── Data structures ───────────────────────────────────────────

/// A way event from events.jsonl.
pub(crate) struct WayEvent {
    ts: String,
    event: String,
    way: String,
    trigger: String,
    check: String,
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
    pub(crate) window: u64,
}

/// Which view the replay shows. `Timeline` is the cumulative frame table; `WhyFired`
/// is the drill-down that joins the current frame's ways to the `SessionIntrospection`
/// model by `way_id` (ADR-154 §1 boundary — no epoch alignment).
#[derive(Clone, Copy, PartialEq)]
enum View {
    Timeline,
    WhyFired,
}

/// Playback state.
struct Player {
    frames: Vec<Frame>,
    current: usize,
    playing: bool,
    speed_idx: usize,
    session_id: String,
    project_name: String,
    context_window_k: u64,
    /// Total compaction windows across the frames (for the `window W/M` header).
    windows: u64,
    term_width: u16,
    term_height: u16,
    /// Current view; the drill-down state below is only meaningful in `WhyFired`.
    view: View,
    /// The per-way "why" index, built lazily on first entry to the drill-down so a
    /// plain replay never pays for the model + transcript read.
    #[cfg(feature = "tui")]
    why_index: Option<WhyIndex>,
    /// The selected way of the current frame — highlighted in the Timeline table
    /// (↑/↓ move it, Enter opens its why-fired detail) and focused in the drill-down.
    /// Shared so Enter/Tab carry the selection between the two views.
    why_selected: usize,
    /// Scroll offset into the focused way's detail panel.
    why_detail_scroll: usize,
    /// `live` mode re-reads the event log on a tick and follows the newest frame
    /// (ADR-154 §3). `following` is on until the user scrolls back to inspect an
    /// earlier frame; End/`G` resumes it. `events_sig` is the stat-gate: skip the
    /// re-parse while the event sources' combined (length, mtime) is unchanged.
    live: bool,
    following: bool,
    #[cfg(feature = "tui")]
    events_sig: (u64, u64),
}

const SPEEDS: &[(u64, &str)] = &[
    (2000, "2.0s"),
    (1000, "1.0s"),
    (500, "0.5s"),
    (250, "0.25s"),
    (100, "0.1s"),
];

// ── Drop guard for raw terminal mode ──────────────────────────

#[cfg(feature = "tui")]
struct TermGuard;

#[cfg(feature = "tui")]
impl TermGuard {
    fn enter() -> Result<Self> {
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

// ── Project scoping ───────────────────────────────────────────

/// Resolve which project(s) to replay. `Ok(None)` means *every* project
/// (`--all`); `Ok(Some(path))` scopes to one. Defaults to the current project
/// and — the correctness fix (ADR-154 §4) — **fails loud** instead of silently
/// globalizing when the current project can't be detected.
pub(crate) fn resolve_project_scope(project: Option<&str>, all: bool) -> Result<Option<String>> {
    if all {
        return Ok(None);
    }
    if let Some(p) = project {
        return Ok(Some(p.to_string()));
    }
    match std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .or_else(detect_project_dir)
    {
        Some(p) => Ok(Some(p)),
        None => bail!(
            "couldn't detect the current project: CLAUDE_PROJECT_DIR is unset and no \
             .claude/settings.json or CLAUDE.md was found above the working directory. \
             Pass --project <path> to scope to a project, or --all to replay across every project."
        ),
    }
}

/// Whether an event's stored `project` path belongs to `scope`, compared as
/// normalized absolute paths — replacing the old loose `contains` substring test
/// that let unrelated projects (`/a/foo` vs `/a/foo-bar`) bleed together.
///
/// The comparison is exact (modulo trailing slash). On the primary path this is
/// right: `CLAUDE_PROJECT_DIR` is the scope at both write and read time, so the
/// strings match. On a *manual* run where scope falls back to `detect_project_dir`
/// (a symlink-resolved cwd), a session whose stored `project` was a logical or
/// symlinked path — or a subdirectory `$PWD` — won't match and is simply absent
/// from the list (not an error). Pass `--all` or an explicit `--project` to see it.
pub(crate) fn project_matches(stored: &str, scope: &str) -> bool {
    normalize_project_path(stored) == normalize_project_path(scope)
}

fn normalize_project_path(p: &str) -> String {
    p.trim_end_matches('/').to_string()
}

// ── Entry point ───────────────────────────────────────────────

#[cfg(feature = "tui")]
pub fn run(
    session: Option<&str>,
    project: Option<&str>,
    speed: Option<u64>,
    list: bool,
    all: bool,
) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }

    let project_scope = resolve_project_scope(project, all)?;

    if list {
        return list_sessions(&content, project_scope.as_deref());
    }

    // Find the session: explicit > interactive picker
    let session_id = match session {
        Some(s) => s.to_string(),
        None => {
            match pick_session(&content, project_scope.as_deref()) {
                Some(s) => s,
                None => return Ok(()),
            }
        }
    };

    let project_name = find_session_project(&content, &session_id)
        .unwrap_or_else(|| "unknown".to_string());

    // Load and build frames
    let events = load_session_events(&content, &session_id);
    if events.is_empty() {
        println!("No events found for session {}", &session_id[..session_id.len().min(12)]);
        return Ok(());
    }

    let context_window = session::detect_context_window_for(&project_name, &session_id);
    let context_window_k = context_window / 1000;
    let frames = reconstruct_frames(&events, &project_name, &session_id, context_window);

    if frames.is_empty() {
        println!("No frames to replay.");
        return Ok(());
    }
    let windows = frames.iter().map(|f| f.window).max().unwrap_or(1);

    let speed_idx = match speed {
        Some(ms) => SPEEDS.iter().position(|(s, _)| *s <= ms).unwrap_or(1),
        None => 1,
    };

    let (term_width, term_height) = terminal::size().unwrap_or((120, 40));

    let mut player = Player {
        frames,
        current: 0,
        playing: false,
        speed_idx,
        session_id,
        project_name,
        context_window_k,
        windows,
        term_width,
        term_height,
        view: View::Timeline,
        why_index: None,
        why_selected: 0,        why_detail_scroll: 0,
        live: false,
        following: false,
        events_sig: (0, 0),
    };

    run_tui(&mut player)
}

#[cfg(not(feature = "tui"))]
pub fn run(
    _session: Option<&str>,
    project: Option<&str>,
    _speed: Option<u64>,
    list: bool,
    all: bool,
) -> Result<()> {
    if list {
        let content = ways_core::firing::load_events_text();
        if content.trim().is_empty() {
            println!("No events recorded yet.");
            return Ok(());
        }
        let scope = resolve_project_scope(project, all)?;
        return list_sessions(&content, scope.as_deref());
    }
    println!("Rethink requires the 'tui' feature. Build with: cargo build --features tui");
    Ok(())
}

// ── Live monitor (ADR-154 §3) ─────────────────────────────────

/// `ways introspect live` — monitor `session_id` as new ways fire, following the
/// newest frame. Reuses the replay TUI; the loop re-reads the event log on a tick,
/// gated by a stat check (§3). `project` is the resolved scope (the session's own
/// recorded project takes precedence when present).
#[cfg(feature = "tui")]
pub fn run_live(session_id: &str, project: Option<&str>, speed: Option<u64>) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    let project_name = find_session_project(&content, session_id)
        .or_else(|| project.map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());

    let events = load_session_events(&content, session_id);
    if events.is_empty() {
        println!("No events for the current session yet.");
        return Ok(());
    }

    let context_window = session::detect_context_window_for(&project_name, session_id);
    let context_window_k = context_window / 1000;
    let frames = reconstruct_frames(&events, &project_name, session_id, context_window);
    if frames.is_empty() {
        println!("No frames to monitor yet.");
        return Ok(());
    }
    let windows = frames.iter().map(|f| f.window).max().unwrap_or(1);

    let speed_idx = match speed {
        Some(ms) => SPEEDS.iter().position(|(s, _)| *s <= ms).unwrap_or(1),
        None => 1,
    };
    let (term_width, term_height) = terminal::size().unwrap_or((120, 40));

    let last = frames.len() - 1;
    let mut player = Player {
        frames,
        current: last, // start pinned to the newest frame
        playing: false,
        speed_idx,
        session_id: session_id.to_string(),
        project_name,
        context_window_k,
        windows,
        term_width,
        term_height,
        view: View::Timeline,
        why_index: None,
        why_selected: 0,        why_detail_scroll: 0,
        live: true,
        following: true,
        events_sig: events_signature(),
    };
    run_tui(&mut player)
}

#[cfg(not(feature = "tui"))]
pub fn run_live(_session_id: &str, _project: Option<&str>, _speed: Option<u64>) -> Result<()> {
    println!("Live monitor requires the 'tui' feature. Build with: cargo build --features tui");
    Ok(())
}

/// The stat-gate signature over the event sources: combined byte length and the
/// newest mtime (seconds). A change in either means new events to re-read; equality
/// means the append-only log is untouched, so the live loop skips the re-parse.
#[cfg(feature = "tui")]
fn events_signature() -> (u64, u64) {
    let mut total_len = 0u64;
    let mut max_mtime = 0u64;
    for p in ways_core::paths::events_log_sources() {
        if let Ok(meta) = std::fs::metadata(&p) {
            total_len = total_len.saturating_add(meta.len());
            if let Ok(secs) = meta
                .modified()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other))
            {
                max_mtime = max_mtime.max(secs.as_secs());
            }
        }
    }
    (total_len, max_mtime)
}

/// Re-read the live session's events if the stat-gate shows a change, rebuild
/// frames, and (when following) pin to the newest frame. Invalidates the drill-down
/// index so its "why" panels rebuild against the fresh frames. A no-op while the log
/// is unchanged, so idle ticks are cheap.
#[cfg(feature = "tui")]
fn refresh_live(player: &mut Player) {
    let sig = events_signature();
    if sig == player.events_sig {
        return;
    }
    player.events_sig = sig;

    let content = ways_core::firing::load_events_text();
    let events = load_session_events(&content, &player.session_id);
    if events.is_empty() {
        return;
    }
    let context_window = player.context_window_k * 1000;
    let frames = reconstruct_frames(&events, &player.project_name, &player.session_id, context_window);
    if frames.is_empty() {
        return;
    }

    let was_last = player.current + 1 >= player.frames.len();
    player.frames = frames;
    player.windows = player.frames.iter().map(|f| f.window).max().unwrap_or(1);
    let last = player.frames.len() - 1;
    // Follow the newest frame unless the user has scrolled back to inspect history.
    player.current = if player.following || was_last {
        last
    } else {
        player.current.min(last)
    };
    player.why_index = None; // fresh frames → rebuild the drill-down index lazily
}

// ── TUI loop ──────────────────────────────────────────────────

#[cfg(feature = "tui")]
fn run_tui(player: &mut Player) -> Result<()> {
    let _guard = TermGuard::enter()?;
    tui_loop(player)
}

#[cfg(feature = "tui")]
fn tui_loop(player: &mut Player) -> Result<()> {
    let mut stdout = std::io::stdout();

    loop {
        if let Ok((w, h)) = terminal::size() {
            player.term_width = w;
            player.term_height = h;
        }

        let raw_output = match player.view {
            View::Timeline => render_frame(player),
            View::WhyFired => render_why(player),
        };
        let output = fit_to_terminal(&raw_output, player.term_width as usize, player.term_height as usize);
        execute!(stdout, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All))?;
        write!(stdout, "{output}")?;
        stdout.flush()?;

        let timeout = if player.live {
            std::time::Duration::from_millis(LIVE_REFRESH_MS)
        } else if player.playing {
            std::time::Duration::from_millis(SPEEDS[player.speed_idx].0)
        } else {
            std::time::Duration::from_secs(60)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // Global keys, then per-view dispatch.
                match key {
                    KeyEvent { code: KeyCode::Esc, .. }
                    | KeyEvent { code: KeyCode::Char('q'), .. } => break,

                    KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } => break,

                    // Tab toggles the why-fired drill-down, carrying the current
                    // selection with it (like Enter). Entering it pauses playback.
                    KeyEvent { code: KeyCode::Tab, .. } => {
                        player.view = match player.view {
                            View::Timeline => View::WhyFired,
                            View::WhyFired => View::Timeline,
                        };
                        player.playing = false;
                        player.why_detail_scroll = 0;
                    }

                    _ => match player.view {
                        View::Timeline => handle_timeline_key(player, key),
                        View::WhyFired => handle_why_key(player, key),
                    },
                }
            }
        } else if player.playing && !player.live {
            if player.current < player.frames.len() - 1 {
                player.current += 1;
            } else {
                player.playing = false;
            }
        }

        // Live mode re-reads the (append-only) event log each tick; the stat-gate
        // in refresh_live makes an unchanged log a cheap no-op.
        if player.live {
            refresh_live(player);
        }
    }
    Ok(())
}

/// The live-monitor refresh interval — the `poll` timeout in live mode (ADR-154 §3
/// suggests ~100–250 ms; imperceptible, and the stat-gate keeps idle ticks cheap).
#[cfg(feature = "tui")]
const LIVE_REFRESH_MS: u64 = 250;

/// The currently selected way's `(id, epoch_fired)` — the anchor carried across a
/// frame change so the cursor stays on the same way instead of jumping to row 0.
#[cfg(feature = "tui")]
fn current_selected_way(player: &Player) -> Option<(String, u64)> {
    let frame = player.frames.get(player.current)?;
    if frame.ways.is_empty() {
        return None;
    }
    let idx = player.why_selected.min(frame.ways.len() - 1);
    frame.ways.get(idx).map(|w| (w.id.clone(), w.epoch_fired))
}

/// The row in `frame` that best preserves an anchor across a frame change: the same
/// way if it's still active, else the nearest still-active way with `epoch_fired ≤`
/// the anchor's (frame.ways is epoch-ascending, so that's the last such row), else
/// the first row. Moving forward the anchor way is always present (frames are
/// cumulative); moving backward it can vanish, and we snap to the nearest lesser epoch.
#[cfg(feature = "tui")]
fn reselect_by_anchor(frame: &Frame, anchor_id: &str, anchor_epoch: u64) -> usize {
    if let Some(i) = frame.ways.iter().position(|w| w.id == anchor_id) {
        return i;
    }
    frame
        .ways
        .iter()
        .rposition(|w| w.epoch_fired <= anchor_epoch)
        .unwrap_or(0)
}

/// Move to frame `target` (clamped), preserving the selected way via its anchor.
/// Shared by the Timeline table and the drill-down so both keep the cursor put.
#[cfg(feature = "tui")]
fn jump_frame(player: &mut Player, target: usize) {
    let anchor = current_selected_way(player);
    let last = player.frames.len().saturating_sub(1);
    player.current = target.min(last);
    player.why_selected = match anchor {
        Some((id, ep)) => reselect_by_anchor(&player.frames[player.current], &id, ep),
        None => 0,
    };
}

/// Timeline-view keys. ▲▼ select a way in the current frame (the viewport follows);
/// Enter opens its why-fired detail. ◀▶/Home/End move along the time axis, keeping
/// the cursor on the same way (nearest lesser epoch when it's rewound out of view).
/// `+`/`-` set playback speed (the arrows now select, not speed). In live mode,
/// manual navigation drops out of follow-the-newest; Space toggles following, and
/// End/`G` resumes it.
#[cfg(feature = "tui")]
fn handle_timeline_key(player: &mut Player, key: KeyEvent) {
    let ways_len = player.frames[player.current].ways.len();
    match key {
        // Row selection within the current frame.
        KeyEvent { code: KeyCode::Up, .. } | KeyEvent { code: KeyCode::Char('k'), .. } => {
            player.why_selected = player.why_selected.saturating_sub(1);
        }
        KeyEvent { code: KeyCode::Down, .. } | KeyEvent { code: KeyCode::Char('j'), .. } => {
            if player.why_selected + 1 < ways_len {
                player.why_selected += 1;
            }
        }
        KeyEvent { code: KeyCode::PageUp, .. } => {
            player.why_selected = player.why_selected.saturating_sub(10);
        }
        KeyEvent { code: KeyCode::PageDown, .. } => {
            player.why_selected = (player.why_selected + 10).min(ways_len.saturating_sub(1));
        }
        // Enter drills into the selected way's why-fired detail.
        KeyEvent { code: KeyCode::Enter, .. } => {
            if ways_len > 0 {
                player.view = View::WhyFired;
                player.playing = false;
                player.why_detail_scroll = 0;
            }
        }
        // Frame navigation (time axis); keeps the cursor on the same way.
        KeyEvent { code: KeyCode::Right, .. } | KeyEvent { code: KeyCode::Char('l'), .. } => {
            player.playing = false;
            player.following = false;
            jump_frame(player, player.current + 1);
        }
        KeyEvent { code: KeyCode::Left, .. } | KeyEvent { code: KeyCode::Char('h'), .. } => {
            player.playing = false;
            player.following = false;
            jump_frame(player, player.current.saturating_sub(1));
        }
        KeyEvent { code: KeyCode::Home, .. } | KeyEvent { code: KeyCode::Char('g'), .. } => {
            player.playing = false;
            player.following = false;
            jump_frame(player, 0);
        }
        KeyEvent { code: KeyCode::End, .. } | KeyEvent { code: KeyCode::Char('G'), .. } => {
            player.playing = false;
            player.following = player.live; // resume following the live tail
            jump_frame(player, usize::MAX);
        }
        KeyEvent { code: KeyCode::Char(' '), .. } => {
            // Live: pause/resume following. Replay: play/pause the animation.
            if player.live {
                player.following = !player.following;
                if player.following {
                    jump_frame(player, usize::MAX); // to newest, keeping the selected way
                }
            } else {
                player.playing = !player.playing;
            }
        }
        // Playback speed (moved off the arrows, which now select rows).
        KeyEvent { code: KeyCode::Char('+'), .. } | KeyEvent { code: KeyCode::Char('='), .. } => {
            if player.speed_idx < SPEEDS.len() - 1 {
                player.speed_idx += 1;
            }
        }
        KeyEvent { code: KeyCode::Char('-'), .. } | KeyEvent { code: KeyCode::Char('_'), .. } => {
            if player.speed_idx > 0 {
                player.speed_idx -= 1;
            }
        }
        _ => {}
    }
}

/// Drill-down keys. ▲▼ select a fired way (resets the reader to the top of its
/// document); `j`/`k` scroll the selected way's document within the detail window
/// (vim-style, the same pattern used to read subagent output); PgUp/PgDn page it,
/// `g`/`G` jump to top/bottom; ◀▶ / `h` `l` move between frames.
#[cfg(feature = "tui")]
fn handle_why_key(player: &mut Player, key: KeyEvent) {
    let ways_len = player.frames[player.current].ways.len();
    // A near-full page for PgUp/PgDn, sized to the detail window.
    let page = (player.term_height as usize).saturating_sub(9).max(1);
    match key {
        // Select a way (arrows only, so j/k stay free for reading the document).
        KeyEvent { code: KeyCode::Up, .. } => {
            player.why_selected = player.why_selected.saturating_sub(1);
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::Down, .. } => {
            if player.why_selected + 1 < ways_len {
                player.why_selected += 1;
            }
            player.why_detail_scroll = 0;
        }
        // Scroll the way document (render_why clamps to its length).
        KeyEvent { code: KeyCode::Char('j'), .. } => {
            player.why_detail_scroll = player.why_detail_scroll.saturating_add(1);
        }
        KeyEvent { code: KeyCode::Char('k'), .. } => {
            player.why_detail_scroll = player.why_detail_scroll.saturating_sub(1);
        }
        KeyEvent { code: KeyCode::PageDown, .. } => {
            player.why_detail_scroll = player.why_detail_scroll.saturating_add(page);
        }
        KeyEvent { code: KeyCode::PageUp, .. } => {
            player.why_detail_scroll = player.why_detail_scroll.saturating_sub(page);
        }
        KeyEvent { code: KeyCode::Char('g'), .. } | KeyEvent { code: KeyCode::Home, .. } => {
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::Char('G'), .. } | KeyEvent { code: KeyCode::End, .. } => {
            player.why_detail_scroll = usize::MAX; // render_why clamps to the bottom
        }
        // Frame navigation (time axis).
        KeyEvent { code: KeyCode::Right, .. } | KeyEvent { code: KeyCode::Char('l'), .. } => {
            player.following = false;
            jump_frame(player, player.current + 1);
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::Left, .. } | KeyEvent { code: KeyCode::Char('h'), .. } => {
            player.following = false;
            jump_frame(player, player.current.saturating_sub(1));
            player.why_detail_scroll = 0;
        }
        _ => {}
    }
}

// ── Why-fired drill-down (ADR-154 §1, §2) ─────────────────────

/// One channel's "why it fired" for a way, aggregated from the model across the
/// way's fires *on that channel*. Matched spans are collected distinctly.
#[cfg(feature = "tui")]
struct WhyEntry {
    way_path: Option<String>,
    trigger_channel: String,
    fire_score: Option<f64>,
    criteria: MatchCriteria,
    matched_spans: Vec<String>,
}

/// Index key: `(way_id, trigger_channel)`. A single way commonly fires on several
/// channels in one session (verified against the event log: e.g. `documentation`
/// fires bash + file + keyword + semantic). Each channel is its own coherent facet —
/// its own score and matched spans. Folding them into one `way_id` entry would let a
/// keyword span be shown under a semantic trigger, fabricating a semantic matched
/// term (the forbidden case, ADR-153). Keying by channel keeps each facet honest;
/// the drill-down looks up the channel the focused frame shows for that way. Still a
/// way_id-based join (no epoch alignment) per the ADR-154 §1 boundary — just at
/// channel granularity.
#[cfg(feature = "tui")]
type WhyKey = (String, String);

#[cfg(feature = "tui")]
type WhyIndex = HashMap<WhyKey, WhyEntry>;

/// Fold the model's per-turn fired-ways into a `(way_id, channel) → WhyEntry` index.
#[cfg(feature = "tui")]
fn build_why_index(model: &SessionIntrospection) -> WhyIndex {
    let mut idx: WhyIndex = HashMap::new();
    for turn in &model.turns {
        for fw in &turn.fired_ways {
            let key = (fw.way_id.clone(), fw.trigger_channel.clone());
            let e = idx.entry(key).or_insert_with(|| WhyEntry {
                way_path: fw.way_path.clone(),
                trigger_channel: fw.trigger_channel.clone(),
                fire_score: fw.fire_score,
                criteria: fw.criteria.clone(),
                matched_spans: Vec::new(),
            });
            if let Some(span) = fw.match_detail.as_ref().and_then(|m| m.matched_span.clone()) {
                if !e.matched_spans.contains(&span) {
                    e.matched_spans.push(span);
                }
            }
        }
    }
    idx
}

/// Read a way file's body: everything after a leading `---`/`---` frontmatter
/// block. Line-based so a body that legitimately opens with a markdown list (`- …`)
/// or a `---` rule is preserved. A file with no opening fence, or an unterminated
/// fence, is returned whole (nothing is silently dropped).
#[cfg(feature = "tui")]
fn read_way_body(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Some(content); // no opening fence — treat all as body
    }
    let mut in_frontmatter = true;
    let mut body = String::new();
    for line in lines {
        if in_frontmatter {
            if line == "---" {
                in_frontmatter = false; // consume the closing fence line
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    // Unterminated frontmatter (no closing fence) → don't drop the whole file.
    if in_frontmatter {
        return Some(content);
    }
    Some(body)
}

/// Render the detail panel for one way: its trigger, resolved `MatchCriteria`, the
/// matched spans (or an honest note when there's no recoverable term), and the way
/// body a human would read. `None` entry means the frame's way has no model record.
#[cfg(feature = "tui")]
fn render_why_detail(way_id: &str, entry: Option<&WhyEntry>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[1m{way_id}\x1b[0m");
    let Some(e) = entry else {
        let _ = writeln!(out, "\x1b[2mno fire record in the model for this way\x1b[0m");
        return out;
    };
    if let Some(p) = &e.way_path {
        let _ = writeln!(out, "\x1b[2m{p}\x1b[0m");
    }
    let _ = writeln!(out);

    let channel = render::format_trigger(&e.trigger_channel);
    match e.fire_score {
        Some(s) => {
            let _ = writeln!(out, "\x1b[1mTrigger\x1b[0m  {channel}  \x1b[2m(score {s:.2})\x1b[0m");
        }
        None => {
            let _ = writeln!(out, "\x1b[1mTrigger\x1b[0m  {channel}");
        }
    }

    let _ = writeln!(out, "\x1b[1mCriteria\x1b[0m");
    let c = &e.criteria;
    let mut wrote = false;
    for (label, val) in [
        ("pattern", &c.pattern),
        ("commands", &c.commands),
        ("files", &c.files),
        ("trigger", &c.trigger),
        ("vocabulary", &c.vocabulary),
        ("scope", &c.scope),
    ] {
        if let Some(v) = val {
            let _ = writeln!(out, "  \x1b[2m{label}:\x1b[0m {v}");
            wrote = true;
        }
    }
    if let Some(t) = c.embed_threshold {
        let _ = writeln!(out, "  \x1b[2membed_threshold:\x1b[0m {t}");
        wrote = true;
    }
    if !wrote {
        let _ = writeln!(out, "  \x1b[2m(none recorded)\x1b[0m");
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "\x1b[1mMatched\x1b[0m");
    if e.matched_spans.is_empty() {
        if e.trigger_channel.starts_with("semantic") {
            let _ = writeln!(out, "  \x1b[2msemantic fire — matched by embedding; no recoverable term\x1b[0m");
        } else {
            let _ = writeln!(out, "  \x1b[2mno span recorded (fired before matched-span enrichment)\x1b[0m");
        }
    } else {
        for span in &e.matched_spans {
            let _ = writeln!(out, "  \x1b[0;36m“{span}”\x1b[0m");
        }
    }

    if let Some(body) = e.way_path.as_deref().and_then(read_way_body) {
        let _ = writeln!(out);
        let _ = writeln!(out, "\x1b[2m── way ─────────────\x1b[0m");
        for line in body.lines() {
            let _ = writeln!(out, "{line}");
        }
    }
    out
}

/// Render the why-fired drill-down: the current frame's fired ways on the left,
/// the selected way's model-derived detail on the right, composed with the
/// micro-compositor (ADR-154 §2). The model index is built lazily on first entry.
#[cfg(feature = "tui")]
fn render_why(player: &mut Player) -> String {
    if player.why_index.is_none() {
        let model = SessionIntrospection::from_session(
            &player.session_id,
            &player.project_name,
            player.context_window_k,
        );
        player.why_index = Some(build_why_index(&model));
    }

    let ways_len = player.frames[player.current].ways.len();
    let sel = player.why_selected.min(ways_len.saturating_sub(1));
    player.why_selected = sel;

    let total_w = player.term_width as usize;
    // Shared chrome is header (4) + footer (2) = 6 rows; the body fills the rest of
    // the drawable height (`term_height − 1`, since fit_to_terminal drops the last
    // cell to avoid a scroll), so 4 + body_h + 2 == term_height − 1.
    let body_h = (player.term_height as usize).saturating_sub(7).max(3);
    let gap = 2;
    let left_w = (total_w / 3).clamp(16, 40).min(total_w.saturating_sub(gap + 12).max(16));
    let right_w = total_w.saturating_sub(left_w + gap).max(12);

    // Build both panels while borrowing the index + frame, then drop those borrows
    // before the scroll write-backs below.
    let (left, right) = {
        let idx = player.why_index.as_ref().unwrap();
        let frame = &player.frames[player.current];

        // Look up the model facet for the channel this frame shows the way on, so a
        // multi-channel way's detail is coherent (a keyword span never surfaces under
        // a semantic trigger).
        let facet = |w: &ActiveWay| idx.get(&(w.id.clone(), w.trigger.clone()));

        // Prefix each row with the epoch the way fired in, so a row cross-references
        // straight back to the Timeline's Epoch column. Right-align to the widest
        // epoch in the frame so the way ids stay aligned.
        let epoch_w = frame
            .ways
            .iter()
            .map(|w| w.epoch_fired)
            .max()
            .unwrap_or(0)
            .to_string()
            .len();

        // A 2-space left margin aligns this list with the Timeline's Way column (and
        // `ways list`), so the lists don't shift horizontally when toggling views.
        let mut left_lines: Vec<String> = Vec::with_capacity(frame.ways.len());
        for (i, w) in frame.ways.iter().enumerate() {
            // A filled bullet marks a way the model has a fire record for on this channel.
            let bullet = if facet(w).is_some() { "•" } else { "·" };
            if i == sel {
                // Margin stays plain; only the content is reversed (as write_way_row does),
                // so the highlight bar starts at the Way column, not in the margin.
                let raw = format!("{bullet} e{:>ew$} {}", w.epoch_fired, w.id, ew = epoch_w);
                left_lines.push(format!(
                    "  \x1b[7m{}\x1b[0m",
                    compositor::fit_visible(&raw, left_w.saturating_sub(2))
                ));
            } else {
                // Dim the epoch tag so the way id stays prominent.
                left_lines.push(format!(
                    "  {bullet} \x1b[2me{:>ew$}\x1b[0m {}",
                    w.epoch_fired,
                    w.id,
                    ew = epoch_w
                ));
            }
        }
        if left_lines.is_empty() {
            left_lines.push("  \x1b[2m(no ways in this frame)\x1b[0m".to_string());
        }

        let detail = frame
            .ways
            .get(sel)
            .map(|w| render_why_detail(&w.id, facet(w)))
            .unwrap_or_else(|| "\x1b[2mno ways fired in this frame\x1b[0m".to_string());

        (
            Panel::from_lines(left_lines).fixed_width(left_w),
            Panel::from_text(&detail).fixed_width(right_w),
        )
    };

    let detail_scroll = player.why_detail_scroll.min(right.max_scroll(body_h));
    player.why_detail_scroll = detail_scroll;
    let left_scroll = sel
        .saturating_sub(body_h.saturating_sub(1))
        .min(left.max_scroll(body_h));

    let composited = compositor::hjoin2(
        &left.viewport(left_scroll, body_h),
        &right.viewport(detail_scroll, body_h),
        gap,
    );

    // Shared footer (same 2 rows as the Timeline).
    let mut nav_buf = String::new();
    render_status_bar(&mut nav_buf, player);
    let nav: Vec<String> = nav_buf.lines().map(str::to_string).collect();

    // Shared header, with the drill-down's own column labels + rule so the two
    // panels are named the way the Timeline's columns are.
    let labels = format!(
        "\x1b[1m{}\x1b[0m{}\x1b[1mWhy it fired\x1b[0m",
        compositor::pad_visible("  Way · epoch", left_w),
        " ".repeat(gap),
    );
    let rule = format!(
        "\x1b[2m{}\x1b[0m",
        "─".repeat((left_w + gap + right_w).min(total_w))
    );
    let header = header_lines(player, vec![labels, rule]);

    // header (4) + body (body_h) + footer (2) = exactly the drawable height, so the
    // chrome lines up row-for-row with the Timeline view when toggling. On a very
    // short terminal (where body_h hits its floor) drop from the top so the footer
    // stays visible, mirroring compose_screen.
    let drawable = (player.term_height as usize).saturating_sub(1).max(4);
    let mut lines = header;
    lines.extend(composited);
    lines.extend(nav);
    if lines.len() > drawable {
        lines.drain(0..lines.len() - drawable);
    }
    lines.join("\n")
}

// ── Frame renderer ────────────────────────────────────────────

/// Assemble a **fixed-height** screen so the nav legend is always pinned to the
/// bottom rows, whatever the content length or terminal size:
///
/// - `top` (header) is pinned at the top;
/// - `rows` is the scrollable region, viewported to keep `sel_line` visible and
///   blank-padded so it always fills its share of the height;
/// - `extras` (e.g. the token timeline) are pinned just above the nav, but only
///   when the terminal is tall enough to also show a few rows of the list;
/// - `nav` is pinned as the final lines.
///
/// The result is exactly `drawable` lines — the height `fit_to_terminal` will keep
/// (`term_height − 1`) — so nothing gets pushed off the bottom.
#[cfg(feature = "tui")]
fn compose_screen(
    top: Vec<String>,
    rows: Vec<String>,
    sel_line: usize,
    extras: Vec<String>,
    nav: Vec<String>,
    drawable: usize,
) -> String {
    let mut rows_area = drawable.saturating_sub(top.len() + nav.len()).max(1);

    // Show the extras only if they still leave a few rows of the list visible.
    let show_extras = !extras.is_empty() && rows_area > extras.len() + 3;
    if show_extras {
        rows_area = rows_area.saturating_sub(extras.len());
    }

    // Reserve one row of the list area for the scroll hint when the list overflows.
    let overflow = rows.len() > rows_area;
    let view_h = if overflow { rows_area.saturating_sub(1).max(1) } else { rows_area };
    let scroll = if overflow {
        let max_scroll = rows.len().saturating_sub(view_h);
        // Bottom-anchor the selected line (same behaviour as the drill-down list).
        sel_line.saturating_sub(view_h.saturating_sub(1)).min(max_scroll)
    } else {
        0
    };

    let mut lines: Vec<String> = Vec::with_capacity(drawable);
    lines.extend(top);

    let mut filled = 0;
    for r in rows.iter().skip(scroll).take(view_h) {
        lines.push(r.clone());
        filled += 1;
    }
    if overflow {
        let below = rows.len().saturating_sub(view_h + scroll);
        lines.push(format!("  \x1b[2m⋮ {scroll} above · {below} below · ↑↓\x1b[0m"));
        filled += 1;
    }
    // Pad the list area so the extras/nav stay anchored at the bottom.
    while filled < rows_area {
        lines.push(String::new());
        filled += 1;
    }

    if show_extras {
        lines.extend(extras);
    }
    lines.extend(nav);
    // On a very short terminal the pinned header + nav alone can exceed the height;
    // drop from the TOP so the nav legend is never the thing that gets clipped.
    if lines.len() > drawable {
        lines.drain(0..lines.len() - drawable);
    }
    lines.join("\n")
}

#[cfg(feature = "tui")]
fn render_frame(player: &mut Player) -> String {
    // Clamp the shared selection to this frame before borrowing it.
    let ways_len = player.frames[player.current].ways.len();
    let selected = player.why_selected.min(ways_len.saturating_sub(1));
    player.why_selected = selected;

    // `fit_to_terminal` keeps `term_height − 1` rows (it leaves the last cell empty
    // to avoid a scroll), so that's the height we compose to.
    let drawable = (player.term_height as usize).saturating_sub(1).max(6);
    let context_window_k = player.context_window_k;

    // Footer + header are the shared chrome (built before borrowing the frame).
    let mut nav_buf = String::new();
    render_status_bar(&mut nav_buf, player);
    let nav: Vec<String> = nav_buf.lines().map(str::to_string).collect();

    // The Timeline's column header is the shared ways-table header (labels + rule).
    let mut th = String::new();
    render::write_table_header(&mut th);
    let top = header_lines(player, th.lines().map(str::to_string).collect());

    let frame = &player.frames[player.current];
    if frame.ways.is_empty() {
        let rows = vec!["  \x1b[2mNo ways triggered yet.\x1b[0m".to_string()];
        return compose_screen(top, rows, 0, Vec::new(), nav, drawable);
    }

    let current_epoch = frame.epoch;
    let current_tokens_k = frame.token_position_k;
    let bar_positions = render::compute_bar_positions(&frame.ways, context_window_k);
    let unique_pos = render::unique_positions(&bar_positions);

    // ── Scrollable, selectable rows ──
    // One selectable unit per way; its rendered block (1 line, or 2 with a
    // check-fires line) is tracked so the viewport can keep the selection visible.
    let mut rows: Vec<String> = Vec::new();
    let mut way_start_line: Vec<usize> = Vec::with_capacity(frame.ways.len());
    for (i, w) in frame.ways.iter().enumerate() {
        way_start_line.push(rows.len());
        // The selection highlight takes visual precedence over new/redisclosed color.
        let (prefix, suffix) = if i == selected {
            ("\x1b[7m", "\x1b[0m")
        } else if w.is_new {
            ("\x1b[1;32m", "\x1b[0m")
        } else if w.is_redisclosed {
            ("\x1b[1;36m", "\x1b[0m")
        } else {
            ("", "")
        };
        let mut block = String::new();
        render::write_way_row(
            &mut block, w, current_epoch, current_tokens_k,
            &bar_positions, &unique_pos, i, prefix, suffix,
        );
        rows.extend(block.lines().map(str::to_string));
    }
    let sel_line = way_start_line.get(selected).copied().unwrap_or(0);

    // ── Extras pinned above the nav (token timeline + new events) ──
    let mut extras: Vec<String> = Vec::new();
    if current_tokens_k > 0 {
        let mut tl = String::new();
        let _ = writeln!(tl);
        render::write_token_timeline(
            &mut tl, &frame.ways, &unique_pos,
            current_tokens_k, context_window_k,
        );
        extras.extend(tl.lines().map(str::to_string));
    }
    if !frame.new_events.is_empty() {
        extras.push(String::new());
        extras.push(format!("  \x1b[1;32m+ {}\x1b[0m", frame.new_events.join(", ")));
    }

    compose_screen(top, rows, sel_line, extras, nav, drawable)
}

/// The unified 2-row footer shared by both views: a full-width rule, then the
/// tab-bar + view-appropriate key hints, distributed evenly across the terminal
/// width (space-between). The `tab`/`esc`/position tail is identical across views so
/// the legend never jumps when toggling.
fn render_status_bar(out: &mut String, player: &Player) {
    let width = (player.term_width as usize).max(1);
    let _ = writeln!(out, "\x1b[2m{}\x1b[0m", "─".repeat(width));

    let active = if player.view == View::WhyFired { 1 } else { 0 };

    // Each element is one `[key] label` unit; `justify` spreads them across `width`.
    let mut segs: Vec<String> = vec![compositor::tab_bar(&["Timeline", "Why fired"], active)];
    match player.view {
        View::Timeline => {
            segs.push("\x1b[7m ▲▼ \x1b[0m select".into());
            segs.push("\x1b[7m ⏎ \x1b[0m why".into());
            segs.push("\x1b[7m ◀▶ \x1b[0m frame".into());
            if player.live {
                segs.push(if player.following {
                    "\x1b[7m space \x1b[0m \x1b[1;32m● following\x1b[0m".into()
                } else {
                    "\x1b[7m space \x1b[0m \x1b[1;33m● paused\x1b[0m".into()
                });
            } else {
                segs.push("\x1b[7m space \x1b[0m play".into());
                segs.push(format!("\x1b[7m +- \x1b[0m \x1b[2m{}\x1b[0m", SPEEDS[player.speed_idx].1));
            }
        }
        View::WhyFired => {
            segs.push("\x1b[7m ▲▼ \x1b[0m way".into());
            segs.push("\x1b[7m j/k \x1b[0m read".into());
            segs.push("\x1b[7m ◀▶ \x1b[0m frame".into());
        }
    }
    segs.push("\x1b[7m tab \x1b[0m view".into());
    segs.push("\x1b[7m esc \x1b[0m quit".into());
    segs.push(format!("\x1b[1m{}/{}\x1b[0m", player.current + 1, player.frames.len()));

    out.push_str(&justify(&segs, width));
}

/// Lay `segments` out across `width` with even gaps between them (space-between):
/// the first hugs the left edge, the last the right, the rest spread evenly. Falls
/// back to a single-space join when the content is already wider than `width`.
#[cfg(feature = "tui")]
fn justify(segments: &[String], width: usize) -> String {
    let content: usize = segments.iter().map(|s| compositor::visible_len(s)).sum();
    let gaps = segments.len().saturating_sub(1);
    if gaps == 0 || content + gaps >= width {
        return segments.join(" ");
    }
    let total_space = width - content;
    let base = total_space / gaps;
    let extra = total_space % gaps;
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        out.push_str(seg);
        if i < gaps {
            out.push_str(&" ".repeat(base + usize::from(i < extra)));
        }
    }
    out
}

/// The unified 4-row header both views share, so toggling never makes the header
/// jump: `Session <id>  <path>`, a metrics line, then the view's own 2-row column
/// header (labels + rule). `col_header` must be exactly 2 lines.
#[cfg(feature = "tui")]
fn header_lines(player: &Player, col_header: Vec<String>) -> Vec<String> {
    let frame = &player.frames[player.current];
    let live = if player.live {
        if player.following {
            "  \x1b[1;32m● LIVE\x1b[0m"
        } else {
            "  \x1b[1;33m● LIVE paused\x1b[0m"
        }
    } else {
        ""
    };
    let mut h = vec![
        // Full session id (there's ample room), with the session's project path.
        format!(
            "\x1b[1mSession\x1b[0m {}  \x1b[2m{}\x1b[0m",
            player.session_id, player.project_name
        ),
        // The current frame's wall-clock time anchors "when" you are as you scrub
        // windows/frames — otherwise every window looks alike.
        format!(
            "  \x1b[2mepoch {} · {}K ctx · {} ways · window {}/{} · {}\x1b[0m{live}",
            frame.epoch,
            player.context_window_k,
            frame.ways.len(),
            frame.window,
            player.windows,
            friendly_ts(&frame.timestamp),
        ),
    ];
    h.extend(col_header);
    h
}

/// `2026-07-03T16:52:00Z` → `2026-07-03 16:52` (minute precision, no `T`/`Z`).
#[cfg(feature = "tui")]
fn friendly_ts(ts: &str) -> String {
    let spaced = ts.replace('T', " ");
    spaced.get(..16).unwrap_or(&spaced).to_string()
}

// ── Frame construction ────────────────────────────────────────

/// Reconstruct the full replay frame timeline for a session. Loads the token
/// timeline, pre-resolves per-way refire thresholds, and clusters events into
/// epoch frames. Shared by the interactive replay (`run`) and the JSON dump
/// (`rethink_dump::run_json`).
///
/// Refire thresholds reflect each way's *current* curve — rethink is a replay,
/// so a curve edited since the recorded session shows today's value. That's the
/// best we can do without snapshotting frontmatter into events.jsonl.
pub(crate) fn reconstruct_frames(
    events: &[WayEvent],
    project_name: &str,
    session_id: &str,
    context_window: u64,
) -> Vec<Frame> {
    let context_window_k = context_window / 1000;
    let token_timeline = build_token_timeline(project_name, session_id);
    let fallback_refire_k = context_window_k * 25 / 100;
    let mut refire_cache: HashMap<String, u64> = HashMap::new();
    for ev in events {
        if ev.way.is_empty() || refire_cache.contains_key(&ev.way) {
            continue;
        }
        let threshold_k = session::way_refire_threshold_k(&ev.way, project_name, context_window)
            .unwrap_or(fallback_refire_k);
        refire_cache.insert(ev.way.clone(), threshold_k);
    }
    build_frames(events, &token_timeline, &refire_cache, fallback_refire_k)
}

fn build_frames(
    events: &[WayEvent],
    token_timeline: &[(String, u64)],
    refire_cache: &HashMap<String, u64>,
    fallback_refire_k: u64,
) -> Vec<Frame> {
    let refire_for = |way_id: &str| -> u64 {
        refire_cache.get(way_id).copied().unwrap_or(fallback_refire_k)
    };
    let mut frames: Vec<Frame> = Vec::new();
    let mut active_ways: HashMap<String, ActiveWay> = HashMap::new();
    let mut check_fires: HashMap<String, u64> = HashMap::new();
    let mut epoch: u64 = 0;
    let mut window: u64 = 1;

    let start_ts = events.first().map(|e| &e.ts).cloned().unwrap_or_default();
    let start_secs = parse_ts_secs(&start_ts);

    // Cluster events by timestamp proximity (≤3s gap = same epoch)
    let mut clusters: Vec<Vec<&WayEvent>> = Vec::new();
    let mut current_cluster: Vec<&WayEvent> = Vec::new();
    let mut last_ts_secs: u64 = 0;

    for ev in events {
        let ts_secs = parse_ts_secs(&ev.ts);
        if !current_cluster.is_empty() && ts_secs > last_ts_secs + 3 {
            clusters.push(std::mem::take(&mut current_cluster));
        }
        current_cluster.push(ev);
        last_ts_secs = ts_secs;
    }
    if !current_cluster.is_empty() {
        clusters.push(current_cluster);
    }

    for cluster in &clusters {
        // A `session_start` after the session's origin is a compaction boundary: the
        // real markers were cleared there, so reset the accumulated window state and
        // restart epoch numbering. The latest window then reflects only what fired
        // since the last compaction — the same grain `ways list` shows.
        let boundary = !frames.is_empty()
            && cluster.iter().any(|ev| ev.event == "session_start");
        if boundary {
            active_ways.clear();
            check_fires.clear();
            window += 1;
            epoch = 0;
        }

        epoch += 1;
        let cluster_ts = cluster[0].ts.clone();
        let cluster_secs = parse_ts_secs(&cluster_ts);
        let elapsed = cluster_secs.saturating_sub(start_secs);

        let token_k = find_token_position(token_timeline, &cluster_ts);

        let mut new_events: Vec<String> = Vec::new();
        if boundary {
            new_events.push(format!("⎯ compaction · window {window} ⎯"));
        }

        // Mark all existing ways as not-new
        for w in active_ways.values_mut() {
            w.is_new = false;
            w.is_redisclosed = false;
        }

        for ev in cluster {
            match ev.event.as_str() {
                "way_fired" => {
                    if !ev.way.is_empty() {
                        let existing = active_ways.get(&ev.way);
                        if existing.is_none() {
                            new_events.push(format!(
                                "{} ({})",
                                ev.way,
                                render::format_trigger(&ev.trigger)
                            ));
                        }
                        active_ways.insert(ev.way.clone(), ActiveWay {
                            id: ev.way.clone(),
                            trigger: ev.trigger.clone(),
                            epoch_fired: epoch,
                            token_pos: token_k * 1000,
                            check_fires: check_fires.get(&ev.way).copied().unwrap_or(0),
                            is_new: existing.is_none(),
                            is_redisclosed: false,
                            refire_threshold_k: refire_for(&ev.way),
                        });
                    }
                }
                "check_fired" => {
                    if !ev.check.is_empty() {
                        let count = check_fires.entry(ev.check.clone()).or_insert(0);
                        *count += 1;
                        if let Some(w) = active_ways.get_mut(&ev.check) {
                            w.check_fires = *count;
                        }
                        new_events.push(format!("✓ check {}", ev.check));
                    }
                }
                "way_redisclosed" => {
                    if !ev.way.is_empty() {
                        new_events.push(format!("↻ {}", ev.way));
                        // A redisclosure means the way is active (re-injected). Update it
                        // if present; otherwise ADD it — after a compaction-window reset a
                        // still-active way first reappears via redisclosure, not a fresh
                        // fire, and must repopulate the window or it looks empty.
                        active_ways
                            .entry(ev.way.clone())
                            .and_modify(|w| {
                                w.epoch_fired = epoch;
                                w.token_pos = token_k * 1000;
                                w.is_redisclosed = true;
                                w.is_new = false;
                            })
                            .or_insert_with(|| ActiveWay {
                                id: ev.way.clone(),
                                trigger: ev.trigger.clone(),
                                epoch_fired: epoch,
                                token_pos: token_k * 1000,
                                check_fires: check_fires.get(&ev.way).copied().unwrap_or(0),
                                is_new: false,
                                is_redisclosed: true,
                                refire_threshold_k: refire_for(&ev.way),
                            });
                    }
                }
                _ => {}
            }
        }

        let mut ways: Vec<ActiveWay> = active_ways.values().cloned().collect();
        ways.sort_by_key(|w| w.epoch_fired);

        frames.push(Frame {
            epoch,
            timestamp: cluster_ts,
            elapsed_secs: elapsed,
            token_position_k: token_k,
            ways,
            new_events,
            window,
        });
    }

    frames
}

fn build_token_timeline(project: &str, session_id: &str) -> Vec<(String, u64)> {
    let project_slug = project.replace(['/', '.'], "-");
    let transcript_path = home_dir()
        .join(format!(".claude/projects/{project_slug}/{session_id}.jsonl"));

    let content = match std::fs::read_to_string(&transcript_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut timeline: Vec<(String, u64)> = Vec::new();

    for line in content.lines() {
        if !line.contains("cache_read_input_tokens") {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("type").and_then(|t| t.as_str()) != Some("assistant") {
                continue;
            }
            let ts = val.get("timestamp")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(usage) = val.get("message").and_then(|m| m.get("usage")) {
                let cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                let cache_create = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                let input = usage["input_tokens"].as_u64().unwrap_or(0);
                let total_k = (cache_read + cache_create + input) / 1000;
                if !ts.is_empty() {
                    timeline.push((ts, total_k));
                }
            }
        }
    }

    timeline
}

fn find_token_position(timeline: &[(String, u64)], ts: &str) -> u64 {
    if timeline.is_empty() {
        return 0;
    }
    let mut best = 0u64;
    for (entry_ts, tokens_k) in timeline {
        if entry_ts.as_str() <= ts {
            best = *tokens_k;
        } else {
            break;
        }
    }
    best
}

// ── Event loading ─────────────────────────────────────────────

pub(crate) fn load_session_events(content: &str, session_id: &str) -> Vec<WayEvent> {
    let mut events: Vec<WayEvent> = content
        .lines()
        .filter(|l| l.contains(session_id))
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            if v["session"].as_str()? != session_id {
                return None;
            }
            Some(WayEvent {
                ts: v["ts"].as_str().unwrap_or("").to_string(),
                event: v["event"].as_str().unwrap_or("").to_string(),
                way: v["way"].as_str().unwrap_or("").to_string(),
                trigger: v["trigger"].as_str().unwrap_or("").to_string(),
                check: v["check"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();
    // The event log is a UNION of sources (state + legacy projection) concatenated
    // without sorting, so a session's events can arrive out of order — which would
    // scramble build_frames' ≤3s clustering and its compaction-window boundaries
    // (the symptom: the "newest" frame stuck at an old legacy tail). Sort by
    // timestamp so the stream is chronological. RFC-3339 UTC strings sort lexically.
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    events
}

pub(crate) fn find_session_project(content: &str, session_id: &str) -> Option<String> {
    for line in content.lines() {
        if !line.contains(session_id) || !line.contains("session_start") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v["session"].as_str() == Some(session_id) {
                return v["project"].as_str().map(|s| s.to_string());
            }
        }
    }
    None
}

// ── Session listing and picker ────────────────────────────────

#[derive(serde::Serialize)]
pub(crate) struct SessionInfo {
    pub(crate) id: String,
    pub(crate) ts: String,
    pub(crate) project: String,
    pub(crate) event_count: u32,
    pub(crate) way_fires: u32,
    pub(crate) duration_secs: u64,
}

pub(crate) fn gather_sessions(content: &str, project_filter: Option<&str>) -> Vec<SessionInfo> {
    let mut sessions: Vec<SessionInfo> = Vec::new();
    let mut event_counts: HashMap<String, (u32, u32)> = HashMap::new();
    let mut last_ts: HashMap<String, String> = HashMap::new();
    // One entry per session id. `clear-markers.sh` writes `session_start` on both
    // SessionStart and post-compaction, so a compacted session has several such
    // lines; keep the first (its origin) and let the post-loop pass fill the
    // aggregated counts, so the list (and the `--list --json` `count`) don't dupe.
    let mut seen: HashSet<String> = HashSet::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let sid = match v["session"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };

        let event = v["event"].as_str().unwrap_or("");
        let ts = v["ts"].as_str().unwrap_or("").to_string();

        if event == "session_start" {
            let project = v["project"].as_str().unwrap_or("").to_string();
            if let Some(pf) = project_filter {
                if !project_matches(&project, pf) {
                    continue;
                }
            }
            if seen.insert(sid.clone()) {
                sessions.push(SessionInfo {
                    id: sid.clone(),
                    ts: ts.clone(),
                    project,
                    event_count: 0,
                    way_fires: 0,
                    duration_secs: 0,
                });
            }
        }

        let counts = event_counts.entry(sid.clone()).or_insert((0, 0));
        counts.0 += 1;
        if event == "way_fired" {
            counts.1 += 1;
        }
        last_ts.insert(sid, ts);
    }

    for s in &mut sessions {
        if let Some((total, fires)) = event_counts.get(&s.id) {
            s.event_count = *total;
            s.way_fires = *fires;
        }
        if let Some(last) = last_ts.get(&s.id) {
            let start = parse_ts_secs(&s.ts);
            let end = parse_ts_secs(last);
            s.duration_secs = end.saturating_sub(start);
        }
    }

    sessions
}

fn list_sessions(content: &str, project_filter: Option<&str>) -> Result<()> {
    let sessions = gather_sessions(content, project_filter);
    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!();
    println!(
        "\x1b[1m{:<14} {:<20} {:<30} {:>6} {:>6} {:>8}\x1b[0m",
        "Session", "Date", "Project", "Events", "Ways", "Duration"
    );
    println!("\x1b[2m{}\x1b[0m", "─".repeat(90));

    for s in sessions.iter().rev().take(50) {
        let short_id = &s.id[..s.id.len().min(12)];
        let date = &s.ts[..s.ts.len().min(16)];
        let project_short = s.project.split('/').next_back().unwrap_or(&s.project);
        let duration = format_duration(s.duration_secs);
        println!(
            "  {:<12} {:<20} {:<30} {:>6} {:>6} {:>8}",
            short_id, date, project_short, s.event_count, s.way_fires, duration
        );
    }
    println!();
    println!(
        "\x1b[2m  {} sessions total. Use --session <id> or run without args for interactive picker.\x1b[0m",
        sessions.len()
    );
    println!();
    Ok(())
}

#[cfg(feature = "tui")]
fn pick_session(content: &str, project_filter: Option<&str>) -> Option<String> {
    let sessions = gather_sessions(content, project_filter);
    if sessions.is_empty() {
        println!("No sessions found.");
        return None;
    }

    let sessions: Vec<&SessionInfo> = sessions.iter().rev().collect();
    let mut selected: usize = 0;
    let page_size = 20usize;

    let _guard = TermGuard::enter().ok()?;
    let mut stdout = std::io::stdout();

    loop {
        let (tw, th) = terminal::size().unwrap_or((120, 40));
        let mut out = String::new();
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "\x1b[1m  Select a session to replay\x1b[0m  \x1b[2m({} sessions)\x1b[0m",
            sessions.len()
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  \x1b[1m{:<14} {:<18} {:<28} {:>5} {:>5} {:>8}\x1b[0m",
            "Session", "Date", "Project", "Evts", "Ways", "Duration"
        );
        let _ = writeln!(out, "  \x1b[2m{}\x1b[0m", "─".repeat(82));

        let page_start = (selected / page_size) * page_size;
        let page_end = (page_start + page_size).min(sessions.len());

        for (i, s) in sessions.iter().enumerate().skip(page_start).take(page_end - page_start) {
            let short_id = &s.id[..s.id.len().min(12)];
            let date = &s.ts[..s.ts.len().min(16)];
            let project_short = s.project.split('/').next_back().unwrap_or(&s.project);
            let duration = format_duration(s.duration_secs);

            let (prefix, suffix) = if i == selected {
                ("\x1b[7m", "\x1b[0m")
            } else {
                ("", "")
            };

            let _ = writeln!(
                out,
                "  {prefix}{:<12}  {:<18} {:<28} {:>5} {:>5} {:>8}{suffix}",
                short_id, date, project_short, s.event_count, s.way_fires, duration
            );
        }

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  \x1b[2mPage {}/{}\x1b[0m",
            selected / page_size + 1,
            sessions.len().div_ceil(page_size)
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "\x1b[2m{}\x1b[0m", "─".repeat(85));
        let _ = write!(
            out,
            " \x1b[7m ▲▼ \x1b[0m select  \x1b[7m ⏎ \x1b[0m replay  \x1b[7m esc \x1b[0m quit"
        );

        let fitted = fit_to_terminal(&out, tw as usize, th as usize);
        execute!(stdout, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All)).ok();
        write!(stdout, "{fitted}").ok();
        stdout.flush().ok();

        if let Ok(Event::Key(key)) = event::read() {
            match key {
                KeyEvent { code: KeyCode::Esc, .. }
                | KeyEvent { code: KeyCode::Char('q'), .. } => return None,

                KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } => return None,

                KeyEvent { code: KeyCode::Enter, .. } => {
                    return Some(sessions[selected].id.clone());
                }

                KeyEvent { code: KeyCode::Up, .. }
                | KeyEvent { code: KeyCode::Char('k'), .. } => {
                    selected = selected.saturating_sub(1);
                }

                KeyEvent { code: KeyCode::Down, .. }
                | KeyEvent { code: KeyCode::Char('j'), .. } => {
                    if selected < sessions.len() - 1 {
                        selected += 1;
                    }
                }

                KeyEvent { code: KeyCode::PageUp, .. } => {
                    selected = selected.saturating_sub(page_size);
                }

                KeyEvent { code: KeyCode::PageDown, .. } => {
                    selected = (selected + page_size).min(sessions.len() - 1);
                }

                KeyEvent { code: KeyCode::Home, .. }
                | KeyEvent { code: KeyCode::Char('g'), .. } => {
                    selected = 0;
                }

                KeyEvent { code: KeyCode::End, .. }
                | KeyEvent { code: KeyCode::Char('G'), .. } => {
                    selected = sessions.len() - 1;
                }

                _ => {}
            }
        }
    }
}

#[cfg(not(feature = "tui"))]
fn pick_session(_content: &str, _project_filter: Option<&str>) -> Option<String> {
    eprintln!("Interactive picker requires the 'tui' feature.");
    None
}

// ── Helpers ───────────────────────────────────────────────────

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{h}h {m}m")
    }
}


/// Fit rendered output to terminal dimensions.
/// Uses \r\n because raw mode requires explicit carriage return.
fn fit_to_terminal(output: &str, width: usize, height: usize) -> String {
    let mut result = String::new();
    let max_lines = height.saturating_sub(1);

    for (line_count, line) in output.lines().enumerate() {
        if line_count >= max_lines {
            break;
        }
        result.push_str(&crate::cmd::compositor::truncate_visible(line, width));
        result.push_str("\r\n");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_matches_is_exact_not_substring() {
        // Exact path (and trailing-slash normalization) matches.
        assert!(project_matches("/home/a/proj", "/home/a/proj"));
        assert!(project_matches("/home/a/proj/", "/home/a/proj"));
        assert!(project_matches("/home/a/proj", "/home/a/proj/"));
        // The bug the fix closes: sibling / prefixed projects must NOT match,
        // which the old `contains` substring test wrongly conflated.
        assert!(!project_matches("/home/a/proj-2", "/home/a/proj"));
        assert!(!project_matches("/home/a/proj", "proj"));
        assert!(!project_matches("/home/a/other", "/home/a/proj"));
    }

    #[test]
    fn gather_sessions_dedups_compaction_restarts() {
        // Same session id with two `session_start` lines (initial + post-compaction).
        let content = concat!(
            r#"{"event":"session_start","session":"s1","ts":"2026-01-01T00:00:00Z","project":"/p"}"#, "\n",
            r#"{"event":"way_fired","session":"s1","ts":"2026-01-01T00:01:00Z","way":"a/b"}"#, "\n",
            r#"{"event":"session_start","session":"s1","ts":"2026-01-02T00:00:00Z","project":"/p"}"#, "\n",
            r#"{"event":"way_fired","session":"s1","ts":"2026-01-02T00:01:00Z","way":"a/c"}"#, "\n",
        );
        let sessions = gather_sessions(content, Some("/p"));
        assert_eq!(sessions.len(), 1, "one entry per session id, not per session_start");
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[0].ts, "2026-01-01T00:00:00Z", "keeps the origin start");
        assert_eq!(sessions[0].event_count, 4, "counts aggregate across the whole session");
        assert_eq!(sessions[0].way_fires, 2);
    }

    #[test]
    fn scope_all_is_none_and_explicit_wins() {
        // `--all` → every project, regardless of env.
        assert_eq!(resolve_project_scope(None, true).unwrap(), None);
        assert_eq!(resolve_project_scope(Some("/x"), true).unwrap(), None);
        // Explicit `--project` is honored without touching detection.
        assert_eq!(
            resolve_project_scope(Some("/home/a/proj"), false).unwrap(),
            Some("/home/a/proj".to_string())
        );
    }
}

// Drill-down "why" folding + rendering (the join-honesty-critical part).
#[cfg(all(test, feature = "tui"))]
mod why_tests {
    use super::*;
    use ways_core::introspection::{
        FiredWay, IntrospectionSummary, JoinConfidence, MatchCriteria, MatchDetail,
        SessionIntrospection, Turn,
    };

    fn fired(way: &str, channel: &str, span: Option<&str>, score: Option<f64>) -> FiredWay {
        FiredWay {
            way_id: way.into(),
            trigger_channel: channel.into(),
            fire_score: score,
            way_path: None,
            criteria: MatchCriteria { pattern: Some("p".into()), ..Default::default() },
            match_detail: span.map(|s| MatchDetail {
                matched_span: Some(s.into()),
                confidence: JoinConfidence::Keyed,
            }),
        }
    }

    fn model(turns_ways: Vec<Vec<FiredWay>>) -> SessionIntrospection {
        let turns = turns_ways
            .into_iter()
            .map(|fired_ways| Turn {
                epoch: 1,
                token_position: 0,
                ts: "2026-01-01T00:00:00Z".into(),
                transcript_uuid: None,
                join_confidence: JoinConfidence::Heuristic,
                fired_ways,
            })
            .collect();
        SessionIntrospection {
            id: "s".into(),
            project: "/p".into(),
            window_k: 200,
            summary: IntrospectionSummary::default(),
            turns,
        }
    }

    fn key(way: &str, channel: &str) -> WhyKey {
        (way.to_string(), channel.to_string())
    }

    #[test]
    fn why_index_keys_by_channel_and_dedups_spans() {
        let m = model(vec![
            vec![fired("d/a", "keyword", Some("commit"), None)],
            vec![
                fired("d/a", "keyword", Some("commit"), None), // dup span, same channel
                fired("d/a", "keyword", Some("stage"), None),  // new span
            ],
        ]);
        let idx = build_why_index(&m);
        let e = idx.get(&key("d/a", "keyword")).expect("keyed by (way, channel)");
        assert_eq!(e.matched_spans, vec!["commit", "stage"], "deduped, first-seen order");
    }

    #[test]
    fn multichannel_way_keeps_each_facet_honest() {
        // The real hole (verified common in the event log): a way fires BOTH
        // semantically and by keyword in one session. The semantic facet must never
        // borrow the keyword fire's span (that would fabricate a semantic term).
        let idx = build_why_index(&model(vec![
            vec![fired("d/doc", "semantic:embedding:en", None, Some(0.71))], // semantic first
            vec![fired("d/doc", "keyword", Some("diataxis"), None)],         // keyword later
        ]));

        let sem = render_why_detail("d/doc", idx.get(&key("d/doc", "semantic:embedding:en")));
        assert!(sem.contains("no recoverable term"), "semantic facet stays term-free: {sem}");
        assert!(!sem.contains("diataxis"), "keyword span must NOT appear under semantic");
        assert!(sem.contains("score 0.71"));

        let kw = render_why_detail("d/doc", idx.get(&key("d/doc", "keyword")));
        assert!(kw.contains("diataxis"), "keyword facet shows its own real span");
    }

    #[test]
    fn detail_labels_semantic_and_missing_spans_honestly() {
        // Semantic fire → names the embedding + score, never a fabricated term.
        let sem = build_why_index(&model(vec![vec![fired(
            "d/s", "semantic:embedding:en", None, Some(0.73),
        )]]));
        let out = render_why_detail("d/s", sem.get(&key("d/s", "semantic:embedding:en")));
        assert!(out.contains("no recoverable term"), "semantic honesty: {out}");
        assert!(out.contains("score 0.73"));

        // Keyword fire with a span → shows the quoted span.
        let kw = build_why_index(&model(vec![vec![fired(
            "d/k", "keyword", Some("threat model"), None,
        )]]));
        assert!(render_why_detail("d/k", kw.get(&key("d/k", "keyword"))).contains("threat model"));

        // Keyword fire, no span (pre-enrichment) → says so, invents nothing.
        let none = build_why_index(&model(vec![vec![fired("d/n", "keyword", None, None)]]));
        assert!(render_why_detail("d/n", none.get(&key("d/n", "keyword"))).contains("no span recorded"));
    }

    #[test]
    fn detail_handles_way_with_no_model_record() {
        assert!(render_why_detail("d/x", None).contains("no fire record"));
    }

    fn active(id: &str, epoch: u64) -> ActiveWay {
        ActiveWay {
            id: id.into(),
            trigger: "keyword".into(),
            epoch_fired: epoch,
            token_pos: 0,
            check_fires: 0,
            is_new: false,
            is_redisclosed: false,
            refire_threshold_k: 0,
        }
    }

    fn frame_of(ways: Vec<ActiveWay>) -> Frame {
        Frame {
            epoch: ways.iter().map(|w| w.epoch_fired).max().unwrap_or(0),
            timestamp: "2026-01-01T00:00:00Z".into(),
            elapsed_secs: 0,
            token_position_k: 0,
            ways,
            new_events: vec![],
            window: 1,
        }
    }

    #[test]
    fn load_session_events_sorts_by_timestamp() {
        // The union of log sources can present a recent event before an older one;
        // build_frames needs them chronological or its clustering/windows scramble.
        let content = concat!(
            r#"{"ts":"2026-01-02T00:00:00Z","event":"way_fired","session":"s","way":"d/late"}"#,
            "\n",
            r#"{"ts":"2026-01-01T00:00:00Z","event":"way_fired","session":"s","way":"d/early"}"#,
            "\n",
        );
        let evs = load_session_events(content, "s");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].ts, "2026-01-01T00:00:00Z", "earliest first");
        assert_eq!(evs[1].ts, "2026-01-02T00:00:00Z");
    }

    #[test]
    fn build_frames_segments_at_compaction_boundaries() {
        let ev = |ts: &str, event: &str, way: &str| WayEvent {
            ts: ts.into(),
            event: event.into(),
            way: way.into(),
            trigger: "keyword".into(),
            check: String::new(),
        };
        // Window 1: origin session_start + two fires. A second session_start
        // (a compaction) opens window 2, which starts fresh with one fire.
        let events = vec![
            ev("2026-01-01T00:00:00Z", "session_start", ""),
            ev("2026-01-01T00:00:01Z", "way_fired", "d/a"),
            ev("2026-01-01T00:01:00Z", "way_fired", "d/b"),
            ev("2026-01-01T02:00:00Z", "session_start", ""),
            ev("2026-01-01T02:00:01Z", "way_fired", "d/c"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);

        // The latest window reset the accumulated ways and restarted epoch numbering.
        let last = frames.last().unwrap();
        assert_eq!(last.window, 2);
        let ids: Vec<&str> = last.ways.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["d/c"], "window 2 shows only its own fires (reset)");
        assert!(last.epoch <= 2, "epoch restarted per window, not continued from w1");

        // Window 1 still accumulated both of its ways.
        let w1 = frames.iter().find(|f| f.window == 1 && f.ways.len() == 2).unwrap();
        assert!(w1.ways.iter().any(|w| w.id == "d/a"));
        assert!(w1.ways.iter().any(|w| w.id == "d/b"));

        // The boundary frame carries a compaction marker.
        assert!(frames
            .iter()
            .any(|f| f.new_events.iter().any(|e| e.contains("compaction"))));
    }

    #[test]
    fn redisclosure_repopulates_a_reset_window() {
        let ev = |ts: &str, event: &str, way: &str| WayEvent {
            ts: ts.into(),
            event: event.into(),
            way: way.into(),
            trigger: "keyword".into(),
            check: String::new(),
        };
        // A way fires in window 1; after a compaction, it only *re-discloses* (no
        // fresh fire) in window 2 — as a mature window mostly does. It must still show
        // in window 2, or the current window looks empty (the regression this fixes).
        let events = vec![
            ev("2026-01-01T00:00:00Z", "session_start", ""),
            ev("2026-01-01T00:00:01Z", "way_fired", "d/a"),
            ev("2026-01-01T02:00:00Z", "session_start", ""),
            ev("2026-01-01T02:00:01Z", "way_redisclosed", "d/a"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let last = frames.last().unwrap();
        assert_eq!(last.window, 2);
        assert!(
            last.ways.iter().any(|w| w.id == "d/a"),
            "a redisclosure must repopulate the reset window"
        );
    }

    #[test]
    fn anchor_keeps_same_way_when_still_active() {
        // Moving forward (or a frame where the way is still present) → exact id match.
        let f = frame_of(vec![active("a", 1), active("b", 3), active("c", 5)]);
        assert_eq!(reselect_by_anchor(&f, "b", 3), 1);
    }

    #[test]
    fn justify_spreads_segments_to_full_width() {
        let segs = vec!["a".to_string(), "bb".to_string(), "c".to_string()];
        let line = justify(&segs, 20);
        assert_eq!(compositor::visible_len(&line), 20, "fills the full width");
        assert!(line.starts_with('a'), "first segment hugs the left");
        assert!(line.ends_with('c'), "last segment hugs the right");
        // Content wider than the width → single-space join, no panic.
        let tight = justify(&segs, 3);
        assert_eq!(tight, "a bb c");
    }

    #[test]
    fn friendly_ts_trims_to_minute() {
        assert_eq!(friendly_ts("2026-07-03T16:52:00Z"), "2026-07-03 16:52");
        assert_eq!(friendly_ts("bogus"), "bogus"); // too short → returned as-is
    }

    #[test]
    fn compose_screen_is_fixed_height_with_nav_pinned() {
        let top = vec!["h1".to_string(), "h2".to_string()];
        let nav = vec!["sep".to_string(), "nav".to_string()];

        // Long list, short screen → exactly `drawable` lines, nav on the last line.
        let rows: Vec<String> = (0..50).map(|i| format!("row{i}")).collect();
        let out = compose_screen(top.clone(), rows, 40, vec![], nav.clone(), 20);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 20, "composes to exactly the drawable height");
        assert_eq!(lines[0], "h1", "header pinned at the top");
        assert_eq!(lines[19], "nav", "nav pinned to the last line even when overflowing");

        // Short list → the middle is blank-padded so the nav still sits at the bottom.
        let out2 = compose_screen(top, vec!["only".to_string()], 0, vec![], nav, 20);
        let l2: Vec<&str> = out2.split('\n').collect();
        assert_eq!(l2.len(), 20);
        assert_eq!(l2[2], "only");
        assert_eq!(l2[19], "nav", "nav stays anchored to the bottom, not floating up");
        assert_eq!(l2[10], "", "the list area is padded with blanks");

        // Tiny terminal where the pinned header+nav alone exceed the height: the nav
        // must survive (the header is dropped instead of the footer).
        let tiny = compose_screen(
            vec!["h1".into(), "h2".into(), "h3".into()],
            vec!["r".into()],
            0,
            vec![],
            vec!["sep".into(), "nav".into()],
            4,
        );
        let tl: Vec<&str> = tiny.split('\n').collect();
        assert_eq!(tl.len(), 4, "never exceeds the drawable height");
        assert_eq!(tl[3], "nav", "footer survives; the header is clipped instead");
    }

    #[test]
    fn anchor_snaps_to_nearest_lesser_epoch_when_rewound_out() {
        // Rewound frame missing "c" (epoch 5): anchor c@5 → nearest present epoch ≤ 5
        // is b@3 (row 1). Frames are epoch-ascending, so it's the last such row.
        let f = frame_of(vec![active("a", 1), active("b", 3)]);
        assert_eq!(reselect_by_anchor(&f, "c", 5), 1);
        // Anchor epoch below everything present → first row.
        assert_eq!(reselect_by_anchor(&f, "z", 0), 0);
    }

    #[test]
    fn read_way_body_preserves_leading_dashes_and_handles_edges() {
        let base = std::env::temp_dir().join(format!("ways-body-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let write_read = |name: &str, content: &str| {
            let p = base.join(name);
            std::fs::write(&p, content).unwrap();
            read_way_body(p.to_str().unwrap()).unwrap()
        };
        // A body opening with a markdown list keeps its leading dashes.
        assert_eq!(
            write_read("list.md", "---\ndescription: d\n---\n- one\n- two\n"),
            "- one\n- two\n"
        );
        // Empty frontmatter → body is everything after the closing fence.
        assert_eq!(write_read("empty.md", "---\n---\nbody line\n"), "body line\n");
        // No opening fence → the whole file is the body.
        assert_eq!(write_read("nofm.md", "just text\nmore\n"), "just text\nmore\n");
        let _ = std::fs::remove_dir_all(&base);
    }
}
