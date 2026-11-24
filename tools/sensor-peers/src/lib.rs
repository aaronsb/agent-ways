// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-FileCopyrightText: 2026 Jonathan Muse
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

pub mod last_inbound;

use sensor_trait::{Focus, Sensor};
use std::collections::HashMap;
use std::fs;
use std::time::Duration;

/// Discovers peer Claude Code sessions by reading ~/.claude/sessions/*.json
/// and their transcript files. Same discovery pattern as abtop.
///
/// Reports a peer that starts or exits, and a same-project peer whose
/// context nears compaction. A peer in the same working directory weighs
/// more than one elsewhere. It does not report a peer turning between
/// working and waiting: a peer does that every turn, and each report it
/// wrote would cost the receiving session a turn of its own.
///
/// It reads no messages: the attend mod's drain carries them.
///
/// References:
/// - ADR-416 item 5: the host keeps the presence half of peers, without
///   the working and waiting flips.
pub struct PeerSensor {
    /// Our own PID, so we can exclude self
    own_pid: u32,
    /// Claude Code's config dir: session records and transcripts.
    claude: claude_sessions::ClaudeDir,
    /// Previous snapshot: session_id → summary
    prior: HashMap<String, PeerSummary>,
    /// First poll establishes baseline
    baseline_established: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct PeerSummary {
    pid: u32,
    cwd: String,
    project_name: String,
    context_percent: f64,
    model: String,
    status: PeerStatus,
}

#[derive(Clone, Debug, PartialEq)]
enum PeerStatus {
    Working,
    Waiting,
    Unknown,
}

impl std::fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerStatus::Working => write!(f, "working"),
            PeerStatus::Waiting => write!(f, "waiting"),
            PeerStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Minimal session file structure — matches Claude Code's format.
/// Only the fields we need; everything else is ignored.
#[derive(Debug)]
struct SessionFile {
    pid: u32,
    cwd: String,
    session_id: String,
}

impl PeerSensor {
    pub fn new() -> Self {
        Self {
            own_pid: std::process::id(),
            claude: claude_sessions::ClaudeDir::user(),
            prior: HashMap::new(),
            baseline_established: false,
        }
    }

    /// Return a list of active peer sessions as
    /// `(session_id, cwd, project_name, status, context_percent)`.
    ///
    /// `session_id` is included so renderers can look up the
    /// per-cwd instance suffix (ADR-129) without separately
    /// re-walking session.json files.
    pub fn list_peers(&self) -> Vec<(String, String, String, String, f64)> {
        let peers = self.discover_peers();
        let mut result: Vec<_> = peers.iter()
            .map(|(sid, p)| (
                sid.clone(),
                p.cwd.clone(),
                p.project_name.clone(),
                p.status.to_string(),
                p.context_percent,
            ))
            .collect();
        result.sort_by(|a, b| a.1.cmp(&b.1));
        result
    }

    /// Return the set of live Claude session IDs currently visible to
    /// the peer sensor. Callers cross-reference this against
    /// `_groups.yaml` member lists when they need a liveness-checked
    /// view (channel routing in `attend message send --channel`, etc.) — the
    /// yaml count alone trusts membership records that outlive their session.
    pub fn live_session_ids(&self) -> std::collections::HashSet<String> {
        self.discover_peers().into_keys().collect()
    }

    fn discover_peers(&self) -> HashMap<String, PeerSummary> {
        let mut peers = HashMap::new();

        for record in self.claude.session_records() {
            let Some(sf) = session_file(record) else { continue };

            // Skip our own parent claude process.
            // attend's parent is claude, so check the ancestry.
            if attend_presence::process::has_ancestor(self.own_pid, sf.pid) {
                continue;
            }

            // Check if PID is alive and is a claude process
            if !attend_presence::process::is_claude(sf.pid) {
                continue;
            }

            let project_name = sf.cwd
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("?")
                .to_string();

            // One lookup per peer per poll: a peer with no transcript yet
            // would otherwise cost a scan of every project twice.
            let transcript = self.claude.find_transcript(Some(&sf.cwd), &sf.session_id);
            let (context_percent, model) = transcript
                .as_deref()
                .and_then(transcript_summary)
                .unwrap_or((0.0, "-".to_string()));
            let status = infer_status(transcript.as_deref());

            peers.insert(sf.session_id, PeerSummary {
                pid: sf.pid,
                cwd: sf.cwd,
                project_name,
                context_percent,
                model,
                status,
            });
        }

        peers
    }

