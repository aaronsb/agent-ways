//! Session enumeration: the sessions the event log records, the plain
//! table `introspect list` prints, and which of them Claude Code still has a
//! transcript for, found through claude-sessions.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use agent_fmt::when::parse_utc_iso;

use super::scope::project_matches;
use agent_theme::{paint, Role, Style};

#[derive(serde::Serialize)]
pub(crate) struct SessionInfo {
    pub(crate) id: String,
    pub(crate) ts: String,
    pub(crate) project: String,
    pub(crate) event_count: u32,
    pub(crate) way_fires: u32,
    pub(crate) duration_secs: u64,
    /// Whether Claude Code's transcript of the session is on disk: the
    /// token positions and the why-fired join read it. Filled by
    /// [`find_transcripts`]; not part of the `list --json` output.
    #[serde(skip)]
    pub(crate) transcript: bool,
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
                    transcript: false,
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
            // A stamp that is not UTC gives no duration rather than one from 1970.
            if let (Some(start), Some(end)) = (parse_utc_iso(&s.ts), parse_utc_iso(last)) {
                s.duration_secs = end.saturating_sub(start);
            }
        }
    }

    sessions
}

/// The plain session table of `introspect list`, newest first.
pub(super) fn list_sessions(content: &str, project_filter: Option<&str>) -> Result<()> {
    let mut sessions = gather_sessions(content, project_filter);
    find_transcripts(&mut sessions, &ways_core::paths::claude_dir());
    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!();
    println!(
        "{}",
        paint(
            Style::new().bold(),
            format!("{:<14} {:<20} {:<30} {:>6} {:>6} {:>8}  {}", "Session", "Date", "Project", "Events", "Ways", "Duration", "Transcript")
        )
    );
    println!("{}", paint(Role::Muted, "─".repeat(102)));

    for s in sessions.iter().rev().take(50) {
        let short_id = &s.id[..s.id.len().min(12)];
        let date = &s.ts[..s.ts.len().min(16)];
        let project_short = s.project.split('/').next_back().unwrap_or(&s.project);
        let duration = agent_fmt::when::duration(s.duration_secs);
        let transcript = if s.transcript { paint(Role::Ok, "yes") } else { paint(Role::Muted, "gone") };
        println!(
            "  {:<12} {:<20} {:<30} {:>6} {:>6} {:>8}  {transcript}",
            short_id, date, project_short, s.event_count, s.way_fires, duration
        );
    }
    println!();
    println!(
        "{}",
        paint(
            Role::Muted,
            format!("  {} sessions total. `ways introspect replay --session <id>` replays one; without --session it opens the picker.", sessions.len())
        )
    );
    println!();
    Ok(())
}

/// Mark each session whose transcript Claude Code still holds. A project's
/// directory is looked up once (claude-sessions' encoder and lookup); a
/// session recorded under another directory, such as a subagent's cwd, is
/// found by the id scan [`claude_sessions::ClaudeDir::find_transcript`] falls
/// back to.
pub(crate) fn find_transcripts(sessions: &mut [SessionInfo], claude: &claude_sessions::ClaudeDir) {
    let mut dirs: HashMap<String, Option<std::path::PathBuf>> = HashMap::new();
    for s in sessions.iter_mut() {
        let dir = dirs.entry(s.project.clone()).or_insert_with(|| claude.find_project_dir(&s.project));
        s.transcript = match dir {
            Some(d) if d.join(format!("{}.jsonl", s.id)).is_file() => true,
            _ => claude.find_transcript(None, &s.id).is_some(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session's transcript is found in its project's directory, named by
    /// claude-sessions' encoder (an `_` becomes `-`), or by its id under
    /// another project; one Claude Code no longer holds is marked gone.
    #[test]
    fn transcripts_are_found_by_project_or_by_id() {
        let root = std::env::temp_dir().join(format!("ways-introspect-tx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("projects");
        let own = projects.join(claude_sessions::project_slug("/p/my_proj"));
        let other = projects.join("-elsewhere");
        for (d, id) in [(&own, "s1"), (&other, "s2")] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(format!("{id}.jsonl")), "{}\n").unwrap();
        }
        let content: String = ["s1", "s2", "s3"]
            .iter()
            .map(|id| format!("{{\"event\":\"session_start\",\"session\":\"{id}\",\"ts\":\"2026-01-01T00:00:00Z\",\"project\":\"/p/my_proj\"}}\n"))
            .collect();
        let mut sessions = gather_sessions(&content, None);
        find_transcripts(&mut sessions, &claude_sessions::ClaudeDir::at(&root));
        let found: Vec<(&str, bool)> = sessions.iter().map(|s| (s.id.as_str(), s.transcript)).collect();
        assert_eq!(found, [("s1", true), ("s2", true), ("s3", false)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A start stamp that is not UTC gives no duration, not one measured from 1970.
    #[test]
    fn a_non_utc_start_gives_no_duration() {
        let content = concat!(
            r#"{"event":"session_start","session":"s1","ts":"2026-01-01T00:00:00+02:00","project":"/p"}"#, "\n",
            r#"{"event":"way_fired","session":"s1","ts":"2026-01-01T00:01:00Z","way":"a/b"}"#, "\n",
        );
        let sessions = gather_sessions(content, Some("/p"));
        assert_eq!(sessions[0].duration_secs, 0);
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
}
