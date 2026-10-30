//! Frame reconstruction — cluster events into epoch frames, load the token
//! timeline, and read a session's events from the (unioned) event log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::cmd::render;
use crate::session;
use agent_fmt::when::parse_utc_iso;

use super::model::{ActiveWay, Clock, Frame, Outcome, Suppression, WayEvent};

// ── Frame construction ────────────────────────────────────────

/// Reconstruct the full replay frame timeline for a session. Loads the token
/// timeline, pre-resolves per-way refire thresholds, and clusters events into
/// epoch frames. Shared by the replay screens and `session replay --json`.
///
/// Refire thresholds reflect each way's *current* curve — this is a replay,
/// so a curve edited since the recorded session shows today's value. That's the
/// best we can do without snapshotting frontmatter into events.jsonl.
///
/// A frame holds the injected ways, and with `matched` the ways the relevance
/// judge blocked too (#742). Each way carries its [`Outcome`].
pub(crate) fn reconstruct_frames(
    events: &[WayEvent],
    project_name: &str,
    session_id: &str,
    context_window: u64,
    matched: bool,
) -> Vec<Frame> {
    let frames = reconstruct_all(events, project_name, session_id, context_window);
    if matched {
        frames
    } else {
        frames.iter().map(|f| f.shown(false)).collect()
    }
}

/// Every frame with every matched candidate, for the screens, which toggle
/// the view without reading the log again.
pub(crate) fn reconstruct_all(
    events: &[WayEvent],
    project_name: &str,
    session_id: &str,
    context_window: u64,
) -> Vec<Frame> {
    let context_window_k = context_window / 1000;
    let claude = ways_core::paths::claude_dir();
    let mut agents: Vec<String> = events.iter().map(fired_by).collect();
    agents.sort();
    agents.dedup();
    let timelines: HashMap<String, Vec<(String, u64)>> = agents
        .into_iter()
        .map(|agent| {
            let transcript = session::transcript_in(&claude, None, project_name, session_id, &agent);
            let timeline = transcript.map(|t| cached_token_timeline(&t)).unwrap_or_default();
            (agent, timeline)
        })
        .collect();
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
    build_frames_by_agent(events, &timelines, &refire_cache, fallback_refire_k)
}

/// [`build_frames_by_agent`] with main's token timeline only.
#[cfg(test)]
pub(super) fn build_frames(
    events: &[WayEvent],
    token_timeline: &[(String, u64)],
    refire_cache: &HashMap<String, u64>,
    fallback_refire_k: u64,
) -> Vec<Frame> {
    let timelines = HashMap::from([(MAIN.to_string(), token_timeline.to_vec())]);
    build_frames_by_agent(events, &timelines, refire_cache, fallback_refire_k)
}

