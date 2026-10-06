//! `ways session` — the user/agent-facing surface over a session's way
//! firings (ADR-154). Modes: `replay` and `live`, one timeline screen on
//! agent-tui (ADR-504 §1, §9; #780), with `replay --json` as the replay's CLI form;
//! `list`, the session table or `--json`; `dump`, the `SessionIntrospection`
//! model as JSON; `fires`, the semantic fires by score.
//!
//! - [`model`] — the replay data types (`WayEvent`, `ActiveWay`, `Frame`).
//! - [`agents`] — the agents that fired each way, named.
//! - [`scope`] — project-scope resolution and matching.
//! - [`frames`] — frame reconstruction and event/token loading.
//! - [`sessions`] — session enumeration, the `list` table, transcript discovery.
//! - [`live`] — which sessions are being written to, by stat on a backoff.
//! - [`dump`] — `replay --json` and `list --json`.
//! - [`why`] — the why-fired index and detail.
//! - [`table`] — the ways table and context lines on agent-tui.
//! - [`screen`] — the picker, timeline and why-fired screens.

mod agents;
mod dump;
mod frames;
mod model;
mod picker;
mod fires_tab;
mod live;
mod report;
mod scope;
mod screen;
mod sessions;
mod table;
mod why;

#[cfg(test)]
mod tests;

use std::io::IsTerminal;

use anyhow::{bail, Result};


use crate::session;
pub(crate) use model::Frame;
pub use crate::cmd::screen_host::Open;
use crate::cmd::screen_host::{depth_of, look, show};
use picker::Picker;
use screen::{Introspect, Replay};

/// A session id shortened for a table or a message: its first 12
/// characters. Cut by characters, since an id from the command line or the
/// log need not be ASCII.
pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
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
pub fn replay(session: Option<&str>, project: Option<&str>, all: bool, speed: Option<u64>, json: bool, matched: bool, open: &Open) -> Result<()> {
    if json {
        return dump::replay_json(session, project, all, matched);
    }
    let scope = scope::resolve_project_scope(project, all);
    let content = ways_core::firing::load_events_text_for(session, scope.as_ref().ok().and_then(|s| s.as_deref()));
    if content.trim().is_empty() {
        println!("No events recorded yet.");
        return Ok(());
    }
    let scope = scope?;
    need_terminal(open, "replay")?;
    let (palette, shape) = look(depth_of(open.depth.as_deref())?);
    let with_speed = move |mut r: Replay| {
        if let Some(ms) = speed {
            r.play = r.play.clone().with_speed_ms(ms);
        }
        r
    };
    let shown = scope.clone().unwrap_or_else(|| "every project".into());
    let reports = report::Reports::new(&content, scope.as_deref(), shown.clone());
    let screen = match session {
        // A session being written to opens following, as Enter opens it.
        Some(id) => match Replay::load(&content, id, None, transcript_live(&content, id)) {
            Ok(r) => Introspect::showing(with_speed(r), reports, palette, shape),
            Err(e) => {
                println!("{e}");
                return Ok(());
            }
        },
        None => {
            let Some(picker) = picker(&content, scope.as_deref(), shown) else {
                println!("No sessions found.");
                return Ok(());
            };
            let opener: screen::Opener = Box::new(move |id, live| Replay::load(&content, id, None, live).map(with_speed));
            Introspect::picking(picker, opener, reports, palette, shape)
        }
    };
    show(screen.into_app(), open)
}

/// The sessions in `scope`, newest first, their transcripts found and
/// stated once, re-stated on the backoff while the list is shown; the
/// session this process runs in marked. `None` when there are none.
fn picker(content: &str, scope: Option<&str>, shown: String) -> Option<Picker> {
    let mut found = sessions::gather_sessions(content, scope);
    if found.is_empty() {
        return None;
    }
    sessions::find_transcripts(&mut found, &ways_core::paths::claude_dir());
    let own = std::env::var("CLAUDE_CODE_SESSION_ID").ok().filter(|s| !s.is_empty());
    Some(Picker::new(found, shown).watching(live::system_stat(), live::system_clock()).own(own))
}

/// Whether session `id`'s transcript is being written to: one stat.
fn transcript_live(content: &str, id: &str) -> bool {
    let project = frames::find_session_project(content, id);
    let now = (live::system_clock())();
    ways_core::paths::claude_dir()
        .find_transcript(project.as_deref(), id)
        .and_then(|p| live::stat_file(&p))
        .is_some_and(|p| live::is_live(p.mtime_ms, now))
}

