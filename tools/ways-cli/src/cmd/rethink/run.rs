//! Entry points and the interactive/live loops: `ways introspect` replay (`run`)
//! and the `live` monitor (`run_live`), plus the TUI event loop.

use anyhow::Result;

use super::scope::resolve_project_scope;
use super::sessions::list_sessions;

#[cfg(feature = "tui")]
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
#[cfg(feature = "tui")]
use std::io::Write;

#[cfg(feature = "tui")]
use crate::session;

#[cfg(feature = "tui")]
use super::drilldown::render_why;
#[cfg(feature = "tui")]
use super::frames::{find_session_project, load_session_events, reconstruct_frames};
#[cfg(feature = "tui")]
use super::keys::{handle_timeline_key, handle_why_key};
#[cfg(feature = "tui")]
use super::layout::{fit_to_terminal, render_frame};
#[cfg(feature = "tui")]
use super::model::{Player, View, SPEEDS, TermGuard};
#[cfg(feature = "tui")]
use super::sessions::pick_session;

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
    // Prefer the caller's project (where the monitor was launched) over the session's
    // recorded project — the latter can be mislabeled by the boundary hook, and for a
    // live view the launch directory is the right context for the label + transcript.
    let project_name = project
        .map(str::to_string)
        .or_else(|| find_session_project(&content, session_id))
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
