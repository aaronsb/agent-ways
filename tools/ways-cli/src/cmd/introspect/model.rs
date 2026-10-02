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

/// What the relevance judge's verdict left of a way in a frame (ADR-196).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Outcome {
    /// Injected into the session; the judge passed it or did not see it.
    Injected,
    /// Kept out by the judge in enforce mode: P(yes) under the threshold.
    /// It injected nothing, and shows only in the frame it was judged in.
    Blocked,
    /// Injected, but in shadow mode the judge would have kept it out.
    WouldBlock,
}

impl Outcome {
    /// The name `session replay --json` gives the outcome; its frames take
    /// it in the CLI's change for #742.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Outcome::Injected => "injected",
            Outcome::Blocked => "blocked",
            Outcome::WouldBlock => "would_block",
        }
    }

    /// The row mark the screens draw before the way's id; none when injected.
    pub(crate) fn mark(self) -> &'static str {
        match self {
            Outcome::Injected => "",
            Outcome::Blocked => "⊘ ",
            Outcome::WouldBlock => "◌ ",
        }
    }
}

/// A way at a given frame: an active way, or a candidate the judge kept out.
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
    pub(crate) outcome: Outcome,
    /// The judge's P(yes) on a blocked or would-block row; empty otherwise.
    pub(crate) p_yes: String,
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
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) epoch: u64,
    pub(crate) timestamp: String,
    pub(crate) elapsed_secs: u64,
    pub(crate) token_position_k: u64,
    /// The injected ways in (epoch fired, id) order, then the candidates the
    /// judge blocked in this frame, by id. [`Frame::shown`] filters them.
    pub(crate) ways: Vec<ActiveWay>,
    pub(crate) new_events: Vec<String>,
    /// Which compaction window (1-based) this frame belongs to. A long session is
    /// segmented at each `session_start` boundary; epoch/distance restart per window
    /// and the accumulated ways reset, so the latest window mirrors `ways session ways`. The
    /// boundary itself surfaces as a `⎯ compaction ⎯` entry in `new_events`.
    pub(crate) window: u64,
}

impl Frame {
    /// This frame as one view shows it: the injected ways (a shadow
    /// would-block was injected, so it stays), or with `matched` every
    /// matched candidate, the judge-blocked ones too.
    pub(crate) fn shown(&self, matched: bool) -> Frame {
        let mut f = self.clone();
        if !matched {
            f.ways.retain(|w| w.outcome != Outcome::Blocked);
        }
        f
    }

    /// How many candidates the judge blocked in this frame.
    pub(crate) fn blocked(&self) -> usize {
        self.ways.iter().filter(|w| w.outcome == Outcome::Blocked).count()
    }
}
