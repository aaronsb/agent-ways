//! Frame reconstruction — cluster events into epoch frames, load the token
//! timeline, and read a session's events from the (unioned) event log.

use std::collections::HashMap;

use crate::cmd::render;
use crate::session;
use crate::util::{home_dir, parse_ts_secs};

use super::model::{ActiveWay, Frame, WayEvent};

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

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;

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
}
