//! Replay data: the events read from the log, the ways active at a frame,
//! and the frames.

use crate::cmd::render::WayRow;

/// A way event from events.jsonl.
#[derive(Default)]
pub(crate) struct WayEvent {
    pub(super) ts: String,
    pub(super) event: String,
    pub(super) way: String,
    pub(super) trigger: String,
    pub(super) check: String,
    /// The relevance gate's P(yes) and verdict on a `way_judged` event (ADR-196).
    pub(super) p_yes: String,
    pub(super) verdict: String,
    /// The blocked ancestor of a way the gate blocked with it, unjudged.
    pub(super) ancestor: String,
    /// On an `injection_suppressed` event (#786): the switch that applied
    /// (`session` or `config`), the lane it held back, and the agent.
    pub(super) switch: String,
    pub(super) lane: String,
    pub(super) agent: String,
    /// The agent a fire, re-disclosure or check was delivered to: `main`,
    /// or the subagent's id. Empty where the event does not record it.
    pub(super) agent_id: String,
    /// On a `way_suppressed` event: `way` or `check`, and the reason
    /// (`refire` or `context_cap`).
    pub(super) kind: String,
    pub(super) reason: String,
}

/// Ways held back from a subagent by the subagent switch (#768, #786):
/// one Task dispatch (lane `task`), or one agent's hooks, logged once per
/// agent.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct Suppression {
    /// `session` (`ways session subagents off`) or `config` (`subagents: false`).
    pub(crate) switch: String,
    pub(crate) lane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<String>,
}

impl Suppression {
    /// A Task dispatch, rather than an agent's own hooks.
    pub(crate) fn is_dispatch(&self) -> bool {
        self.lane == "task"
    }

    /// How the timeline names it: `dispatch` or `agent <id>`, and the switch.
    pub(crate) fn label(&self) -> String {
        let what = match (&self.agent, self.is_dispatch()) {
            (_, true) => "dispatch".to_string(),
            (Some(a), false) => format!("agent {a}"),
            (None, false) => "agent".to_string(),
        };
        format!("{what} ({} switch)", self.switch)
    }
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
    /// Matched again inside its refire window and not shown again
    /// (`way_suppressed`, reason `refire`). Like a block, it injected
    /// nothing and shows only in its frame.
    RefireHeld,
    /// Matched but withheld by the context cap (`way_suppressed`, reason
    /// `context_cap`); only in its frame.
    CapHeld,
}

impl Outcome {
    /// The name `session replay --json` gives the outcome; its frames take
    /// it in the CLI's change for #742.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Outcome::Injected => "injected",
            Outcome::Blocked => "blocked",
            Outcome::WouldBlock => "would_block",
            Outcome::RefireHeld => "refire_suppressed",
            Outcome::CapHeld => "context_cap",
        }
    }

    /// Whether the way reached the session: a shadow would-block did.
    pub(crate) fn injected(self) -> bool {
        matches!(self, Outcome::Injected | Outcome::WouldBlock)
    }
}

/// What happened to a way, as its row shows it: [`Fate::of`] picks one,
/// and `table::look` gives each its mark and colour, so the two agree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fate {
    /// Fired and injected.
    Injected,
    /// Its latest injection was a re-disclosure.
    Redisclosed,
    /// Injected, and its check has fired since.
    CheckFired,
    /// Injected; the judge, in shadow, would have kept it out.
    WouldBlock,
    /// The judge kept it out.
    Blocked,
    /// The refire window held it back.
    RefireHeld,
    /// The context cap withheld it.
    CapHeld,
}

impl Fate {
    /// A withheld row's outcome names it; an injected row's shadow verdict
    /// comes first, then a re-disclosure, then a check.
    pub(crate) fn of(w: &ActiveWay) -> Fate {
        match w.outcome {
            Outcome::Blocked => Fate::Blocked,
            Outcome::RefireHeld => Fate::RefireHeld,
            Outcome::CapHeld => Fate::CapHeld,
            Outcome::WouldBlock => Fate::WouldBlock,
            Outcome::Injected if w.by_redisclosure => Fate::Redisclosed,
            Outcome::Injected if w.check_fires > 0 => Fate::CheckFired,
            Outcome::Injected => Fate::Injected,
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
    /// On a row blocked with its ancestor, the ancestor whose P(yes) it
    /// shows; empty otherwise.
    pub(crate) ancestor: String,
    /// The agent the way was injected into: `main` or a subagent's id. A
    /// way several agents fired is a row per agent. Empty on a blocked row
    /// whose verdict did not record its agent.
    pub(crate) agent: String,
    /// Whether its latest injection was a re-disclosure rather than a
    /// fire; unlike `is_redisclosed`, it holds past the frame it happened in.
    pub(crate) by_redisclosure: bool,
}

impl WayRow for ActiveWay {
    fn id(&self) -> &str { &self.id }
    fn epoch_fired(&self) -> u64 { self.epoch_fired }
    fn token_pos(&self) -> u64 { self.token_pos }
    fn trigger(&self) -> &str { &self.trigger }
    fn check_fires(&self) -> u64 { self.check_fires }
    fn refire_threshold_k(&self) -> u64 { self.refire_threshold_k }
    fn agent_id(&self) -> &str { &self.agent }
}

/// A single frame in the replay.
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) epoch: u64,
    pub(crate) timestamp: String,
    pub(crate) elapsed_secs: u64,
    pub(crate) token_position_k: u64,
    /// The injected ways in (epoch fired, id, agent) order, then the
    /// candidates the judge blocked or the refire window or context cap held
    /// back in this frame, by id. [`Frame::shown`] filters them.
    pub(crate) ways: Vec<ActiveWay>,
    pub(crate) new_events: Vec<String>,
    /// Ways the subagent switch held back in this frame, in log order.
    pub(crate) suppressed: Vec<Suppression>,
    /// Which compaction window (1-based) this frame belongs to. A long session is
    /// segmented at each `session_start` boundary; epoch/distance restart per window
    /// and the accumulated ways reset, so the latest window mirrors `ways session ways`. The
    /// boundary itself surfaces as a `⎯ compaction ⎯` entry in `new_events`.
    pub(crate) window: u64,
}

impl Frame {
    /// This frame as one view shows it: the injected ways (a shadow
    /// would-block was injected, so it stays), or with `matched` every
    /// matched candidate, the ones the judge, the refire window or the
    /// context cap kept out too.
    pub(crate) fn shown(&self, matched: bool) -> Frame {
        let mut f = self.clone();
        if !matched {
            f.ways.retain(|w| w.outcome.injected());
        }
        f
    }

    /// How many candidates the judge blocked in this frame.
    pub(crate) fn blocked(&self) -> usize {
        self.ways.iter().filter(|w| w.outcome == Outcome::Blocked).count()
    }
}
