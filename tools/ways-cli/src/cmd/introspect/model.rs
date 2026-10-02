//! Replay data: the events read from the log, the ways active at a frame,
//! and the frames.

use crate::cmd::render::WayRow;

/// A way event from events.jsonl.
pub(crate) struct WayEvent {
    pub(super) ts: String,
    pub(super) event: String,
    pub(super) way: String,
    pub(super) trigger: String,
    pub(super) check: String,
    /// The relevance gate's P(yes) and verdict on a `way_judged` event (ADR-196).
    pub(super) p_yes: String,
    pub(super) verdict: String,
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
    /// Which compaction window (1-based) this frame belongs to. A long session is
    /// segmented at each `session_start` boundary; epoch/distance restart per window
    /// and the accumulated ways reset, so the latest window mirrors `ways session ways`. The
    /// boundary itself surfaces as a `⎯ compaction ⎯` entry in `new_events`.
    pub(crate) window: u64,
}
