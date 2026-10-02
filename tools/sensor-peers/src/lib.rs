pub mod last_inbound;

use attend_groups::{ReceiveDir, Room};
use sensor_trait::{Focus, Sensor};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Callback that returns the directories this session receives from, given
/// its origin path: `attend_groups::Groups::receive_dirs`. Called on every
/// scan so mid-session channel join/leave is reflected without restarting
/// the sensor loop.
pub type ReceiveDirsFn = Arc<dyn Fn(&str) -> Vec<ReceiveDir> + Send + Sync>;

/// Discovers peer Claude Code sessions by reading ~/.claude/sessions/*.json
/// and their transcript files. Same discovery pattern as abtop.
///
/// Also reads signal files from attend's signals base for peer messages.
///
/// Reports deltas when peers appear, disappear, or change state.
/// Filters through focus: only surfaces peers in the same working directory
/// (or with overlapping git branches) as noteworthy.
pub struct PeerSensor {
    /// Our own PID, so we can exclude self
    own_pid: u32,
    /// Claude Code's config dir: session records and transcripts.
    claude: claude_sessions::ClaudeDir,
    /// Previous snapshot: session_id → summary
    prior: HashMap<String, PeerSummary>,
    /// Signal files we've already seen (by filename)
    seen_signals: HashSet<String>,
    /// Whether we've shown the reply hint (only show once per session)
    reply_hint_shown: bool,
    /// Our own session ID (to skip our own signals)
    own_session_id: Option<String>,
    /// First poll establishes baseline
    baseline_established: bool,
    /// Provider of the receive set scanned on each poll. A closure lets the
    /// sensor pick up channel joins and leaves that happen after startup
    /// without the orchestrator having to push updates. Without one the
    /// sensor reads the project tray and `#open`. Set via
    /// `set_receive_dirs_provider()`.
    receive_dirs_fn: Option<ReceiveDirsFn>,
    /// Per-peer message timestamps for engagement-based magnitude boosting.
    /// Keyed by "from" field (e.g., "claude:<session_id>").
    /// When the same peer sends multiple messages in a window, their
    /// subsequent messages get a magnitude boost so they can break through
    /// the elevated refractory threshold in the peer sensor.
    peer_activity: HashMap<String, VecDeque<Instant>>,
    /// Sliding window for per-peer engagement boost calculation.
    /// Set via `set_peer_activity_window` from attend's engagement config.
    peer_activity_window: Duration,
    /// Set true once a checkpoint has been imported (warm restart). A
    /// warm restart restores the seen-set, so only messages that arrived
    /// during the down-gap surface. A cold start (this stays false) has
    /// no seen-set, so the first scan baselines the existing backlog
    /// instead of dumping it — see `read_signals`.
    checkpoint_loaded: bool,
    /// Whether import_state has run at least once — gates the restore
    /// banner and `checkpoint_loaded` to the startup import, since the
    /// ADR-172 drain-mark refresh re-imports every peers poll.
    state_imported: bool,
    /// Set true after the first `read_signals`. Gates the one-time
    /// cold-start message baseline.
    message_baseline_done: bool,
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

/// Which room a message came from — used only for the digest breakdown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MsgKind {
    Directed,
    Open,
    Group,
}

/// A message awaiting emission for the current poll. Collected during the
/// scan, then either emitted individually or rolled into a digest once the
/// whole poll's volume is known.
struct PendingMsg {
    magnitude: f64,
    /// Wire `from` and `cwd`, kept raw: the sender label renders only
    /// in the per-message emit branch, since a digest never shows it.
    from: String,
    cwd: String,
    body: String,
    kind: MsgKind,
    /// Wall-clock age in seconds (since the file's mtime) at scan time.
    age_secs: u64,
}

impl PeerSensor {
    pub fn new() -> Self {
        let own_pid = std::process::id();
        let own_session_id = attend_presence::session::find_own_session_id(own_pid);
        Self {
            own_pid,
            claude: claude_sessions::ClaudeDir::user(),
            prior: HashMap::new(),
            seen_signals: HashSet::new(),
            reply_hint_shown: false,
            own_session_id,
            baseline_established: false,
            receive_dirs_fn: None,
            peer_activity: HashMap::new(),
            peer_activity_window: Duration::from_secs(900),
            checkpoint_loaded: false,
            state_imported: false,
            message_baseline_done: false,
        }
    }