/// The frames of `events`, each agent's token position read from its own
/// timeline in `timelines`, keyed by agent id; an agent with none is at 0.
pub(super) fn build_frames_by_agent(
    events: &[WayEvent],
    timelines: &HashMap<String, Vec<(String, u64)>>,
    refire_cache: &HashMap<String, u64>,
    fallback_refire_k: u64,
) -> Vec<Frame> {
    let refire_for = |way_id: &str| -> u64 {
        refire_cache.get(way_id).copied().unwrap_or(fallback_refire_k)
    };
    let mut frames: Vec<Frame> = Vec::new();
    // A way is a row per agent that fired it (#814): each agent has its
    // own context, so main and a subagent firing one way are two rows.
    let mut active_ways: HashMap<(String, String), ActiveWay> = HashMap::new();
    let mut check_fires: HashMap<(String, String), u64> = HashMap::new();
    // The agent that last fired each way: a check event that does not name
    // its agent counts against it.
    let mut last_agent: HashMap<String, String> = HashMap::new();
    // Each agent's epoch (#815): the clusters holding its events advance
    // it, as the hooks bump an agent's own counter. `seq` counts every
    // cluster and orders the rows by when they fired.
    let mut epochs: HashMap<String, u64> = HashMap::new();
    let mut seq: u64 = 0;
    let mut window: u64 = 1;

    // An event whose stamp is not UTC is skipped: read as 0 it would open an
    // epoch of its own and split the one it sits in.
    let timed: Vec<(u64, &WayEvent)> = events.iter().filter_map(|e| Some((parse_utc_iso(&e.ts)?, e))).collect();
    let start_secs = timed.first().map_or(0, |(s, _)| *s);

    // Cluster events by timestamp proximity (≤3s gap = same epoch)
    let mut clusters: Vec<Vec<&WayEvent>> = Vec::new();
    let mut current_cluster: Vec<&WayEvent> = Vec::new();
    let mut last_ts_secs: u64 = 0;

    for &(ts_secs, ev) in &timed {
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
        // since the last compaction — the same grain `ways session ways` shows.
        let boundary = !frames.is_empty()
            && cluster.iter().any(|ev| ev.event == "session_start");
        if boundary {
            active_ways.clear();
            check_fires.clear();
            last_agent.clear();
            window += 1;
            epochs.clear();
            seq = 0;
        }

        seq += 1;
        for agent in cluster_agents(cluster, &last_agent) {
            *epochs.entry(agent).or_insert(0) += 1;
        }
        let cluster_ts = cluster[0].ts.clone();
        let cluster_secs = parse_utc_iso(&cluster_ts).unwrap_or(start_secs);
        let elapsed = cluster_secs.saturating_sub(start_secs);

        let clocks: HashMap<String, Clock> = epochs
            .iter()
            .map(|(agent, &epoch)| {
                let tokens_k = timelines.get(agent).map_or(0, |t| find_token_position(t, &cluster_ts));
                (agent.clone(), Clock { epoch, tokens_k })
            })
            .collect();
        // A row that names no agent counts in main's context.
        let main = clocks.get(MAIN).copied().unwrap_or_default();
        let clock = |agent: &str| clocks.get(agent).copied().unwrap_or(main);

        let mut new_events: Vec<String> = Vec::new();
        if boundary {
            new_events.push(format!("⎯ compaction · window {window} ⎯"));
        }

        // Mark all existing ways as not-new
        for w in active_ways.values_mut() {
            w.is_new = false;
            w.is_redisclosed = false;
        }

        // Ways fired or re-disclosed in this frame, and the judge's verdicts
        // against its threshold, read once the frame's fires are in.
        let mut fired_now: Vec<(&str, String)> = Vec::new();
        let mut judged: Vec<&WayEvent> = Vec::new();
        let mut held: Vec<&WayEvent> = Vec::new();
        let mut suppressed: Vec<Suppression> = Vec::new();

        for ev in cluster {
            match ev.event.as_str() {
                "way_fired" => {
                    if !ev.way.is_empty() {
                        let key = (ev.way.clone(), fired_by(ev));
                        fired_now.push((&ev.way, key.1.clone()));
                        last_agent.insert(ev.way.clone(), key.1.clone());
                        // The note names a way new to the session, whichever
                        // agent fired it; the row is new to its agent.
                        if !active_ways.keys().any(|(w, _)| *w == ev.way) {
                            new_events.push(format!(
                                "{} ({})",
                                ev.way,
                                render::format_trigger(&ev.trigger)
                            ));
                        }
                        let is_new = !active_ways.contains_key(&key);
                        let at = clock(&key.1);
                        active_ways.insert(key.clone(), ActiveWay {
                            id: ev.way.clone(),
                            trigger: ev.trigger.clone(),
                            epoch_fired: at.epoch,
                            seq,
                            token_pos: at.tokens_k * 1000,
                            check_fires: check_fires.get(&key).copied().unwrap_or(0),
                            is_new,
                            is_redisclosed: false,
                            refire_threshold_k: refire_for(&ev.way),
                            outcome: Outcome::Injected,
                            p_yes: String::new(),
                            ancestor: String::new(),
                            agent: key.1,
                            by_redisclosure: false,
                        });
                    }
                }
                "check_fired" => {
                    if !ev.check.is_empty() {
                        let agent = if ev.agent_id.is_empty() {
                            last_agent.get(&ev.check).cloned().unwrap_or_else(|| MAIN.to_string())
                        } else {
                            ev.agent_id.clone()
                        };
                        let key = (ev.check.clone(), agent);
                        let count = check_fires.entry(key.clone()).or_insert(0);
                        *count += 1;
                        if let Some(w) = active_ways.get_mut(&key) {
                            w.check_fires = *count;
                        }
                        new_events.push(format!("✓ check {}", ev.check));
                    }
                }
                "way_redisclosed" if !ev.way.is_empty() => {
                    new_events.push(format!("↻ {}", ev.way));
                    let key = (ev.way.clone(), fired_by(ev));
                    fired_now.push((&ev.way, key.1.clone()));
                    last_agent.insert(ev.way.clone(), key.1.clone());
                    let checks = check_fires.get(&key).copied().unwrap_or(0);
                    let at = clock(&key.1);
                    // A redisclosure means the way is active (re-injected). Update it
                    // if present; otherwise ADD it — after a compaction-window reset a
                    // still-active way first reappears via redisclosure, not a fresh
                    // fire, and must repopulate the window or it looks empty.
                    active_ways
                        .entry(key.clone())
                        .and_modify(|w| {
                            w.epoch_fired = at.epoch;
                            w.seq = seq;
                            w.token_pos = at.tokens_k * 1000;
                            w.is_redisclosed = true;
                            w.by_redisclosure = true;
                            w.is_new = false;
                            // A new injection: this frame's verdict, if any,
                            // marks it below, not the last one's.
                            w.outcome = Outcome::Injected;
                            w.p_yes.clear();
                        })
                        .or_insert_with(|| ActiveWay {
                            id: ev.way.clone(),
                            trigger: ev.trigger.clone(),
                            epoch_fired: at.epoch,
                            seq,
                            token_pos: at.tokens_k * 1000,
                            check_fires: checks,
                            is_new: false,
                            is_redisclosed: true,
                            refire_threshold_k: refire_for(&ev.way),
                            outcome: Outcome::Injected,
                            p_yes: String::new(),
                            ancestor: String::new(),
                            agent: key.1,
                            by_redisclosure: true,
                        });
                }
                "way_judged" if !ev.way.is_empty() && matches!(ev.verdict.as_str(), "block" | "would_block") => judged.push(ev),
                // #814: a way matched and held back, by its refire window or
                // the context cap. A check's own suppression is not a row.
                "way_suppressed" if !ev.way.is_empty() && ev.kind == "way" => held.push(ev),
                // #786: the switch held a subagent's ways back. It is a mark
                // on the frame, not an event note, so `new_events` reads as
                // it did before the timeline knew of it.
                "injection_suppressed" => suppressed.push(Suppression {
                    switch: ev.switch.clone(),
                    lane: ev.lane.clone(),
                    agent: Some(ev.agent.clone()).filter(|a| !a.is_empty()),
                }),
                _ => {}
            }
        }

        // ADR-196, #742: the judge's verdict is a mark on the way's row. A
        // shadow would-block marks the fire or re-disclosure it judged; a
        // block is a row of its own in this frame, which injected nothing,
        // beside the way's active row when it was already active. A shadow
        // verdict with no row to mark stays an event note, so each fact is
        // said once.
        let mut blocked: Vec<ActiveWay> = Vec::new();
        for ev in judged {
            if ev.verdict == "would_block" {
                // The verdict marks this frame's fires of the way: its
                // agent's, where the verdict names one.
                let mut marked = false;
                for (way, agent) in &fired_now {
                    if *way != ev.way || !(ev.agent_id.is_empty() || ev.agent_id == *agent) {
                        continue;
                    }
                    if let Some(w) = active_ways.get_mut(&(ev.way.clone(), agent.clone())) {
                        w.outcome = Outcome::WouldBlock;
                        w.p_yes = ev.p_yes.clone();
                        marked = true;
                    }
                }
                if !marked {
                    new_events.push(format!("◌ {} (gate {}, shadow)", ev.way, ev.p_yes));
                }
            } else if !blocked.iter().any(|b| b.id == ev.way && b.agent == ev.agent_id) {
                let at = clock(&ev.agent_id);
                blocked.push(ActiveWay {
                    id: ev.way.clone(),
                    // The channel the why index files a judge-blocked way under.
                    trigger: "judge".into(),
                    epoch_fired: at.epoch,
                    seq,
                    token_pos: at.tokens_k * 1000,
                    check_fires: 0,
                    is_new: false,
                    is_redisclosed: false,
                    refire_threshold_k: 0,
                    outcome: Outcome::Blocked,
                    p_yes: ev.p_yes.clone(),
                    ancestor: ev.ancestor.clone(),
                    agent: ev.agent_id.clone(),
                    by_redisclosure: false,
                });
            }
        }
        // A way held back is a row of its own in its frame, as a block is:
        // it injected nothing then, whatever row it has from an earlier fire.
        for ev in held {
            let outcome = if ev.reason == "context_cap" { Outcome::CapHeld } else { Outcome::RefireHeld };
            let agent = fired_by(ev);
            if blocked.iter().any(|b| b.id == ev.way && b.agent == agent && b.outcome == outcome) {
                continue;
            }
            let at = clock(&agent);
            blocked.push(ActiveWay {
                id: ev.way.clone(),
                trigger: ev.trigger.clone(),
                epoch_fired: at.epoch,
                seq,
                token_pos: at.tokens_k * 1000,
                check_fires: 0,
                is_new: false,
                is_redisclosed: false,
                refire_threshold_k: 0,
                outcome,
                p_yes: String::new(),
                ancestor: String::new(),
                agent,
                by_redisclosure: false,
            });
        }
        blocked.sort_by(|a, b| row_order(a).cmp(&row_order(b)));

        let mut ways: Vec<ActiveWay> = active_ways.values().cloned().collect();
        ways.sort_by(|a, b| (a.seq, row_order(a)).cmp(&(b.seq, row_order(b))));
        ways.extend(blocked);

        frames.push(Frame {
            epoch: main.epoch,
            timestamp: cluster_ts,
            elapsed_secs: elapsed,
            token_position_k: main.tokens_k,
            ways,
            new_events,
            suppressed,
            window,
            clocks: clocks.into_iter().filter(|(a, _)| a != MAIN).collect(),
        });
    }

    frames
}

