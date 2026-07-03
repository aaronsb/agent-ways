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
    term_width: u16,
    term_height: u16,
    /// Current view; the drill-down state below is only meaningful in `WhyFired`.
    view: View,
    /// The per-way "why" index, built lazily on first entry to the drill-down so a
    /// plain replay never pays for the model + transcript read.
    #[cfg(feature = "tui")]
    why_index: Option<WhyIndex>,
    /// Which fired way of the current frame is focused in the drill-down.
    why_selected: usize,
    /// Scroll offset into the focused way's detail panel.
    why_detail_scroll: usize,
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
        term_width,
        term_height,
        view: View::Timeline,
        why_index: None,
        why_selected: 0,
        why_detail_scroll: 0,
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

        let timeout = if player.playing {
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

                    // Tab toggles the why-fired drill-down. Entering it pauses
                    // playback and starts the focus at the top of the frame's ways.
                    KeyEvent { code: KeyCode::Tab, .. } => {
                        player.view = match player.view {
                            View::Timeline => View::WhyFired,
                            View::WhyFired => View::Timeline,
                        };
                        player.playing = false;
                        player.why_selected = 0;
                        player.why_detail_scroll = 0;
                    }

                    _ => match player.view {
                        View::Timeline => handle_timeline_key(player, key),
                        View::WhyFired => handle_why_key(player, key),
                    },
                }
            }
        } else if player.playing {
            if player.current < player.frames.len() - 1 {
                player.current += 1;
            } else {
                player.playing = false;
            }
        }
    }
    Ok(())
}

/// Timeline-view keys: frame navigation, play/pause, and speed.
#[cfg(feature = "tui")]
fn handle_timeline_key(player: &mut Player, key: KeyEvent) {
    match key {
        KeyEvent { code: KeyCode::Right, .. } | KeyEvent { code: KeyCode::Char('l'), .. } => {
            player.playing = false;
            if player.current < player.frames.len() - 1 {
                player.current += 1;
            }
        }
        KeyEvent { code: KeyCode::Left, .. } | KeyEvent { code: KeyCode::Char('h'), .. } => {
            player.playing = false;
            if player.current > 0 {
                player.current -= 1;
            }
        }
        KeyEvent { code: KeyCode::Char(' '), .. } => player.playing = !player.playing,
        KeyEvent { code: KeyCode::Up, .. } | KeyEvent { code: KeyCode::Char('k'), .. } => {
            if player.speed_idx < SPEEDS.len() - 1 {
                player.speed_idx += 1;
            }
        }
        KeyEvent { code: KeyCode::Down, .. } | KeyEvent { code: KeyCode::Char('j'), .. } => {
            if player.speed_idx > 0 {
                player.speed_idx -= 1;
            }
        }
        KeyEvent { code: KeyCode::Home, .. } | KeyEvent { code: KeyCode::Char('g'), .. } => {
            player.current = 0;
            player.playing = false;
        }
        KeyEvent { code: KeyCode::End, .. } | KeyEvent { code: KeyCode::Char('G'), .. } => {
            player.current = player.frames.len() - 1;
            player.playing = false;
        }
        _ => {}
    }
}

/// Drill-down keys: ▲▼ select a fired way, ◀▶ move between frames without leaving
/// the drill-down, PgUp/PgDn (or `[`/`]`) scroll the detail panel.
#[cfg(feature = "tui")]
fn handle_why_key(player: &mut Player, key: KeyEvent) {
    let ways_len = player.frames[player.current].ways.len();
    match key {
        KeyEvent { code: KeyCode::Up, .. } | KeyEvent { code: KeyCode::Char('k'), .. } => {
            player.why_selected = player.why_selected.saturating_sub(1);
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::Down, .. } | KeyEvent { code: KeyCode::Char('j'), .. } => {
            if player.why_selected + 1 < ways_len {
                player.why_selected += 1;
            }
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::Right, .. } | KeyEvent { code: KeyCode::Char('l'), .. } => {
            if player.current < player.frames.len() - 1 {
                player.current += 1;
            }
            player.why_selected = 0;
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::Left, .. } | KeyEvent { code: KeyCode::Char('h'), .. } => {
            if player.current > 0 {
                player.current -= 1;
            }
            player.why_selected = 0;
            player.why_detail_scroll = 0;
        }
        KeyEvent { code: KeyCode::PageDown, .. } | KeyEvent { code: KeyCode::Char(']'), .. } => {
            player.why_detail_scroll = player.why_detail_scroll.saturating_add(5);
        }
        KeyEvent { code: KeyCode::PageUp, .. } | KeyEvent { code: KeyCode::Char('['), .. } => {
            player.why_detail_scroll = player.why_detail_scroll.saturating_sub(5);
        }
        KeyEvent { code: KeyCode::Home, .. } | KeyEvent { code: KeyCode::Char('g'), .. } => {
            player.why_detail_scroll = 0;
        }
        _ => {}
    }
}