/// `ways session live` — monitor the current session's way firings, following
/// the newest frame as ways fire. The "current" session is the most recent one in
/// scope (the one actively writing events); `--session` overrides it. Scoping
/// mirrors `replay`: defaults to the current project, `--project` for a specific
/// one, and fails loud rather than silently globalizing when detection fails.
///
/// It is the replay screen opened on that session, following (#780): the
/// sessions tab lists the project's sessions behind it, and Esc goes there.
pub fn live(session: Option<&str>, project: Option<&str>, open: &Open) -> Result<()> {
    let project = project.map(ways_core::util::project_arg);
    let project = project.as_deref();
    let content = ways_core::firing::load_events_text_for(session, project);
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
        Ok(r) => {
            // The spend is scoped to the project's root, as judge calls record
            // it, though the monitor may have been launched in a subdirectory.
            let root = project.map(str::to_string).or_else(ways_core::util::project_root).unwrap_or_else(|| launch_project.clone());
            let reports = report::Reports::new(&content, Some(&root), root.clone());
            // The list behind it is the replay's: the project's sessions. When
            // the project cannot be told, the session shows alone, as before.
            let list = scope::resolve_project_scope(project, false).ok().and_then(|scope| {
                let shown = scope.clone().unwrap_or_else(|| "every project".into());
                picker(&content, scope.as_deref(), shown)
            });
            let screen = match list {
                Some(mut p) => {
                    p.select(&session_id);
                    let content = content.clone();
                    let opener: screen::Opener = Box::new(move |id, live| Replay::load(&content, id, None, live));
                    Introspect::picking(p, opener, reports, palette, shape).opened(r)
                }
                None => Introspect::showing(r, reports, palette, shape),
            };
            show(screen.into_app(), open)
        }
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
    let scope = scope::resolve_project_scope(project, all);
    let content = ways_core::firing::load_events_text_for(session, scope.as_ref().ok().and_then(|s| s.as_deref()));
    if content.trim().is_empty() {
        println!("{{\"error\":\"no events recorded yet\"}}");
        return Ok(());
    }

    // Fail-loud scope resolution, as JSON (agent-facing).
    let scope = match scope {
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
    json: bool,
) -> Result<()> {
    let scope = scope::resolve_project_scope(project, all);
    let content = ways_core::firing::load_events_text_for(session, scope.as_ref().ok().and_then(|s| s.as_deref()));
    if content.trim().is_empty() {
        if json {
            println!("{}", empty_fires_json(matched));
        } else {
            println!("No events recorded yet.");
        }
        return Ok(());
    }

    let scope = scope?;
    let session_id = match session {
        Some(s) => s.to_string(),
        None => match dump::most_recent_session(&content, scope.as_deref()) {
            Some(s) => s,
            None => {
                if json {
                    println!("{}", empty_fires_json(matched));
                } else {
                    println!("No sessions found in scope.");
                }
                return Ok(());
            }
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&fires_json(&content, &session_id, max_score, limit, matched))?);
    } else {
        print!("{}", fires_report(&content, &session_id, max_score, limit, matched));
    }
    Ok(())
}

/// One semantic fire of a session: its score, the way, the text it
/// matched, and whether it was a re-disclosure.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct SemanticFire {
    pub(crate) score: f64,
    pub(crate) way: String,
    pub(crate) surface: String,
    pub(crate) redisclosed: bool,
}

/// The semantic fires of `session_id` in the event log `content`, lowest
/// score first: the borderline fires, whose relevance is most in question,
/// lead. A fire is semantic when its trigger begins `semantic:`
/// (`semantic:embedding:en|multi`); keyword and state fires carry no score
/// or surface.
pub(crate) fn semantic_fires(content: &str, session_id: &str) -> Vec<SemanticFire> {
    let mut rows = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if v.get("session").and_then(|s| s.as_str()) != Some(session_id) {
            continue;
        }
        let redisclosed = match v.get("event").and_then(|e| e.as_str()).unwrap_or("") {
            "way_fired" => false,
            "way_redisclosed" => true,
            _ => continue,
        };
        if !v.get("trigger").and_then(|t| t.as_str()).unwrap_or("").starts_with("semantic:") {
            continue;
        }
        // `fire_score` is written as a formatted string field (see show::way_scored).
        let Some(score) = v.get("fire_score").and_then(|s| s.as_str()).and_then(|s| s.parse::<f64>().ok()) else {
            continue;
        };
        let way = v.get("way").and_then(|w| w.as_str()).unwrap_or("?").to_string();
        // `surface` only rides fires logged after the read-side instrument shipped;
        // older events lack it, and show a placeholder rather than drop out.
        let surface = v.get("surface").and_then(|s| s.as_str()).unwrap_or("—").to_string();
        rows.push(SemanticFire { score, way, surface, redisclosed });
    }
    rows.sort_by(|a, b| a.score.total_cmp(&b.score));
    rows
}