    #[cfg(test)]
    fn read_transcript_summary(&self, cwd: &str, session_id: &str) -> Option<(f64, String)> {
        transcript_summary(&self.claude.find_transcript(Some(cwd), session_id)?)
    }

    #[cfg(test)]
    fn infer_status(&self, cwd: &str, session_id: &str) -> PeerStatus {
        infer_status(self.claude.find_transcript(Some(cwd), session_id).as_deref())
    }
}

/// The last complete lines of `path` that hold an assistant turn with
/// usage: a 64 KB tail, doubled until it holds one, reaches the start of
/// the file, or reaches [`MAX_TAIL`] (a transcript with no usage line, such
/// as a session with only user turns so far, is not re-read whole each
/// poll). A long final assistant line (tool output, long answers run
/// past 49 KB) is read whole rather than cut, which `serde_json` cannot
/// parse.
const MAX_TAIL: u64 = 4 * 1024 * 1024;

fn usage_tail(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut size: u64 = 64 * 1024;
    loop {
        let start = len.saturating_sub(size);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok()?;
        // Drop the partial first line unless the read began at the start.
        let body = if start == 0 {
            &buf[..]
        } else {
            match buf.iter().position(|&b| b == b'\n') {
                Some(i) => &buf[i + 1..],
                None => &[][..],
            }
        };
        let text = String::from_utf8_lossy(body).into_owned();
        if start == 0 || size >= MAX_TAIL || claude_sessions::usage::last_context_tokens(&text).is_some() {
            return Some(text);
        }
        size = size.saturating_mul(2);
    }
}

/// A peer's context fill (percent of its window) and model, from the tail
/// of its transcript.
fn transcript_summary(path: &std::path::Path) -> Option<(f64, String)> {
    let tail = usage_tail(path)?;

    // Placeholder turns (`<synthetic>`, `-`, `unknown`) are skipped by
    // `last_model`, so a peer whose newest turn is synthetic keeps its real
    // model (ADR-166).
    let model = claude_sessions::usage::last_model(&tail).unwrap_or_else(|| "-".to_string());
    let context_tokens = claude_sessions::usage::last_context_tokens(&tail).unwrap_or(0);
    // The window comes from the one resolver (ADR-166) — this sensor used to
    // carry its own substring rules, which disagreed with the gauge's about
    // sonnet-4.
    //
    // `resolve_for_foreign_session`, not `resolve`: this window belongs to the
    // *peer's* session, and `CLAUDE_CONTEXT_WINDOW` states the window of the
    // process that set it. Applying the operator's override to a peer would
    // compute that peer's fill against the observer's window — an operator who
    // set it to 1M would see a haiku peer at 190K/200K (about to compact)
    // rendered as 19% full, and miss the one peer that actually needs room.
    //
    // `-` is the no-model placeholder, not a model id: with no model there is
    // no meaningful percentage, so the peer's fill is suppressed rather than
    // defaulted to a number that would look authoritative.
    let context_window: u64 = if model == "-" {
        0
    } else {
        ways_core::context_window::resolve_for_foreign_session(Some(&model)).tokens
    };
    let context_percent = if context_window > 0 {
        (context_tokens as f64 / context_window as f64) * 100.0
    } else {
        0.0
    };

    Some((context_percent, model))
}

/// Working when the transcript changed in the last 30 s, else waiting;
/// unknown without one.
fn infer_status(transcript: Option<&std::path::Path>) -> PeerStatus {
    let mtime = transcript
        .and_then(|path| fs::metadata(path).ok())
        .and_then(|m| m.modified().ok());

    match mtime {
        Some(t) => {
            let age = t.elapsed().unwrap_or_default();
            if age.as_secs() < 30 {
                PeerStatus::Working
            } else {
                PeerStatus::Waiting
            }
        }
        None => PeerStatus::Unknown,
    }
}

impl Default for PeerSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl Sensor for PeerSensor {
    fn name(&self) -> &str {
        "peers"
    }