/// The top-level agent's id in the event log.
pub(crate) const MAIN: &str = session::MAIN_AGENT;

/// The agent a fire or re-disclosure went to. A row written before the
/// log recorded agents was the top-level agent's.
fn fired_by(ev: &WayEvent) -> String {
    if ev.agent_id.is_empty() { MAIN.to_string() } else { ev.agent_id.clone() }
}

/// The agents with an event in `cluster`. A check that names no agent is
/// the last agent to fire its way before the check, the rule the event
/// loop in [`build_frames_by_agent`] counts it by.
fn cluster_agents(cluster: &[&WayEvent], last_agent: &HashMap<String, String>) -> Vec<String> {
    let mut last = last_agent.clone();
    let mut agents: Vec<String> = Vec::new();
    for ev in cluster {
        let agent = match ev.event.as_str() {
            "check_fired" if ev.agent_id.is_empty() => last.get(&ev.check).cloned().unwrap_or_else(|| MAIN.to_string()),
            "way_fired" | "way_redisclosed" if !ev.way.is_empty() => {
                let agent = fired_by(ev);
                last.insert(ev.way.clone(), agent.clone());
                agent
            }
            _ => fired_by(ev),
        };
        if !agents.contains(&agent) {
            agents.push(agent);
        }
    }
    agents
}

