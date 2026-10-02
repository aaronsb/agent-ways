//! `ways session` — the user/agent-facing surface over a session's way
//! firings (ADR-154). Modes: `replay` and `live`, timeline screens on
//! agent-tui (ADR-504 §1, §9), with `replay --json` as the replay's CLI form;
//! `list`, the session table or `--json`; `dump`, the `SessionIntrospection`
//! model as JSON; `fires`, the semantic fires by score.
//!
//! - [`model`] — the replay data types (`WayEvent`, `ActiveWay`, `Frame`).
//! - [`scope`] — project-scope resolution and matching.
//! - [`frames`] — frame reconstruction and event/token loading.
//! - [`sessions`] — session enumeration, the `list` table, transcript discovery.
//! - [`dump`] — `replay --json` and `list --json`.
//! - [`why`] — the why-fired index and detail.
//! - [`table`] — the ways table and context lines on agent-tui.
//! - [`screen`] — the picker, timeline and why-fired screens.

mod dump;
mod frames;
mod model;
mod scope;
mod screen;
mod sessions;
mod table;
mod why;

#[cfg(test)]
mod tests;

use std::io::IsTerminal;

use anyhow::{bail, Result};

use agent_theme::ColorDepth;
use agent_tui::theme::{Palette, Shape};

use crate::session;
pub(crate) use model::Frame;
use screen::{Introspect, Picker, Replay};

/// A session id shortened for a table or a message: its first 12
/// characters. Cut by characters, since an id from the command line or the
/// log need not be ASCII.
pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

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
    fn headless(&self) -> bool {
        !self.keys.is_empty() || self.snap.is_some()
    }
}

fn depth_of(s: Option<&str>) -> Result<ColorDepth> {
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
fn look(depth: ColorDepth) -> (Palette, Shape) {
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

/// Show the screens: on the terminal until they close, or headless.
fn show(mut screen: Introspect, open: &Open) -> Result<()> {
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

fn need_terminal(open: &Open, mode: &str) -> Result<()> {
    if !open.headless() && !(std::io::stdout().is_terminal() && std::io::stdin().is_terminal()) {
        bail!("`ways session {mode}` needs a terminal; `ways session replay --json` prints a session's timeline");
    }
    Ok(())
}

/// `ways session replay` — a session's way firings frame by frame. With
/// no `--session`, the picker lists the sessions in scope. `--json` prints
/// the timeline instead.
pub fn replay(session: Option<&str>, project: Option<&str>, all: bool, speed: Option<u64>, json: bool, open: &Open) -> Result<()> {
    if json {
        return dump::replay_json(session, project, all);
    }
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }
    let scope = scope::resolve_project_scope(project, all)?;
    need_terminal(open, "replay")?;
    let (palette, shape) = look(depth_of(open.depth.as_deref())?);
    let with_speed = move |mut r: Replay| {
        if let Some(ms) = speed {
            r.play = r.play.clone().with_speed_ms(ms);
        }
        r
    };
    let screen = match session {
        Some(id) => match Replay::load(&content, id, None, false) {
            Ok(r) => Introspect::showing(with_speed(r), palette, shape),
            Err(e) => {
                println!("{e}");
                return Ok(());
            }
        },
        None => {
            let mut found = sessions::gather_sessions(&content, scope.as_deref());
            if found.is_empty() {
                println!("No sessions found.");
                return Ok(());
            }
            sessions::find_transcripts(&mut found, &ways_core::paths::claude_dir());
            let shown = scope.clone().unwrap_or_else(|| "every project".into());
            let opener: screen::Opener = Box::new(move |id| Replay::load(&content, id, None, false).map(with_speed));
            Introspect::picking(Picker::new(found, shown), opener, palette, shape)
        }
    };
    show(screen, open)
}

/// `ways session live` — monitor the current session's way firings, following
/// the newest frame as ways fire. The "current" session is the most recent one in
/// scope (the one actively writing events); `--session` overrides it. Scoping
/// mirrors `replay`: defaults to the current project, `--project` for a specific
/// one, and fails loud rather than silently globalizing when detection fails.
pub fn live(session: Option<&str>, project: Option<&str>, open: &Open) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }

    // Resolve which session to monitor:
    // - explicit `--session` wins;
    // - `--project` scopes to the latest session recorded under that project;
    // - otherwise the *most recently active* session anywhere — the one still
    //   appending events. We deliberately don't scope the default to a detected
    //   project: a long-running session's recorded project can be stale/wrong (the
    //   boundary hook logs `CLAUDE_PROJECT_DIR:-$PWD`), and "live" means "what's
    //   happening now," which is an activity signal, not a project one.
    let session_id = match (session, project) {
        (Some(s), _) => s.to_string(),
        (None, Some(p)) => match dump::most_recent_session(&content, Some(p)) {
            Some(s) => s,
            None => {
                println!("No sessions found for project {p}.");
                return Ok(());
            }
        },
        (None, None) => match dump::most_recent_active_session(&content) {
            Some(s) => s,
            None => {
                println!("No sessions found to monitor.");
                return Ok(());
            }
        },
    };

    // The project shown (and used for the transcript lookup) is where you launched
    // the monitor — CLAUDE_PROJECT_DIR, or the current directory — NOT the session's
    // recorded project, which the boundary hook may have mislabeled. For a live view,
    // "the project" is where you're working now.
    let launch_project = project.map_or_else(crate::util::project_dir, str::to_string);
    need_terminal(open, "live")?;
    let (palette, shape) = look(depth_of(open.depth.as_deref())?);
    match Replay::load(&content, &session_id, Some(&launch_project), true) {
        Ok(r) => show(Introspect::showing(r, palette, shape), open),
        Err(_) => {
            println!("No events for the current session yet.");
            Ok(())
        }
    }
}