/// The `fires` JSON when there is no session to list: the same keys, empty.
fn empty_fires_json(matched: bool) -> serde_json::Value {
    let mut out = serde_json::json!({"session": null, "total": 0, "fires": []});
    if matched {
        out["judge_blocks"] = serde_json::json!([]);
    }
    out
}

/// The `fires` listing as JSON: the session, its semantic fires lowest
/// score first after `max_score` and `limit`, how many there were before
/// the limit, and with `matched` the ways the relevance judge kept out.
fn fires_json(content: &str, session_id: &str, max_score: Option<f64>, limit: Option<usize>, matched: bool) -> serde_json::Value {
    let mut fires = semantic_fires(content, session_id);
    fires.retain(|f| max_score.is_none_or(|cap| f.score <= cap));
    let total = fires.len();
    fires.truncate(limit.unwrap_or(total));
    let mut out = serde_json::json!({"session": session_id, "total": total, "fires": fires});
    if matched {
        out["judge_blocks"] = serde_json::json!(ways_core::introspection::judge_blocks(content, session_id));
    }
    out
}

/// The `fires` listing of `session_id` from the event log `content`, and
/// with `matched` the ways the relevance judge kept out.
fn fires_report(content: &str, session_id: &str, max_score: Option<f64>, limit: Option<usize>, matched: bool) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let mut rows = semantic_fires(content, session_id);
    rows.retain(|f| max_score.is_none_or(|cap| f.score <= cap));

    if rows.is_empty() {
        let _ = writeln!(
            out,
            "No semantic fires for session {} (keyword/state fires carry no score/surface).",
            short_id(session_id)
        );
        if matched {
            out.push_str(&judge_blocks_text(&ways_core::introspection::judge_blocks(content, session_id)));
        }
        return out;
    }

    let total = rows.len();
    let shown = limit.unwrap_or(total).min(total);

    let _ = writeln!(
        out,
        "{} semantic fire{} · session {} · lowest score first{}",
        total,
        if total == 1 { "" } else { "s" },
        short_id(session_id),
        max_score.map(|c| format!(" · ≤ {c:.2}")).unwrap_or_default(),
    );
    for f in rows.into_iter().take(shown) {
        let mark = if f.redisclosed { "↩" } else { " " };
        let _ = writeln!(out, "  {:.3} {mark} {:<44}  {}", f.score, f.way, f.surface);
    }
    if shown < total {
        let _ = writeln!(out, "  … {} more (raise --limit)", total - shown);
    }
    if matched {
        out.push_str(&judge_blocks_text(&ways_core::introspection::judge_blocks(content, session_id)));
    }
    out
}

/// The ways the relevance judge kept out, each with its P(yes) against the
/// threshold: `--matched` adds them to a listing of what fired.
pub(crate) fn print_judge_blocks(blocks: &[ways_core::introspection::JudgeBlock]) {
    print!("{}", judge_blocks_text(blocks));
}

fn judge_blocks_text(blocks: &[ways_core::introspection::JudgeBlock]) -> String {
    if blocks.is_empty() {
        return "No way was kept out by the relevance judge in this session.\n".into();
    }
    let mut out = format!("{} kept out by the relevance judge in this session:\n", blocks.len());
    for b in blocks {
        let with = b.verdict.ancestor.as_deref().map(|a| format!(" (with {a})")).unwrap_or_default();
        out.push_str(&format!("  P(yes) {:.2} < {:.2}  {}{with}\n", b.verdict.p_yes, b.verdict.threshold, b.way));
    }
    out
}

#[cfg(test)]
mod fires_tests {
    use super::{fires_json, fires_report};

