//! Key handling for the two views, plus the cursor-anchor logic that keeps the
//! selected way put as frames change.

use crossterm::event::{KeyCode, KeyEvent};

use super::model::{Frame, Player, View, SPEEDS};

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
///
/// The epoch comparison is only meaningful *within* a window (epochs restart per
/// compaction window). Across a window boundary the id-match usually resolves it, and
/// the epoch fallback is just cursor placement — never correctness — so a cross-window
/// mismatch at worst lands the cursor on a reasonable nearby row.
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
pub(super) fn handle_timeline_key(player: &mut Player, key: KeyEvent) {
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
pub(super) fn handle_why_key(player: &mut Player, key: KeyEvent) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::model::ActiveWay;

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
    fn anchor_keeps_same_way_when_still_active() {
        // Moving forward (or a frame where the way is still present) → exact id match.
        let f = frame_of(vec![active("a", 1), active("b", 3), active("c", 5)]);
        assert_eq!(reselect_by_anchor(&f, "b", 3), 1);
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
}