/// `ways session list` — enumerate candidate sessions in scope, as a table or
/// (`--json`) machine-listable data for an agent to pick from before dumping.
pub fn list(project: Option<&str>, all: bool, json: bool) -> Result<()> {
    if json {
        return dump::run_list_json(project, all);
    }
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }
    let scope = scope::resolve_project_scope(project, all)?;
    sessions::list_sessions(&content, scope.as_deref())
}

/// `ways session dump` — emit a session's reconstructed introspection (turns,
/// fired ways, criteria, keyed transcript join, matched spans) as JSON, so an
/// agent can investigate *which ways fired, on which turn, and why* without a TUI.
///
/// Scoping mirrors `replay`: default the current project, `--project` for a
/// specific one, `--all` across every project (which only affects session
/// picking). With no `--session`, the most recent session in scope is dumped.
pub fn dump(session: Option<&str>, project: Option<&str>, all: bool, matched: bool) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("{{\"error\":\"no events recorded yet\"}}");
        return Ok(());
    }

    // Fail-loud scope resolution, as JSON (agent-facing).
    let scope = match scope::resolve_project_scope(project, all) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"error\":{}}}", serde_json::to_string(&e.to_string())?);
            return Ok(());
        }
    };

    let session_id = match session {
        Some(s) => s.to_string(),
        None => match dump::most_recent_session(&content, scope.as_deref()) {
            Some(s) => s,
            None => {
                println!("{{\"error\":\"no sessions found in scope\"}}");
                return Ok(());
            }
        }
    };

    // Project path drives the criteria corpus and the transcript slug. Prefer the
    // caller's scope (explicit `--project` or the detected current project) — it's
    // authoritative — over the session's first recorded `session_start` project,
    // which can be a stray subagent/hook cwd. The transcript reader scans by
    // session id if the slug misses, so a wrong path here still resolves the join.
    let project_path = scope
        .clone()
        .or_else(|| frames::find_session_project(&content, &session_id))
        .unwrap_or_default();
    let window_k = session::detect_context_window_for(&project_path, &session_id) / 1000;

    let model = ways_core::introspection::SessionIntrospection::from_session(
        &session_id,
        &project_path,
        window_k,
    );
    // What reached the session, unless asked for every matched candidate.
    let model = if matched { model } else { model.injected_only() };
    println!("{}", serde_json::to_string_pretty(&model)?);
    Ok(())
}