    /// Set the per-peer engagement window. Called by the orchestrator
    /// to align with the attend engagement config.
    pub fn set_peer_activity_window(&mut self, window: Duration) {
        self.peer_activity_window = window;
    }


    /// Compute the magnitude boost for a peer based on their recent activity.
    /// Records the current message and returns the boost multiplier.
    ///
    /// The boost creates a gradient: messages from peers who've been actively
    /// exchanging messages climb above the elevated refractory threshold
    /// while background broadcasts stay at baseline and get suppressed.
    ///
    /// Window is 10 minutes — sized to Claude's actual turn cadence, where
    /// 3 messages between agents takes 5-10 minutes of wall clock.
    ///
    /// - 1st message in window: 1.0x (entry level, fires at rest)
    /// - 2nd message: 1.75x (participant emerging)
    /// - 3rd+ message: 2.5x (established conversation partner — reliably
    ///   breaks through refractory)
    fn peer_engagement_boost(&mut self, from: &str) -> f64 {
        let now = Instant::now();
        let window = self.peer_activity_window;
        let history = self.peer_activity.entry(from.to_string()).or_default();
        // Prune old entries
        while let Some(front) = history.front() {
            if now.duration_since(*front) > window {
                history.pop_front();
            } else {
                break;
            }
        }
        history.push_back(now);
        match history.len() {
            0 | 1 => 1.0,
            2 => 1.75,
            _ => 2.5,
        }
    }

    /// Register the provider of the receive set. The closure is invoked on
    /// every scan, so mid-session channel join/leave propagates without
    /// restarting the sensor loop.
    pub fn set_receive_dirs_provider(&mut self, f: ReceiveDirsFn) {
        self.receive_dirs_fn = Some(f);
    }