// ── Why-fired drill-down (ADR-154 §1, §2) ─────────────────────

/// Per-way "why it fired", aggregated from the `SessionIntrospection` model across
/// all of the way's fires in the session. Keyed by `way_id` — the join the
/// drill-down uses, per the ADR-154 §1 boundary (no epoch alignment). Criteria are
/// way-level (identical across a way's fires); matched spans are collected distinctly.
#[cfg(feature = "tui")]
struct WhyEntry {
    way_path: Option<String>,
    trigger_channel: String,
    fire_score: Option<f64>,
    criteria: MatchCriteria,
    matched_spans: Vec<String>,
}

#[cfg(feature = "tui")]
type WhyIndex = HashMap<String, WhyEntry>;

/// Fold the model's per-turn fired-ways into a `way_id → WhyEntry` index.
#[cfg(feature = "tui")]
fn build_why_index(model: &SessionIntrospection) -> WhyIndex {
    let mut idx: WhyIndex = HashMap::new();
    for turn in &model.turns {
        for fw in &turn.fired_ways {
            let e = idx.entry(fw.way_id.clone()).or_insert_with(|| WhyEntry {
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

/// Read a way file's body (everything after a leading frontmatter fence).
#[cfg(feature = "tui")]
fn read_way_body(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let body = match content.strip_prefix("---\n") {
        Some(rest) => match rest.find("\n---") {
            Some(end) => rest[end + 4..].trim_start_matches(['\n', '-']).to_string(),
            None => content,
        },
        None => content,
    };
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
    let body_h = (player.term_height as usize).saturating_sub(6).max(3);
    let gap = 2;
    let left_w = (total_w / 3).clamp(16, 40).min(total_w.saturating_sub(gap + 12).max(16));
    let right_w = total_w.saturating_sub(left_w + gap).max(12);

    // Build both panels while borrowing the index + frame, then drop those borrows
    // before the scroll write-backs below.
    let epoch;
    let (left, right) = {
        let idx = player.why_index.as_ref().unwrap();
        let frame = &player.frames[player.current];
        epoch = frame.epoch;

        let mut left_lines: Vec<String> = Vec::with_capacity(frame.ways.len());
        for (i, w) in frame.ways.iter().enumerate() {
            // A filled bullet marks a way the model has a fire record for.
            let bullet = if idx.contains_key(&w.id) { "•" } else { "·" };
            let line = format!("{bullet} {}", w.id);
            if i == sel {
                left_lines.push(format!("\x1b[7m{}\x1b[0m", compositor::fit_visible(&line, left_w)));
            } else {
                left_lines.push(line);
            }
        }
        if left_lines.is_empty() {
            left_lines.push("\x1b[2m(no ways in this frame)\x1b[0m".to_string());
        }

        let detail = frame
            .ways
            .get(sel)
            .map(|w| render_why_detail(&w.id, idx.get(&w.id)))
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

    let mut out = String::new();
    let short_id = &player.session_id[..player.session_id.len().min(12)];
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{}  \x1b[2mSession {short_id}… · epoch {epoch} · frame {}/{}\x1b[0m",
        compositor::tab_bar(&["Timeline", "Why fired"], 1),
        player.current + 1,
        player.frames.len(),
    );
    let _ = writeln!(out);
    for line in composited {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "\x1b[2m{}\x1b[0m", "─".repeat(total_w.min(85)));
    let _ = write!(
        out,
        " \x1b[7m ▲▼ \x1b[0m way  \x1b[7m ◀ ▶ \x1b[0m frame  \x1b[7m PgUp/Dn \x1b[0m scroll  \x1b[7m tab \x1b[0m timeline  \x1b[7m esc \x1b[0m quit"
    );
    out
}

// ── Frame renderer ────────────────────────────────────────────

fn render_frame(player: &Player) -> String {
    let frame = &player.frames[player.current];
    let current_epoch = frame.epoch;
    let context_window_k = player.context_window_k;
    let current_tokens_k = frame.token_position_k;

    let mut out = String::new();

    // Header
    let short_id = &player.session_id[..player.session_id.len().min(12)];
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "\x1b[1mSession\x1b[0m {short_id}...  \x1b[2mepoch {current_epoch} · {context_window_k}K ctx · {} ways fired\x1b[0m",
        frame.ways.len()
    );
    let _ = writeln!(
        out,
        "\x1b[2m  {} · +{}s elapsed\x1b[0m",
        &frame.timestamp[..frame.timestamp.len().min(19)],
        frame.elapsed_secs
    );
    let _ = writeln!(out);

    if frame.ways.is_empty() {
        let _ = writeln!(out, "  \x1b[2mNo ways triggered yet.\x1b[0m");
        let _ = writeln!(out);
        render_status_bar(&mut out, player);
        return out;
    }

    let bar_positions = render::compute_bar_positions(&frame.ways, context_window_k);
    let unique_pos = render::unique_positions(&bar_positions);

    render::write_table_header(&mut out);

    for (i, w) in frame.ways.iter().enumerate() {
        let (prefix, suffix) = if w.is_new {
            ("\x1b[1;32m", "\x1b[0m")
        } else if w.is_redisclosed {
            ("\x1b[1;36m", "\x1b[0m")
        } else {
            ("", "")
        };

        render::write_way_row(
            &mut out, w, current_epoch, current_tokens_k,
            &bar_positions, &unique_pos, i, prefix, suffix,
        );
    }

    if current_tokens_k > 0 {
        let _ = writeln!(out);
        render::write_token_timeline(
            &mut out, &frame.ways, &unique_pos,
            current_tokens_k, context_window_k,
        );
    }

    let _ = writeln!(out);

    // New events this frame
    if !frame.new_events.is_empty() {
        let _ = writeln!(out, "  \x1b[1;32m+ {}\x1b[0m", frame.new_events.join(", "));
        let _ = writeln!(out);
    }

    render_status_bar(&mut out, player);
    out
}

fn render_status_bar(out: &mut String, player: &Player) {
    let total = player.frames.len();
    let current = player.current + 1;
    let speed_label = SPEEDS[player.speed_idx].1;
    let state = if player.playing { "▶ playing" } else { "⏸ paused" };

    let _ = writeln!(out, "\x1b[2m{}\x1b[0m", "─".repeat(85));
    let _ = write!(
        out,
        " \x1b[7m ◀ ▶ \x1b[0m frame  \
         \x1b[7m ⏵ \x1b[0m play/pause  \
         \x1b[7m ▲▼ \x1b[0m speed  \
         \x1b[7m esc \x1b[0m quit  \
         \x1b[2m│\x1b[0m  \
         \x1b[1m{current}/{total}\x1b[0m  \
         {speed_label}  \
         {state}"
    );
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
        epoch += 1;
        let cluster_ts = cluster[0].ts.clone();
        let cluster_secs = parse_ts_secs(&cluster_ts);
        let elapsed = cluster_secs.saturating_sub(start_secs);

        let token_k = find_token_position(token_timeline, &cluster_ts);

        let mut new_events: Vec<String> = Vec::new();

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
                        active_ways.entry(ev.way.clone()).and_modify(|w| {
                            w.epoch_fired = epoch;
                            w.token_pos = token_k * 1000;
                            w.is_redisclosed = true;
                            w.is_new = false;
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
    content
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
        .collect()
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
        result.push_str(&truncate_visible(line, width));
        result.push_str("\r\n");
    }
    result
}

/// Truncate a string to `max_visible` visible characters, preserving ANSI escapes.
fn truncate_visible(s: &str, max_visible: usize) -> String {
    let mut result = String::new();
    let mut visible = 0;
    let mut in_escape = false;

    for ch in s.chars() {
        if in_escape {
            result.push(ch);
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            result.push(ch);
            continue;
        }
        if visible >= max_visible {
            break;
        }
        result.push(ch);
        visible += 1;
    }
    if result.contains('\x1b') {
        result.push_str("\x1b[0m");
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

    #[test]
    fn why_index_folds_fires_and_dedups_spans() {
        let m = model(vec![
            vec![fired("d/a", "keyword", Some("commit"), None)],
            vec![
                fired("d/a", "keyword", Some("commit"), None), // dup span across turns
                fired("d/a", "keyword", Some("stage"), None),  // new span
            ],
        ]);
        let idx = build_why_index(&m);
        let e = idx.get("d/a").expect("way indexed");
        assert_eq!(e.matched_spans, vec!["commit", "stage"], "deduped, in first-seen order");
    }

    #[test]
    fn detail_labels_semantic_and_missing_spans_honestly() {
        // Semantic fire → names the embedding + score, never a fabricated term.
        let sem = build_why_index(&model(vec![vec![fired(
            "d/s", "semantic:embedding:en", None, Some(0.73),
        )]]));
        let out = render_why_detail("d/s", sem.get("d/s"));
        assert!(out.contains("no recoverable term"), "semantic honesty: {out}");
        assert!(out.contains("score 0.73"));

        // Keyword fire with a span → shows the quoted span.
        let kw = build_why_index(&model(vec![vec![fired(
            "d/k", "keyword", Some("threat model"), None,
        )]]));
        assert!(render_why_detail("d/k", kw.get("d/k")).contains("threat model"));

        // Keyword fire, no span (pre-enrichment) → says so, invents nothing.
        let none = build_why_index(&model(vec![vec![fired("d/n", "keyword", None, None)]]));
        assert!(render_why_detail("d/n", none.get("d/n")).contains("no span recorded"));
    }

    #[test]
    fn detail_handles_way_with_no_model_record() {
        assert!(render_why_detail("d/x", None).contains("no fire record"));
    }
}
