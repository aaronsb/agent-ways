//! Session enumeration, the `--list` table, and the interactive session picker.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::util::parse_ts_secs;

use super::scope::project_matches;

#[cfg(feature = "tui")]
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
#[cfg(feature = "tui")]
use std::fmt::Write as FmtWrite;
#[cfg(feature = "tui")]
use std::io::Write;

#[cfg(feature = "tui")]
use super::layout::fit_to_terminal;
#[cfg(feature = "tui")]
use super::model::TermGuard;

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

pub(super) fn list_sessions(content: &str, project_filter: Option<&str>) -> Result<()> {
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
pub(super) fn pick_session(content: &str, project_filter: Option<&str>) -> Option<String> {
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
pub(super) fn pick_session(_content: &str, _project_filter: Option<&str>) -> Option<String> {
    eprintln!("Interactive picker requires the 'tui' feature.");
    None
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