    /// The directories to scan for `origin` this poll.
    fn receive_dirs(&self, origin: &str) -> Vec<ReceiveDir> {
        match &self.receive_dirs_fn {
            Some(f) => f(origin),
            None => attend_groups::Groups::new(&attend_presence::cache::signals_dir(), "").receive_dirs(origin),
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
    /// view (channel routing in `attend send --channel`, etc.) — the
    /// yaml count alone trusts membership records that outlive their session.
    pub fn live_session_ids(&self) -> std::collections::HashSet<String> {
        self.discover_peers().into_keys().collect()
    }

    /// Read signal files from peers. Scans own project dir, broadcast dir,
    /// joined channels. Returns observations for new signals.
    fn read_signals(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let mut observations = Vec::new();
        // Unseen messages this poll are collected here, then either emitted
        // individually (the common, timely case) or coalesced into one
        // digest when a single poll surfaces more than DIGEST_THRESHOLD —
        // a warm rejoin after a down-gap, or a burst from a hyperactive
        // peer. Nothing is dropped; detail is always in `attend inbox`.
        let mut pending: Vec<PendingMsg> = Vec::new();
        // Directories to scan: own project + `#open` + joined channels.
        let scan_dirs = self.receive_dirs(&focus.working_dir);

        let own_session_id: String = self.own_session_id
            .clone()
            .unwrap_or_else(|| "---none---".to_string());

        // Cold start: the first scan of a session that restored no
        // checkpoint (no seen-set) applies `attend_state::cold_start`, the
        // rule the Stop-hook drain applies too. Addressed mail is delivered
        // whatever its age; old `#open` and channel backlog is marked seen
        // without being shown and counted in one note, so a fresh enrollment
        // is neither flooded nor silently emptied. A warm restart skips this
        // (the checkpoint restored the seen-set), so down-gap messages still
        // surface as unseen. Detail is always available via `attend inbox`.
        let baselining = !self.message_baseline_done && !self.checkpoint_loaded;
        self.message_baseline_done = true;

        // Unseen signals from others, in scan order.
        struct Found {
            room: usize,
            key: String,
            signal_id: String,
            content: String,
            age: Duration,
        }
        let mut found: Vec<Found> = Vec::new();
        for (room, dir) in scan_dirs.iter().enumerate() {
            let Ok(entries) = fs::read_dir(&dir.path) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let filename = match path.file_name().and_then(|f| f.to_str()) {
                    Some(f) if f.ends_with(".signal") => f.to_string(),
                    _ => continue,
                };
                // Skip already-seen. The key is the filename, a unique
                // signal id, so a signal moved between trays stays seen.
                let key = attend_state::seen_key(&filename);
                if self.seen_signals.contains(&key) {
                    continue;
                }
                // Read and parse: `from|project|cwd|message` (legacy) or
                // `from|project|cwd|re:id|message` (threaded, ADR-120).
                let Ok(content) = fs::read_to_string(&path) else { continue };
                let Some(sig) = agent_identity::parse_signal(content.trim()) else {
                    self.seen_signals.insert(key);
                    continue;
                };
                // Skip our own signals — check the from field, not filename.
                // from is "claude:session-id" or "external:user@terminal"
                if sig.from.split_once(':').is_some_and(|(_, identity)| identity == own_session_id) {
                    self.seen_signals.insert(key);
                    continue;
                }
                let age = fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .unwrap_or_default();
                // The signal's id is its filename stem, the value `re:<id>`
                // replies reference (ADR-120).
                let signal_id = filename.strip_suffix(".signal").unwrap_or(&filename).to_string();
                found.push(Found { room, key, signal_id, content, age });
            }
        }

        let plan = baselining.then(|| {
            let pending: Vec<_> = found.iter().map(|f| (&scan_dirs[f.room].room, f.age)).collect();
            attend_state::cold_start::plan(&pending)
        });

        for (i, f) in found.into_iter().enumerate() {
            // Authored messages are durable: reading one marks it seen
            // (dedup), but the file is NEVER deleted here. Destroying a
            // signal that another peer — or this same session after a
            // restart — has not read yet was the cross-peer shred behind
            // ADR-136 Bug 2 (a passing colleague shredding an unread fax).
            // Message lifetime is bound by project liveness in the cleanup
            // sweep (a tray dies when its project is gone), not by a
            // per-read wall-clock timer.
            self.seen_signals.insert(f.key);
            if plan.as_ref().is_some_and(|p| !p.deliver[i]) {
                continue;
            }
            let Some(agent_identity::ParsedSignal { from, cwd: source_cwd, message, .. }) =
                agent_identity::parse_signal(f.content.trim())
            else {
                continue;
            };

            // The message lane bypasses the event-lane noise stack
            // (ADR-136 Decision 1): no salience gate, no refractory, no
            // governor. An authored message is not observation noise, so
            // it is never aged-out or suppressed by wall-clock decay —
            // only deduped (the seen-set) and, on a cold start, put
            // through the cold-start rule above. Decay stays where it
            // belongs: the event lane (git / process / peer-presence).
            //
            // Directed messages (in own project dir) get highest priority.
            // Broadcast and focus group messages are important but less urgent.
            let (base_magnitude, kind): (f64, MsgKind) = match scan_dirs[f.room].room {
                Room::Project => (7.0, MsgKind::Directed), // someone used --to
                Room::Open => (4.0, MsgKind::Open),        // important but not targeted
                Room::Channel(_) => (5.0, MsgKind::Group), // relevant peer
            };

            // Boost by peer engagement: repeated messages from the
            // same peer within a window increase magnitude, so active
            // conversation partners break through elevated refractory
            // thresholds while uninvolved broadcasts stay at baseline
            // (and get suppressed when the peer sensor is refractory).
            // This is the "auto-grouping" mechanism — conversation
            // emerges from observed traffic rather than explicit config.
            let boost = self.peer_engagement_boost(from);
            let magnitude = base_magnitude * boost;

            // Defer emission: collect now, decide individual-vs-digest
            // after the whole poll is scanned (see below).
            pending.push(PendingMsg {
                magnitude,
                from: from.to_string(),
                cwd: source_cwd.to_string(),
                body: message.to_string(),
                kind,
                age_secs: f.age.as_secs(),
            });

            // Track most-recent inbound for `attend reply`. We record
            // here (after deciding to emit an observation) so `attend
            // reply` targets what the operator actually saw, not what was
            // held back. The own-session-id keys the file so concurrent
            // attend processes don't collide.
            //
            // Filter: skip signals originating from the same working
            // directory as this observer. The own-session skip above only
            // catches the *current* claude session id; a previous
            // incarnation of the same agent (different session uuid, same
            // cwd) would otherwise pollute last_inbound, causing `attend
            // reply` to auto-thread to the agent's own past self.
            if let Some(ref sid) = self.own_session_id {
                if source_cwd != focus.working_dir {
                    last_inbound::record(sid, &f.signal_id);
                }
            }
        }

        // The cold start's count of what it held back goes out once, with
        // this first delivery, at `#open`'s magnitude.
        if let Some(note) = plan.and_then(|p| p.note()) {
            observations.push((4.0, note));
        }

        // Coalesce or emit. Below the threshold, every message emits
        // individually and immediately (timely — the common case). Above
        // it, a single poll surfaced a flood (rejoin gap or hyperactive
        // peer), so collapse to one count-led digest instead of N events.
        if pending.len() > DIGEST_THRESHOLD {
            observations.push(build_digest(&pending));
        } else {
            // One display form on both conduits (#534): the same
            // persona-plus-project label the Stop-hook drain renders,
            // so a receiver correlates a Monitor notification with a
            // later drain row by name. One registry snapshot per cwd
            // for this pass; built here, after the digest branch is
            // ruled out, because a digest never shows a sender.
            let instances = attend_instances::view::SnapshotCache::new();
            for m in &pending {
                let sender =
                    attend_instances::view::render_sender_label(&m.from, &m.cwd, &attend_instances::view::Painter::plain(), &instances);
                let include_reply_hint = !self.reply_hint_shown;
                // Chunk long messages at word boundaries so each event stays
                // under Monitor's ~400-char stdout line ceiling; chunks ride
                // the same 200ms batch and group into one notification.
                let chunks = chunk_message(&m.body, MAX_CHUNK_BODY, MAX_CHUNKS);
                let total = chunks.len();
                for (i, chunk) in chunks.into_iter().enumerate() {
                    let header = message_header(&sender, i, total);
                    let body = if i == 0 && include_reply_hint {
                        format!("{} (reply: attend send <msg>)", chunk)
                    } else {
                        chunk
                    };
                    observations.push((m.magnitude, format!("{}{}", header, body)));
                }
                if include_reply_hint {
                    self.reply_hint_shown = true;
                }
            }
        }

        observations
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
                .filter(|p| p.cwd == focus.working_dir)
                .collect();
            eprintln!(
                "[attend] peers: baseline — {} sessions ({} in this project)",
                current.len(), same_dir.len(),
            );
            self.prior = current;
            self.baseline_established = true;
            return Vec::new();
        }

        let mut observations = Vec::new();

        // New peers
        for (sid, peer) in &current {
            if !self.prior.contains_key(sid) {
                let relevance = if peer.cwd == focus.working_dir {
                    "same project"
                } else {
                    "other project"
                };
                let magnitude = if peer.cwd == focus.working_dir { 3.0 } else { 1.0 };
                observations.push((magnitude, format!(
                    "peer session started: {} [{}] ({}, {}, ctx {:.0}%)",
                    peer.project_name, peer.cwd, relevance, peer.status, peer.context_percent
                )));
            }
        }

        // Exited peers
        for (sid, peer) in &self.prior {
            if !current.contains_key(sid) {
                let magnitude = if peer.cwd == focus.working_dir { 2.0 } else { 0.5 };
                observations.push((magnitude, format!(
                    "peer session exited: {} [{}]", peer.project_name, peer.cwd
                )));
            }
        }

        // State changes in existing peers (only for same-project peers)
        for (sid, peer) in &current {
            if let Some(prior) = self.prior.get(sid) {
                // Only track peers in same project
                if peer.cwd != focus.working_dir {
                    continue;
                }

                // Status changed
                if peer.status != prior.status {
                    observations.push((1.5, format!(
                        "peer {} [{}] now {} (was {})",
                        peer.project_name, peer.cwd, peer.status, prior.status
                    )));
                }

                // Context pressure — peer approaching limits
                if peer.context_percent >= 80.0 && prior.context_percent < 80.0 {
                    observations.push((2.0, format!(
                        "peer {} [{}] context at {:.0}% — approaching compaction",
                        peer.project_name, peer.cwd, peer.context_percent
                    )));
                }
            }
        }

        // Check for peer signals (messages from other sessions)
        observations.extend(self.read_signals(focus));

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

    fn export_state(&self) -> Vec<(String, String)> {
        let mut state = Vec::new();
        for sig in &self.seen_signals {
            state.push(("seen_signal".to_string(), sig.clone()));
        }
        state.push(("reply_hint_shown".to_string(), self.reply_hint_shown.to_string()));
        state
    }

    fn import_state(&mut self, state: &[(String, String)]) {
        // Any persisted rows mean this is a warm restart: the seen-set is
        // being restored, so the cold-start backlog baseline is skipped and
        // down-gap messages surface as unseen. Guarded to the FIRST import:
        // ADR-172's per-poll drain-mark refresh also lands here, and it must
        // neither re-log the restore banner every poll nor retroactively
        // flip `checkpoint_loaded` mid-run.
        let first_import = !self.state_imported;
        self.state_imported = true;
        if first_import {
            self.checkpoint_loaded = !state.is_empty();
        }
        for (key, value) in state {
            match key.as_str() {
                "seen_signal" => { self.seen_signals.insert(value.clone()); }
                "reply_hint_shown" => { self.reply_hint_shown = value == "true"; }
                // Legacy "signal_salience" rows (from before the message
                // lane stopped using the per-signal gate) are ignored.
                _ => {}
            }
        }
        if first_import && !self.seen_signals.is_empty() {
            eprintln!("[attend] peers: restored {} seen signals from checkpoint",
                self.seen_signals.len());
        }
    }
}

// --- Helpers ---

/// The fields this sensor keeps from a session record. Records without a
/// cwd are skipped. The cwd is the identity root, not the live cwd (#394), so
/// peer-presence labels agree with the normalized identity everywhere else.
fn session_file(record: claude_sessions::SessionRecord) -> Option<SessionFile> {
    let cwd = attend_presence::session::normalize_origin(&record.cwd?);
    Some(SessionFile { pid: record.pid, cwd, session_id: record.session_id })
}

// ── Event header ────────────────────────────────────────────────

/// The `message from …: ` prefix of one Monitor event line. Chunked
/// messages carry an `(i/n)` counter after the sender.
fn message_header(sender: &str, chunk_index: usize, total: usize) -> String {
    if total == 1 {
        format!("message from {sender}: ")
    } else {
        format!("message from {sender} ({}/{total}): ", chunk_index + 1)
    }
}

// ── Message chunking ────────────────────────────────────────────

/// Max body characters per chunk.
///
/// Monitor truncates stdout lines around 400 characters. The full event
/// line the emit pipeline produces looks like:
///
/// ```text
/// [attend sensor=peers priority=high] message from {sender} ({i}/{n}): {body}{reply_hint?}
/// ```
///
/// With a long sender name the prefix overhead can reach ~80 characters.
/// The reply hint (` (reply: attend send <msg>)`) is 29 characters and is
/// appended to the first chunk's body when shown. Reserving both:
///
/// ```text
/// 400 − 80 (prefix) − 29 (reply hint) = 291 chars available for body
/// ```
///
/// We use 260 to leave a defensible safety margin against prefix variation
/// (longer sender labels, large (N/M) counters) and to ensure even a fully
/// packed first chunk with reply hint stays clearly under the ceiling —
/// 260 + 29 + 80 = 369, ~30 chars below 400.
const MAX_CHUNK_BODY: usize = 260;

/// Cap on chunks per message. A message that splits into more than this many
/// pieces gets its tail replaced with an overflow hint pointing at the inbox.
/// 3 is plenty for conversational chatter and stays well under Monitor's
/// per-notification safety ceiling.
const MAX_CHUNKS: usize = 3;

/// A single poll surfacing more than this many messages is a flood — a
/// warm rejoin after a down-gap, or a hyperactive peer. Above it, the poll
/// coalesces into one count-led digest rather than N events. ~8 is the
/// uninvited-interrupt ceiling agreed for the re-entry digest.
const DIGEST_THRESHOLD: usize = 8;

/// Roll a flood of pending messages into one count-led digest line: counts
/// per room, the age of the newest, and the span back to the oldest.
/// Magnitude is the loudest of the batch so the digest still surfaces.
fn build_digest(pending: &[PendingMsg]) -> (f64, String) {
    let (mut directed, mut open, mut group) = (0usize, 0usize, 0usize);
    let mut max_mag = 0.0f64;
    let mut newest = u64::MAX;
    let mut oldest = 0u64;
    for m in pending {
        match m.kind {
            MsgKind::Directed => directed += 1,
            MsgKind::Open => open += 1,
            MsgKind::Group => group += 1,
        }
        if m.magnitude > max_mag {
            max_mag = m.magnitude;
        }
        newest = newest.min(m.age_secs);
        oldest = oldest.max(m.age_secs);
    }
    let mut parts = Vec::new();
    if directed > 0 {
        parts.push(format!("{directed} to you"));
    }
    if open > 0 {
        parts.push(format!("{open} on #open"));
    }
    if group > 0 {
        parts.push(format!("{group} in groups"));
    }
    let body = format!(
        "{} new messages: {} (newest {} ago, over {}) — attend inbox for detail",
        pending.len(),
        parts.join(", "),
        fmt_ago(newest),
        fmt_ago(oldest),
    );
    (max_mag, body)
}

/// Compact relative age: seconds under 90s, then minutes / hours / days,
/// each rounded to nearest.
fn fmt_ago(secs: u64) -> String {
    if secs < 90 {
        format!("{secs}s")
    } else if secs < 5400 {
        format!("{}m", (secs + 30) / 60)
    } else if secs < 129_600 {
        format!("{}h", (secs + 1800) / 3600)
    } else {
        format!("{}d", (secs + 43_200) / 86_400)
    }
}

/// Split a message into word-boundary chunks, each ≤ `chunk_size` characters,
/// capped at `max_chunks`. Long messages get their tail replaced with an
/// overflow hint on the final kept chunk.
///
/// Words longer than `chunk_size` get their own chunk that necessarily
/// exceeds the limit — we refuse to corrupt UTF-8 by char-slicing mid-word.
/// This is fine for peer chatter where single words are bounded by natural
/// language, and rare-case overflow is still readable even if truncated.
fn chunk_message(message: &str, chunk_size: usize, max_chunks: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in message.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= chunk_size {
            current.push(' ');
            current.push_str(word);
        } else {
            chunks.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    if chunks.len() > max_chunks {
        let overflow_hint = " …see `attend inbox` for full message";
        let hint_chars = overflow_hint.chars().count();
        chunks.truncate(max_chunks);
        if let Some(last) = chunks.last_mut() {
            // Only trim the last chunk if the budget can meaningfully fit
            // the hint. If chunk_size is smaller than the hint itself
            // (pathological tiny budget), skip trimming — the chunk will
            // exceed the nominal budget but will still carry its content
            // plus the hint. A slightly over-budget chunk with context
            // beats an empty chunk that only carries the hint.
            if chunk_size > hint_chars {
                let target = chunk_size - hint_chars;
                let original_len = last.chars().count();
                while last.chars().count() > target {
                    last.pop();
                }
                let did_trim = last.chars().count() < original_len;
                // If we actually trimmed and the remaining content still
                // has whitespace to cut back to, strip the partial word at
                // the tail so the hint attaches at a clean word boundary.
                // If the last chunk is a single unbroken word (pathological
                // long-word case) or we didn't trim at all, leave it alone.
                if did_trim && last.chars().any(|c| c.is_whitespace()) {
                    while let Some(c) = last.chars().last() {
                        last.pop();
                        if c.is_whitespace() {
                            break;
                        }
                    }
                }
            }
            last.push_str(overflow_hint);
        }
    }

    // Guarantee at least one chunk for an empty message (preserves the
    // "message from X:" notification even when the body is blank).
    if chunks.is_empty() {
        chunks.push(String::new());
    }

    chunks
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

    #[test]
    fn short_message_single_chunk() {
        let chunks = chunk_message("hello there", 100, 3);
        assert_eq!(chunks, vec!["hello there"]);
    }

    fn pmsg(kind: MsgKind, age_secs: u64, magnitude: f64) -> PendingMsg {
        PendingMsg {
            magnitude,
            from: "claude:x".to_string(),
            cwd: "/x".to_string(),
            body: "hi".to_string(),
            kind,
            age_secs,
        }
    }

    #[test]
    fn message_header_counts_only_chunked_messages() {
        assert_eq!(
            message_header("Jovan-alpha (ws)", 0, 1),
            "message from Jovan-alpha (ws): "
        );
        assert_eq!(
            message_header("Jovan-alpha (ws)", 1, 3),
            "message from Jovan-alpha (ws) (2/3): "
        );
    }

    #[test]
    fn fmt_ago_scales_units() {
        assert_eq!(fmt_ago(5), "5s");
        assert_eq!(fmt_ago(120), "2m");
        // Minutes band runs to 90m for precision, so 60m stays "60m".
        assert_eq!(fmt_ago(3600), "60m");
        assert_eq!(fmt_ago(7200), "2h");
        assert_eq!(fmt_ago(2 * 86_400), "2d");
    }

    #[test]
    fn digest_breaks_down_by_room_and_span() {
        let pending = vec![
            pmsg(MsgKind::Directed, 120, 7.0),
            pmsg(MsgKind::Directed, 600, 7.0),
            pmsg(MsgKind::Open, 1260, 4.0),
        ];
        let (mag, body) = build_digest(&pending);
        // Loudest of the batch surfaces the digest.
        assert_eq!(mag, 7.0);
        assert!(body.starts_with("3 new messages: "), "{body}");
        assert!(body.contains("2 to you"), "{body}");
        assert!(body.contains("1 on #open"), "{body}");
        // Newest is 120s (2m); span back to the oldest is 1260s (21m).
        assert!(body.contains("newest 2m ago"), "{body}");
        assert!(body.contains("over 21m"), "{body}");
        assert!(body.contains("attend inbox"), "{body}");
    }

    #[test]
    fn digest_omits_empty_rooms() {
        let pending = vec![pmsg(MsgKind::Open, 60, 4.0), pmsg(MsgKind::Open, 30, 4.0)];
        let (_, body) = build_digest(&pending);
        assert!(body.contains("2 on #open"), "{body}");
        assert!(!body.contains("to you"), "{body}");
        assert!(!body.contains("in groups"), "{body}");
    }

    #[test]
    fn long_message_splits_at_word_boundaries() {
        // 3 words of 10 chars each = 30 chars; chunk size 15 → two chunks.
        let msg = "aaaaaaaaaa bbbbbbbbbb cccccccccc";
        let chunks = chunk_message(msg, 15, 3);
        assert_eq!(chunks, vec!["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc"]);
    }

    #[test]
    fn word_joining_respects_budget() {
        // "foo bar baz" is 11 chars; budget 11 → one chunk. Budget 10 → two.
        assert_eq!(chunk_message("foo bar baz", 11, 3), vec!["foo bar baz"]);
        assert_eq!(chunk_message("foo bar baz", 10, 3), vec!["foo bar", "baz"]);
    }

    #[test]
    fn overflow_hint_replaces_tail_when_exceeds_max_chunks() {
        // 5 chunks worth of content, max 3 chunks.
        let msg = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
        let chunks = chunk_message(msg, 10, 3);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.last().unwrap().contains("…see `attend inbox` for full message"));
    }

    #[test]
    fn empty_message_yields_one_empty_chunk() {
        let chunks = chunk_message("", 100, 3);
        assert_eq!(chunks, vec![""]);
    }

    #[test]
    fn single_word_longer_than_budget_still_emits() {
        // A 30-char word with a 10-char budget — we refuse to mid-slice.
        let msg = "supercalifragilisticexpialidocious trailing";
        let chunks = chunk_message(msg, 10, 3);
        // First chunk is the long word (exceeds budget — defensible fallback).
        assert_eq!(chunks[0], "supercalifragilisticexpialidocious");
        assert_eq!(chunks[1], "trailing");
    }

    #[test]
    fn overflow_preserves_content_when_last_chunk_has_no_whitespace() {
        // Simulate: many short words, then a single long unbroken word
        // as the final kept chunk's content. The overflow trim must not
        // annihilate the long word just because there's no whitespace
        // inside it — it's better to be slightly over budget than empty.
        let msg = "a b c d e f g h i j aaaaaaaaaaaaaaaaaaaaa continuation tail";
        let chunks = chunk_message(msg, 10, 3);
        assert_eq!(chunks.len(), 3);
        let last = chunks.last().unwrap();
        assert!(last.contains("…see `attend inbox` for full message"));
        // The pre-hint portion must not be empty.
        let hint_idx = last.find(" …see").unwrap();
        assert!(hint_idx > 0, "chunk content was annihilated before hint: {:?}", last);
    }

    #[test]
    fn utf8_is_counted_by_chars_not_bytes() {
        // "café" is 4 chars but 5 bytes. Budget 4 fits, budget 3 does not.
        let msg = "café rouge";
        assert_eq!(chunk_message(msg, 10, 3), vec!["café rouge"]);
        assert_eq!(chunk_message(msg, 4, 3), vec!["café", "rouge"]);
    }
}
