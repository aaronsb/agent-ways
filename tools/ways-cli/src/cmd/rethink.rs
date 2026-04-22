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

use anyhow::Result;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;

use crate::cmd::render::{self, WayRow};
use crate::session;
use crate::util::{detect_project_dir, home_dir};

// ── Data structures ───────────────────────────────────────────

/// A way event from events.jsonl.
struct WayEvent {
    ts: String,
    event: String,
    way: String,
    trigger: String,
    check: String,
}

/// An active way at a given frame.
#[derive(Clone)]
struct ActiveWay {
    id: String,
    trigger: String,
    epoch_fired: u64,
    token_pos: u64,
    check_fires: u64,
    is_new: bool,
    is_redisclosed: bool,
    refire_threshold_k: u64,
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
struct Frame {
    epoch: u64,
    timestamp: String,
    elapsed_secs: u64,
    token_position_k: u64,
    ways: Vec<ActiveWay>,
    new_events: Vec<String>,
}

/// Playback state.
struct Player {
    frames: Vec<Frame>,
    current: usize,
    playing: bool,
    speed_idx: usize,
    session_id: String,
    context_window_k: u64,
    term_width: u16,
    term_height: u16,
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

// ── Entry point ───────────────────────────────────────────────

#[cfg(feature = "tui")]
pub fn run(session: Option<&str>, project: Option<&str>, speed: Option<u64>, list: bool) -> Result<()> {
    let events_file = home_dir().join(".claude/stats/events.jsonl");
    if !events_file.is_file() {
        println!("No events recorded yet.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&events_file)?;

    // Auto-detect project scope if not explicitly provided
    let detected_project = if project.is_none() {
        std::env::var("CLAUDE_PROJECT_DIR")
            .ok()
            .or_else(detect_project_dir)
    } else {
        None
    };
    let project_scope = project.map(|s| s.to_string()).or(detected_project);

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
    let token_timeline = build_token_timeline(&project_name, &session_id);
    // Pre-resolve per-way refire thresholds once per session. Rethink is
    // a replay, so if a way's curve has been edited since the recorded
    // session we'll see the current value — that's the best we can do
    // without also snapshotting frontmatter in events.jsonl.
    let fallback_refire_k = context_window_k * 25 / 100;
    let mut refire_cache: HashMap<String, u64> = HashMap::new();
    for ev in &events {
        if ev.way.is_empty() || refire_cache.contains_key(&ev.way) {
            continue;
        }
        let threshold_k = session::way_refire_threshold_k(&ev.way, &project_name, context_window)
            .unwrap_or(fallback_refire_k);
        refire_cache.insert(ev.way.clone(), threshold_k);
    }
    let frames = build_frames(&events, &token_timeline, &refire_cache, fallback_refire_k);

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
        context_window_k,
        term_width,
        term_height,
    };

    run_tui(&mut player)
}

#[cfg(not(feature = "tui"))]
pub fn run(_session: Option<&str>, _project: Option<&str>, _speed: Option<u64>, list: bool) -> Result<()> {
    if list {
        let events_file = home_dir().join(".claude/stats/events.jsonl");
        if !events_file.is_file() {
            println!("No events recorded yet.");
            return Ok(());
        }
        let content = std::fs::read_to_string(&events_file)?;
        return list_sessions(&content, None);
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

        let raw_output = render_frame(player);
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
                match key {
                    KeyEvent { code: KeyCode::Esc, .. }
                    | KeyEvent { code: KeyCode::Char('q'), .. } => break,

                    KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. } => break,

                    KeyEvent { code: KeyCode::Right, .. }
                    | KeyEvent { code: KeyCode::Char('l'), .. } => {
                        player.playing = false;
                        if player.current < player.frames.len() - 1 {
                            player.current += 1;
                        }
                    }

                    KeyEvent { code: KeyCode::Left, .. }
                    | KeyEvent { code: KeyCode::Char('h'), .. } => {
                        player.playing = false;
                        if player.current > 0 {
                            player.current -= 1;
                        }
                    }

                    KeyEvent { code: KeyCode::Char(' '), .. } => {
                        player.playing = !player.playing;
                    }

                    KeyEvent { code: KeyCode::Up, .. }
                    | KeyEvent { code: KeyCode::Char('k'), .. } => {
                        if player.speed_idx < SPEEDS.len() - 1 {
                            player.speed_idx += 1;
                        }
                    }

                    KeyEvent { code: KeyCode::Down, .. }
                    | KeyEvent { code: KeyCode::Char('j'), .. } => {
                        if player.speed_idx > 0 {
                            player.speed_idx -= 1;
                        }
                    }

                    KeyEvent { code: KeyCode::Home, .. }
                    | KeyEvent { code: KeyCode::Char('g'), .. } => {
                        player.current = 0;
                        player.playing = false;
                    }

                    KeyEvent { code: KeyCode::End, .. }
                    | KeyEvent { code: KeyCode::Char('G'), .. } => {
                        player.current = player.frames.len() - 1;
                        player.playing = false;
                    }

                    _ => {}
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

fn load_session_events(content: &str, session_id: &str) -> Vec<WayEvent> {
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

fn find_session_project(content: &str, session_id: &str) -> Option<String> {
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

struct SessionInfo {
    id: String,
    ts: String,
    project: String,
    event_count: u32,
    way_fires: u32,
    duration_secs: u64,
}

fn gather_sessions(content: &str, project_filter: Option<&str>) -> Vec<SessionInfo> {
    let mut sessions: Vec<SessionInfo> = Vec::new();
    let mut event_counts: HashMap<String, (u32, u32)> = HashMap::new();
    let mut last_ts: HashMap<String, String> = HashMap::new();

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
                if !project.contains(pf) {
                    continue;
                }
            }
            sessions.push(SessionInfo {
                id: sid.clone(),
                ts: ts.clone(),
                project,
                event_count: 0,
                way_fires: 0,
                duration_secs: 0,
            });
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

fn parse_ts_secs(ts: &str) -> u64 {
    if ts.len() < 19 {
        return 0;
    }
    let year: u64 = ts[0..4].parse().unwrap_or(0);
    let month: u64 = ts[5..7].parse().unwrap_or(0);
    let day: u64 = ts[8..10].parse().unwrap_or(0);
    let hour: u64 = ts[11..13].parse().unwrap_or(0);
    let min: u64 = ts[14..16].parse().unwrap_or(0);
    let sec: u64 = ts[17..19].parse().unwrap_or(0);

    let days = year * 365 + year / 4 + month * 30 + day;
    days * 86400 + hour * 3600 + min * 60 + sec
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