    const LOG: &str = concat!(
        r#"{"event":"way_judged","session":"s","ts":"2026-01-01T00:00:00Z","way":"d/a","p_yes":"0.900","threshold":"0.30","verdict":"pass"}"#, "\n",
        r#"{"event":"way_fired","session":"s","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"semantic:embedding:en","fire_score":"0.410","surface":"prompt"}"#, "\n",
        r#"{"event":"way_fired","session":"s","ts":"2026-01-01T00:00:01Z","way":"d/e","trigger":"semantic:embedding:en","fire_score":"0.700","surface":"later"}"#, "\n",
        r#"{"event":"way_judged","session":"s","ts":"2026-01-01T00:00:00Z","way":"d/b","p_yes":"0.050","threshold":"0.30","verdict":"block"}"#, "\n",
        r#"{"event":"way_judged","session":"s","ts":"2026-01-01T00:00:00Z","way":"d/b/c","p_yes":"0.050","threshold":"0.30","verdict":"block","reason":"ancestor","ancestor":"d/b"}"#, "\n",
    );

    /// `fires` lists what fired; `--matched` adds what the judge kept out.
    #[test]
    fn fires_matched_adds_the_judges_blocks() {
        let plain = fires_report(LOG, "s", None, None, false);
        assert!(plain.contains("0.410") && plain.contains("d/a") && !plain.contains("kept out"), "{plain}");
        let matched = fires_report(LOG, "s", None, None, true);
        assert!(matched.starts_with(&plain), "{matched}");
        assert!(matched.contains("2 kept out by the relevance judge in this session:\n  P(yes) 0.05 < 0.30  d/b\n  P(yes) 0.05 < 0.30  d/b/c (with d/b)\n"), "{matched}");
        let none = fires_report(LOG, "other", None, None, true);
        assert!(none.contains("No semantic fires") && none.contains("No way was kept out"), "{none}");
    }

    /// The text marks a re-disclosure `↩`, as the screen does; the JSON
    /// carries it as `redisclosed`.
    #[test]
    fn a_re_disclosure_is_marked_as_the_screen_marks_it() {
        let log = concat!(
            r#"{"event":"way_fired","session":"s","ts":"2026-01-01T00:00:00Z","way":"d/a","trigger":"semantic:embedding:en","fire_score":"0.410","surface":"prompt"}"#, "\n",
            r#"{"event":"way_redisclosed","session":"s","ts":"2026-01-01T00:09:00Z","way":"d/a","trigger":"semantic:embedding:en","fire_score":"0.520","surface":"again"}"#, "\n",
        );
        let text = fires_report(log, "s", None, None, false);
        assert!(text.contains("  0.410   d/a") && text.contains("  0.520 ↩ d/a"), "{text}");
        assert!(!text.contains('↻'), "{text}");
        let j = fires_json(log, "s", None, None, false);
        assert_eq!(j["fires"][1]["redisclosed"], true, "{j}");
    }

    /// The JSON form carries what the text does, as data: the fires lowest
    /// score first, the count before `--limit`, and with `--matched` the
    /// judge's blocks.
    #[test]
    fn fires_json_lists_the_fires_and_the_judges_blocks() {
        let j = fires_json(LOG, "s", None, Some(1), true);
        assert_eq!(j["session"], "s");
        let fires = j["fires"].as_array().unwrap();
        assert_eq!(fires.len(), 1, "--limit cuts the list: {j}");
        assert_eq!(j["total"], 2, "the count is taken before the limit");
        assert_eq!(fires[0]["way"], "d/a");
        assert_eq!(fires[0]["score"], 0.41);
        let blocks = j["judge_blocks"].as_array().unwrap();
        assert_eq!(blocks.iter().map(|b| b["way"].as_str().unwrap()).collect::<Vec<_>>(), ["d/b", "d/b/c"]);
        assert!(fires_json(LOG, "s", None, None, false).get("judge_blocks").is_none());
        let capped = fires_json(LOG, "s", Some(0.5), None, false);
        assert_eq!(capped["total"], 1, "--max-score drops the 0.700 fire: {capped}");
        assert_eq!(capped["fires"][0]["way"], "d/a");
        // No session: the same keys, empty.
        assert_eq!(super::empty_fires_json(true), serde_json::json!({"session": null, "total": 0, "fires": [], "judge_blocks": []}));
    }
}