/// Rows of one epoch in id order, and a way's rows with main's first.
fn row_order(w: &ActiveWay) -> (&str, bool, &str) {
    (&w.id, w.agent != MAIN, &w.agent)
}

/// Whether the relevance judge gave a verdict in this session.
pub(crate) fn has_verdicts(events: &[WayEvent]) -> bool {
    events.iter().any(|e| e.event == "way_judged")
}

/// Each transcript's token timeline, keyed by its path and kept while its
/// length and modification time hold. A live replay rebuilds its frames on
/// every write to the event log; an agent whose transcript has not changed
/// costs a stat, not a read.
type TimelineCache = HashMap<PathBuf, (u64, Option<SystemTime>, Vec<(String, u64)>)>;
static TIMELINES: OnceLock<Mutex<TimelineCache>> = OnceLock::new();

/// [`build_token_timeline`] of `transcript`, read again only when its length
/// or modification time changed since the last read.
fn cached_token_timeline(transcript: &Path) -> Vec<(String, u64)> {
    let Ok(meta) = std::fs::metadata(transcript) else {
        return Vec::new();
    };
    let key = (meta.len(), meta.modified().ok());
    let cache = TIMELINES.get_or_init(Mutex::default);
    if let Some((len, mtime, timeline)) = cache.lock().ok().as_ref().and_then(|c| c.get(transcript).cloned()) {
        if (len, mtime) == key {
            return timeline;
        }
    }
    let timeline = build_token_timeline(transcript);
    if let Ok(mut c) = cache.lock() {
        c.insert(transcript.to_path_buf(), (key.0, key.1, timeline.clone()));
    }
    timeline
}