/// `ways session fires` — the read-side precision instrument (task #2 of the
/// ADR-160 calibration work). Reads `way_fired`/`way_redisclosed` events straight
/// from events.jsonl — no `SessionIntrospection` reconstruction, no re-embedding —
/// and prints each *semantic* fire as `score · surface · way`, borderline (lowest
/// score) first, so a human can judge whether the matcher fired on text that
/// actually warranted the way. Pair with `--max-score` to isolate the suspect tail
/// that gate calibration (task #5) has to defend.
pub fn fires(
    session: Option<&str>,
    project: Option<&str>,
    all: bool,
    max_score: Option<f64>,
    limit: Option<usize>,
    matched: bool,
) -> Result<()> {
    let content = ways_core::firing::load_events_text();
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }

    let scope = scope::resolve_project_scope(project, all)?;
    let session_id = match session {
        Some(s) => s.to_string(),
        None => match dump::most_recent_session(&content, scope.as_deref()) {
            Some(s) => s,
            None => {
                println!("No sessions found in scope.");
                return Ok(());
            }
        }
    };

    // Pull semantic fires for this session. A fire is semantic when its trigger
    // begins `semantic:` (`semantic:embedding:en|multi`); keyword/state fires have
    // no score or surface to eyeball, so they are out of scope for this view.
    let mut rows: Vec<(f64, String, String, bool)> = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v.get("session").and_then(|s| s.as_str()) != Some(session_id.as_str()) {
            continue;
        }
        let event = v.get("event").and_then(|e| e.as_str()).unwrap_or("");
        let redisclosed = match event {
            "way_fired" => false,
            "way_redisclosed" => true,
            _ => continue,
        };
        let trigger = v.get("trigger").and_then(|t| t.as_str()).unwrap_or("");
        if !trigger.starts_with("semantic:") {
            continue;
        }
        // `fire_score` is written as a formatted string field (see show::way_scored).
        let Some(score) = v.get("fire_score").and_then(|s| s.as_str()).and_then(|s| s.parse::<f64>().ok())
        else {
            continue;
        };
        if let Some(cap) = max_score {
            if score > cap {
                continue;
            }
        }
        let way = v.get("way").and_then(|w| w.as_str()).unwrap_or("?").to_string();
        // `surface` only rides fires logged after the read-side instrument shipped;
        // older events legitimately lack it — show a placeholder rather than drop them.
        let surface = v
            .get("surface")
            .and_then(|s| s.as_str())
            .unwrap_or("—")
            .to_string();
        rows.push((score, way, surface, redisclosed));
    }

    if rows.is_empty() {
        println!(
            "No semantic fires for session {} (keyword/state fires carry no score/surface).",
            short_id(&session_id)
        );
        if matched {
            print_judge_blocks(&ways_core::introspection::judge_blocks(&content, &session_id));
        }
        return Ok(());
    }

    // Borderline first: the lowest-scoring fires are the ones whose relevance is
    // most in question, so they lead the readout.
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let total = rows.len();
    let shown = limit.unwrap_or(total).min(total);

    println!(
        "{} semantic fire{} · session {} · lowest score first{}",
        total,
        if total == 1 { "" } else { "s" },
        short_id(&session_id),
        max_score.map(|c| format!(" · ≤ {c:.2}")).unwrap_or_default(),
    );
    for (score, way, surface, redisclosed) in rows.into_iter().take(shown) {
        let mark = if redisclosed { "↻" } else { " " };
        println!("  {score:.3} {mark} {way:<44}  {surface}");
    }
    if shown < total {
        println!("  … {} more (raise --limit)", total - shown);
    }
    if matched {
        print_judge_blocks(&ways_core::introspection::judge_blocks(&content, &session_id));
    }
    Ok(())
}

/// The ways the relevance judge kept out, each with its P(yes) against the
/// threshold: `--matched` adds them to a listing of what fired.
pub(crate) fn print_judge_blocks(blocks: &[ways_core::introspection::JudgeBlock]) {
    if blocks.is_empty() {
        println!("No way was kept out by the relevance judge.");
        return;
    }
    println!("{} kept out by the relevance judge:", blocks.len());
    for b in blocks {
        println!("  P(yes) {:.2} < {:.2}  {}", b.verdict.p_yes, b.verdict.threshold, b.way);
    }
}