    sensor_trait::sensor_metadata!();

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let current = self.discover_peers();

        // First poll: establish baseline
        if !self.baseline_established {
            let same_dir: Vec<&PeerSummary> = current.values()
                .filter(|p| attend_presence::session::same_dir(&p.cwd, &focus.working_dir))
                .collect();
            eprintln!(
                "[attend] peers: baseline — {} sessions ({} in this project)",
                current.len(), same_dir.len(),
            );
            self.prior = current;
            self.baseline_established = true;
            return Vec::new();
        }

        let observations = changes(&self.prior, &current, &focus.working_dir);
        self.prior = current;
        observations
    }

    fn emission_threshold(&self) -> f64 {
        2.0 // Same-project peer events are magnitude 2-3, others are lower
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(30) // Check every 30s
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(10) // Don't scan sessions faster than 10s
    }

    fn decay_threshold(&self) -> u32 {
        5
    }
}

// --- Helpers ---

/// What changed between two polls, for a session working in `working_dir`:
/// a peer that started or exited, and a same-project peer whose context
/// crossed 80%. A peer's working or waiting status is not compared.
fn changes(prior: &HashMap<String, PeerSummary>, current: &HashMap<String, PeerSummary>, working_dir: &str) -> Vec<(f64, String)> {
    let same = |p: &PeerSummary| attend_presence::session::same_dir(&p.cwd, working_dir);
    let mut observations = Vec::new();

    for (sid, peer) in current {
        if !prior.contains_key(sid) {
            let (magnitude, relevance) = if same(peer) { (3.0, "same project") } else { (1.0, "other project") };
            observations.push((magnitude, format!(
                "peer session started: {} [{}] ({}, {}, ctx {:.0}%)",
                peer.project_name, peer.cwd, relevance, peer.status, peer.context_percent
            )));
        }
    }

    for (sid, peer) in prior {
        if !current.contains_key(sid) {
            let magnitude = if same(peer) { 2.0 } else { 0.5 };
            observations.push((magnitude, format!(
                "peer session exited: {} [{}]", peer.project_name, peer.cwd
            )));
        }
    }

    // A same-project peer approaching compaction.
    for (sid, peer) in current {
        let Some(was) = prior.get(sid) else { continue };
        if same(peer) && peer.context_percent >= 80.0 && was.context_percent < 80.0 {
            observations.push((2.0, format!(
                "peer {} [{}] context at {:.0}% — approaching compaction",
                peer.project_name, peer.cwd, peer.context_percent
            )));
        }
    }

    observations
}