/// The token position at each assistant turn of `transcript`, main's or a
/// subagent's.
fn build_token_timeline(transcript: &Path) -> Vec<(String, u64)> {
    let Ok(content) = std::fs::read_to_string(transcript) else {
        return Vec::new();
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
                let total_k = claude_sessions::usage::usage_total(usage) / 1000;
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
            // A discarded fire put nothing in the session's context, so the
            // replay reads it under its own name and no fire arm takes it.
            let event = match ways_core::firing::discarded_filter(&v) {
                Some(_) => "way_discarded",
                None => v["event"].as_str().unwrap_or(""),
            };
            Some(WayEvent {
                ts: v["ts"].as_str().unwrap_or("").to_string(),
                event: event.to_string(),
                way: v["way"].as_str().unwrap_or("").to_string(),
                trigger: v["trigger"].as_str().unwrap_or("").to_string(),
                check: v["check"].as_str().unwrap_or("").to_string(),
                p_yes: v["p_yes"].as_str().unwrap_or("").to_string(),
                verdict: v["verdict"].as_str().unwrap_or("").to_string(),
                ancestor: v["ancestor"].as_str().unwrap_or("").to_string(),
                switch: v["switch"].as_str().unwrap_or("").to_string(),
                lane: v["lane"].as_str().unwrap_or("").to_string(),
                agent: v["agent"].as_str().unwrap_or("").to_string(),
                agent_id: v["agent_id"].as_str().unwrap_or("").to_string(),
                kind: v["kind"].as_str().unwrap_or("").to_string(),
                reason: v["reason"].as_str().unwrap_or("").to_string(),
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

#[cfg(test)]
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
            p_yes: String::new(),
            verdict: String::new(),
            ancestor: String::new(),
            switch: String::new(),
            lane: String::new(),
            agent: String::new(),
            agent_id: String::new(),
            kind: String::new(),
            reason: String::new(),
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

    /// An event whose timestamp is not UTC is skipped, not read as 1970.
    #[test]
    fn a_non_utc_event_is_skipped_not_epoch_zero() {
        let ev = |ts: &str, way: &str| WayEvent {
            ts: ts.into(),
            event: "way_fired".into(),
            way: way.into(),
            trigger: "keyword".into(),
            check: String::new(),
            p_yes: String::new(),
            verdict: String::new(),
            ancestor: String::new(),
            switch: String::new(),
            lane: String::new(),
            agent: String::new(),
            agent_id: String::new(),
            kind: String::new(),
            reason: String::new(),
        };
        let events = vec![
            ev("2026-01-01T00:00:00Z", "d/a"),
            ev("2026-01-01T00:00:01+00:00", "d/x"),
            ev("2026-01-01T00:00:02Z", "d/b"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        assert_eq!(frames.len(), 1, "a and b share one epoch");
        assert!(frames.iter().all(|f| f.ways.iter().all(|w| w.id != "d/x")), "the non-UTC row is skipped");
    }

    /// Ways that fired in one epoch keep one order, by id, in every frame:
    /// the active set is a hash map, so an order by epoch alone moved rows
    /// about from run to run and frame to frame.
    #[test]
    fn ways_of_one_epoch_are_in_id_order() {
        let ids: Vec<String> = (0..24).map(|i| format!("d/w{i:02}")).collect();
        let events: Vec<WayEvent> = ids
            .iter()
            .rev()
            .map(|id| WayEvent {
                ts: "2026-01-01T00:00:00Z".into(),
                event: "way_fired".into(),
                way: id.clone(),
                trigger: "keyword".into(),
                check: String::new(),
                p_yes: String::new(),
                verdict: String::new(),
                ancestor: String::new(),
                ..Default::default()
            })
            .collect();
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let got: Vec<&str> = frames[0].ways.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(got, ids.iter().map(String::as_str).collect::<Vec<_>>());
    }

    fn at(ts: &str, event: &str, way: &str, verdict: &str) -> WayEvent {
        WayEvent {
            ts: ts.into(),
            event: event.into(),
            way: way.into(),
            trigger: if event == "way_judged" { String::new() } else { "keyword".into() },
            check: String::new(),
            p_yes: if event == "way_judged" { "0.050".into() } else { String::new() },
            verdict: verdict.into(),
            ancestor: String::new(),
            ..Default::default()
        }
    }

    /// The judge's verdicts as the gate logs them: each candidate judged,
    /// then the ones it let through fired.
    fn judged_session() -> Vec<WayEvent> {
        let t = "2026-01-01T00:00:02Z";
        vec![
            at(t, "way_judged", "d/a", "block"),
            at(t, "way_judged", "d/b", "would_block"),
            at(t, "way_judged", "d/c", "pass"),
            at(t, "way_fired", "d/b", ""),
            at(t, "way_fired", "d/c", ""),
        ]
    }

    fn outcomes(f: &Frame) -> Vec<(&str, Outcome)> {
        f.ways.iter().map(|w| (w.id.as_str(), w.outcome)).collect()
    }

    /// Each row carries its outcome: the pass injected, the shadow verdict
    /// marked on its fire, the block a row of its own that injected nothing.
    #[test]
    fn the_judges_verdicts_mark_the_rows_of_their_frame() {
        let frames = build_frames(&judged_session(), &[], &HashMap::new(), 50);
        assert_eq!(frames.len(), 1);
        let f = &frames[0];
        assert_eq!(outcomes(f), [("d/b", Outcome::WouldBlock), ("d/c", Outcome::Injected), ("d/a", Outcome::Blocked)]);
        let a = &f.ways[2];
        assert_eq!((a.trigger.as_str(), a.epoch_fired, a.p_yes.as_str()), ("judge", f.epoch, "0.050"));
        assert_eq!(a.refire_threshold_k, 0, "a blocked way has nothing to re-disclose");
        // The marks say it; the notes keep only the fires.
        assert_eq!(f.new_events, ["d/b (keyword)", "d/c (keyword)"]);
    }

    /// Injected by default; the matched view adds the judge-blocked rows.
    #[test]
    fn the_default_view_is_injected_and_matched_adds_the_blocked() {
        let f = &build_frames(&judged_session(), &[], &HashMap::new(), 50)[0];
        assert_eq!(outcomes(&f.shown(false)), [("d/b", Outcome::WouldBlock), ("d/c", Outcome::Injected)]);
        assert_eq!(outcomes(&f.shown(true)).len(), 3);
        assert_eq!(f.blocked(), 1);
    }

    /// A blocked row lives in the frame it was judged in only: it never
    /// joined the active set.
    #[test]
    fn a_blocked_row_does_not_carry_into_the_next_frame() {
        let mut events = judged_session();
        events.push(at("2026-01-01T00:01:00Z", "way_fired", "d/e", ""));
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        assert_eq!(frames.len(), 2);
        assert!(frames[1].ways.iter().all(|w| w.id != "d/a"));
        assert_eq!(frames[1].ways.iter().find(|w| w.id == "d/b").map(|w| w.outcome), Some(Outcome::WouldBlock), "the shadow mark stays with its fire");
    }

    /// A shadow verdict with no row to mark stays an event note; a block on
    /// a way already active is a row beside the active one, at this epoch.
    #[test]
    fn a_block_on_an_active_way_is_a_row_and_a_shadow_verdict_on_no_row_a_note() {
        let t = "2026-01-01T00:00:02Z";
        let events = vec![
            at("2026-01-01T00:00:00Z", "way_fired", "d/a", ""),
            at(t, "way_judged", "d/a", "block"),
            at(t, "way_judged", "d/b", "would_block"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let f = &frames[0];
        assert_eq!(f.new_events, ["d/a (keyword)", "◌ d/b (gate 0.050, shadow)"]);
        assert_eq!(outcomes(f), [("d/a", Outcome::Injected), ("d/a", Outcome::Blocked)]);
        assert_eq!(f.blocked(), 1);
        assert_eq!(f.ways[1].epoch_fired, f.epoch);
        assert_eq!(outcomes(&f.shown(false)), [("d/a", Outcome::Injected)]);
    }

    /// A re-disclosure is a new injection: the shadow mark of an earlier
    /// fire does not carry onto it when the judge passed it.
    #[test]
    fn a_redisclosure_drops_the_last_verdicts_mark() {
        let events = vec![
            at("2026-01-01T00:00:00Z", "way_judged", "d/a", "would_block"),
            at("2026-01-01T00:00:00Z", "way_fired", "d/a", ""),
            at("2026-01-01T00:10:00Z", "way_judged", "d/a", "pass"),
            at("2026-01-01T00:10:00Z", "way_redisclosed", "d/a", ""),
            at("2026-01-01T00:20:00Z", "way_judged", "d/a", "would_block"),
            at("2026-01-01T00:20:00Z", "way_redisclosed", "d/a", ""),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let mark = |i: usize| (frames[i].ways[0].outcome, frames[i].ways[0].p_yes.clone());
        assert_eq!(mark(0), (Outcome::WouldBlock, "0.050".into()));
        assert_eq!(mark(1), (Outcome::Injected, String::new()));
        assert_eq!(mark(2), (Outcome::WouldBlock, "0.050".into()), "this frame's verdict marks it again");
    }

    #[test]
    fn redisclosure_repopulates_a_reset_window() {
        let ev = |ts: &str, event: &str, way: &str| WayEvent {
            ts: ts.into(),
            event: event.into(),
            way: way.into(),
            trigger: "keyword".into(),
            check: String::new(),
            p_yes: String::new(),
            verdict: String::new(),
            ancestor: String::new(),
            switch: String::new(),
            lane: String::new(),
            agent: String::new(),
            agent_id: String::new(),
            kind: String::new(),
            reason: String::new(),
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

    /// Each agent keeps its own epoch and token position (#815): a
    /// subagent's clusters leave main's epoch alone, and a subagent's row
    /// re-discloses against its own transcript.
    #[test]
    fn each_agent_counts_epochs_and_tokens_in_its_own_context() {
        let fire = |ts: &str, way: &str, agent: &str| WayEvent {
            ts: ts.into(),
            event: "way_fired".into(),
            way: way.into(),
            trigger: "keyword".into(),
            agent_id: agent.into(),
            ..Default::default()
        };
        let events = vec![
            fire("2026-01-01T00:00:00Z", "d/main-a", ""),
            fire("2026-01-01T00:00:10Z", "d/sub-a", "a1"),
            fire("2026-01-01T00:00:20Z", "d/sub-b", "a1"),
            fire("2026-01-01T00:00:30Z", "d/main-b", "main"),
        ];
        let line = |pts: &[(&str, u64)]| pts.iter().map(|(t, k)| (t.to_string(), *k)).collect::<Vec<_>>();
        let timelines = HashMap::from([
            (MAIN.to_string(), line(&[("2026-01-01T00:00:00Z", 10), ("2026-01-01T00:00:30Z", 20)])),
            ("a1".to_string(), line(&[("2026-01-01T00:00:10Z", 5), ("2026-01-01T00:00:20Z", 50)])),
        ]);
        let refire = HashMap::from([("d/sub-a".to_string(), 40)]);
        let frames = build_frames_by_agent(&events, &timelines, &refire, 50);
        let last = frames.last().unwrap();
        let row = |id: &str| last.ways.iter().find(|w| w.id == id).unwrap();

        // Main saw two clusters; the subagent's two leave its epoch at 2.
        assert_eq!(last.epoch, 2, "main's epoch");
        assert_eq!(last.epoch_of(MAIN), 2);
        assert_eq!(row("d/main-a").epoch_fired, 1);
        assert_eq!(row("d/main-b").epoch_fired, 2);
        assert_eq!(last.epoch_of(MAIN) - row("d/main-a").epoch_fired, 1, "main's distance");

        // The subagent's own epoch: its first fire at 1, now at 2.
        assert_eq!(row("d/sub-a").epoch_fired, 1);
        assert_eq!(last.epoch_of("a1"), 2, "the subagent's epoch");

        // Its token position is its transcript's, 5K at the fire, 50K now:
        // 45K past a 40K threshold re-discloses now.
        assert_eq!(row("d/sub-a").token_pos, 5_000);
        assert_eq!(last.tokens_k_of("a1"), 50);
        assert_eq!(last.tokens_k_of(MAIN), 20);
        let (cell, _) = render::next_cell(row("d/sub-a"), last.epoch_of("a1"), last.tokens_k_of("a1"), &agent_theme::Events::DEFAULT);
        assert_eq!(cell, "● now");
    }

    fn by(ts: &str, event: &str, way: &str, agent: &str) -> WayEvent {
        let check = event == "check_fired";
        WayEvent {
            ts: ts.into(),
            event: event.into(),
            way: if check { String::new() } else { way.into() },
            check: if check { way.into() } else { String::new() },
            trigger: "keyword".into(),
            agent_id: agent.into(),
            ..Default::default()
        }
    }

    /// The forecast strip and the bar's clusters place each row by how far
    /// it is from re-disclosure in its own context, from main's position.
    #[test]
    fn the_forecast_places_a_subagent_row_by_its_own_distance() {
        let events = vec![
            by("2026-01-01T00:00:00Z", "way_fired", "d/main", ""),
            by("2026-01-01T00:00:10Z", "way_fired", "d/sub", "a1"),
        ];
        let line = |pts: &[(&str, u64)]| pts.iter().map(|(t, k)| (t.to_string(), *k)).collect::<Vec<_>>();
        let timelines = HashMap::from([
            (MAIN.to_string(), line(&[("2026-01-01T00:00:00Z", 64)])),
            ("a1".to_string(), line(&[("2026-01-01T00:00:10Z", 10)])),
        ]);
        let refire = HashMap::from([("d/main".to_string(), 40), ("d/sub".to_string(), 40)]);
        let frames = build_frames_by_agent(&events, &timelines, &refire, 50);
        let last = frames.last().unwrap();
        // Both rows are 40K from re-disclosure in their own context: main's
        // at 64K + 40K, the subagent's at 10K + 40K. From main's 64K both
        // plot at 104K, one cluster.
        let (pos, unique) = super::super::table::clusters(last, 200, 50);
        assert_eq!(pos[0], pos[1], "one bar position for both rows");
        assert_eq!(unique.len(), 1);
        let text: Vec<String> = super::super::table::context(last, 200, 80)
            .into_iter()
            .map(|(_, l)| l.spans.iter().map(|s| s.content.to_string()).collect())
            .collect();
        let strip = text.iter().find(|l| l.contains("forecast")).expect("a forecast strip");
        assert!(strip.ends_with(" forecast 64K → 104K"), "{strip}");
        let first = strip.chars().next().unwrap();
        assert_eq!(first, '·', "no row plots as due now: {strip}");
    }

    /// The cursor falls back by the frame a row fired at, across agents:
    /// a subagent row low in its own count but late in the frame order is
    /// not taken for an earlier one.
    #[test]
    fn the_cursor_falls_back_by_frame_order_across_agents() {
        let events = vec![
            by("2026-01-01T00:00:00Z", "way_fired", "d/m1", ""),
            by("2026-01-01T00:00:10Z", "way_fired", "d/m2", ""),
            by("2026-01-01T00:00:20Z", "way_fired", "d/m3", ""),
            by("2026-01-01T00:00:30Z", "way_fired", "d/s1", "a1"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let last = frames.last().unwrap();
        let ids: Vec<(&str, u64, u64)> = last.ways.iter().map(|w| (w.id.as_str(), w.epoch_fired, w.seq)).collect();
        assert_eq!(ids, [("d/m1", 1, 1), ("d/m2", 2, 2), ("d/m3", 3, 3), ("d/s1", 1, 4)]);
        // A main way anchored at frame 3 that is gone: the last row fired at
        // or before frame 3 is d/m3, not the subagent's epoch-1 row.
        assert_eq!(super::super::screen::reselect_by_anchor(last, "gone", MAIN, 3), 2);
        assert_eq!(super::super::screen::reselect_by_anchor(last, "d/s1", "a1", 4), 3, "the same row by frame");
    }

    /// A check that names no agent is the last agent's to fire its way
    /// before the check, in the epoch count as in the check count.
    #[test]
    fn an_agentless_check_counts_for_the_agent_that_fired_its_way_before_it() {
        let events = vec![
            by("2026-01-01T00:00:00Z", "way_fired", "d/x", ""),
            by("2026-01-01T00:00:10Z", "check_fired", "d/x", ""),
            by("2026-01-01T00:00:11Z", "way_fired", "d/x", "a1"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let last = frames.last().unwrap();
        let main_row = last.ways.iter().find(|w| w.agent == MAIN).unwrap();
        assert_eq!(main_row.check_fires, 1, "the check counted against main");
        assert_eq!(last.epoch, 2, "so the cluster advanced main's epoch");
        assert_eq!(last.epoch_of("a1"), 1);
    }

    /// Main's compaction clears the session directory, subagents' state
    /// under `agents/` included, so every agent's epoch restarts there.
    #[test]
    fn main_compaction_restarts_every_agents_epoch() {
        let events = vec![
            by("2026-01-01T00:00:00Z", "session_start", "", ""),
            by("2026-01-01T00:00:10Z", "way_fired", "d/s1", "a1"),
            by("2026-01-01T00:00:20Z", "way_fired", "d/s2", "a1"),
            by("2026-01-01T01:00:00Z", "session_start", "", ""),
            by("2026-01-01T01:00:10Z", "way_fired", "d/s3", "a1"),
        ];
        let frames = build_frames(&events, &[], &HashMap::new(), 50);
        let last = frames.last().unwrap();
        assert_eq!(last.window, 2);
        assert_eq!(last.epoch_of("a1"), 1, "the subagent's count restarted");
        assert_eq!(last.ways.iter().map(|w| (w.id.as_str(), w.epoch_fired)).collect::<Vec<_>>(), [("d/s3", 1)]);
    }

    /// A transcript is read again once it grows, and its cached timeline
    /// is served while it does not.
    #[test]
    fn a_token_timeline_is_read_again_only_when_its_transcript_changes() {
        let dir = std::env::temp_dir().join(format!("ways-timeline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let turn = |ts: &str, k: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"usage":{{"input_tokens":{},"cache_read_input_tokens":0}}}}}}"#,
                k * 1000
            ) + "\n"
        };
        std::fs::write(&path, turn("2026-01-01T00:00:00Z", 5)).unwrap();
        assert_eq!(cached_token_timeline(&path), [("2026-01-01T00:00:00Z".to_string(), 5)]);
        let mut grown = turn("2026-01-01T00:00:00Z", 5);
        grown.push_str(&turn("2026-01-01T00:00:10Z", 9));
        std::fs::write(&path, grown).unwrap();
        assert_eq!(cached_token_timeline(&path).len(), 2, "the grown file is read again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A transcript whose length and modification time hold is served from
    /// the cache: contents swapped for others of the same length, with the
    /// time put back, still give the timeline first read.
    #[test]
    fn an_unchanged_transcript_is_served_from_the_cache() {
        let dir = std::env::temp_dir().join(format!("ways-timeline-hit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let turn = |k: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{{"usage":{{"input_tokens":{},"cache_read_input_tokens":0}}}}}}"#,
                k * 1000
            ) + "\n"
        };
        std::fs::write(&path, turn(5)).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(cached_token_timeline(&path), [("2026-01-01T00:00:00Z".to_string(), 5)]);
        std::fs::write(&path, turn(7)).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
        assert_eq!(cached_token_timeline(&path), [("2026-01-01T00:00:00Z".to_string(), 5)], "served from the cache, not read again");
        assert_eq!(build_token_timeline(&path), [("2026-01-01T00:00:00Z".to_string(), 7)], "the file did change");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