/// The fields this sensor keeps from a session record. Records without a
/// cwd are skipped. The cwd is the identity root, not the live cwd (#394), so
/// peer-presence labels agree with the normalized identity everywhere else.
fn session_file(record: claude_sessions::SessionRecord) -> Option<SessionFile> {
    let cwd = attend_presence::session::normalize_origin(&record.cwd?);
    Some(SessionFile { pid: record.pid, cwd, session_id: record.session_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_claude(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("sensor-peers-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn transcript_summary_finds_a_project_with_a_space() {
        // Claude Code names `/srv/my proj` `-srv-my-proj`; the sensor's own
        // encoder kept the space and read nothing for such a peer.
        let root = temp_claude("space");
        let dir = root.join("projects/-srv-my-proj");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("sid-1.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":1000,"cache_read_input_tokens":99000,"cache_creation_input_tokens":0}}}"#,
        )
        .unwrap();
        let mut sensor = PeerSensor::new();
        sensor.claude = claude_sessions::ClaudeDir::at(&root);
        let (pct, model) = sensor.read_transcript_summary("/srv/my proj", "sid-1").unwrap();
        assert_eq!(model, "claude-opus-4-8");
        assert!(pct > 0.0);
        assert!(!matches!(sensor.infer_status("/srv/my proj", "sid-1"), PeerStatus::Unknown));
        fs::remove_dir_all(&root).ok();
    }

    fn peer(cwd: &str, status: PeerStatus, context_percent: f64) -> PeerSummary {
        PeerSummary {
            pid: 42,
            cwd: cwd.to_string(),
            project_name: cwd.rsplit('/').next().unwrap().to_string(),
            context_percent,
            model: "claude-opus-4-8".to_string(),
            status,
        }
    }

    fn polls(statuses: &[PeerStatus]) -> Vec<HashMap<String, PeerSummary>> {
        statuses.iter().map(|s| HashMap::from([("p1".to_string(), peer("/srv/proj", s.clone(), 10.0))])).collect()
    }

    #[test]
    fn a_same_project_peer_turning_working_and_waiting_is_not_reported() {
        // A peer flips every turn. Two flips in one window added up to the
        // event lane's cut, and the drain continued the turn to deliver them.
        let seen = polls(&[PeerStatus::Working, PeerStatus::Waiting, PeerStatus::Working, PeerStatus::Unknown]);
        for pair in seen.windows(2) {
            assert_eq!(changes(&pair[0], &pair[1], "/srv/proj"), Vec::<(f64, String)>::new());
        }
    }

    #[test]
    fn a_peer_that_starts_or_exits_is_still_reported() {
        let none = HashMap::new();
        let one = HashMap::from([("p1".to_string(), peer("/srv/proj", PeerStatus::Working, 12.0))]);
        assert_eq!(
            changes(&none, &one, "/srv/proj"),
            vec![(3.0, "peer session started: proj [/srv/proj] (same project, working, ctx 12%)".to_string())],
        );
        assert_eq!(changes(&one, &none, "/srv/proj"), vec![(2.0, "peer session exited: proj [/srv/proj]".to_string())]);
        assert_eq!(
            changes(&none, &one, "/srv/other"),
            vec![(1.0, "peer session started: proj [/srv/proj] (other project, working, ctx 12%)".to_string())],
        );
        assert_eq!(changes(&one, &none, "/srv/other"), vec![(0.5, "peer session exited: proj [/srv/proj]".to_string())]);
    }

    #[test]
    fn a_same_project_peer_nearing_compaction_is_still_reported() {
        let before = HashMap::from([("p1".to_string(), peer("/srv/proj", PeerStatus::Working, 79.0))]);
        let after = HashMap::from([("p1".to_string(), peer("/srv/proj", PeerStatus::Waiting, 81.0))]);
        assert_eq!(
            changes(&before, &after, "/srv/proj"),
            vec![(2.0, "peer proj [/srv/proj] context at 81% — approaching compaction".to_string())],
        );
        assert_eq!(changes(&before, &after, "/srv/other"), Vec::<(f64, String)>::new());
    }

    #[test]
    fn pretty_printed_session_record_is_read() {
        // The sensor's own extractor required `"key":"value"` with no space.
        let content = "{\n  \"pid\": 42,\n  \"sessionId\": \"s\",\n  \"cwd\": \"/p/.claude/worktrees/w\"\n}";
        let record = claude_sessions::parse_session_record(content, std::path::Path::new("42.json")).unwrap();
        let sf = session_file(record).unwrap();
        assert_eq!((sf.pid, sf.cwd.as_str(), sf.session_id.as_str()), (42, "/p", "s"));
    }

    #[test]
    fn transcript_summary_reads_a_final_line_longer_than_the_tail() {
        // A final assistant turn of 100 KB: an 8 KB tail cut it and lost the
        // usage; the tail now grows until it holds the whole line.
        let root = temp_claude("longline");
        let path = root.join("t.jsonl");
        let long = "x".repeat(100 * 1024);
        fs::write(
            &path,
            format!(
                "{{\"type\":\"user\"}}\n{{\"type\":\"assistant\",\"message\":{{\"model\":\"claude-opus-4-8\",\"content\":\"{long}\",\"usage\":{{\"input_tokens\":1000,\"cache_read_input_tokens\":99000,\"cache_creation_input_tokens\":0}}}}}}\n"
            ),
        )
        .unwrap();
        let (pct, model) = transcript_summary(&path).unwrap();
        assert_eq!(model, "claude-opus-4-8");
        assert!(pct > 0.0);
        fs::remove_dir_all(&root).ok();
    }
}
