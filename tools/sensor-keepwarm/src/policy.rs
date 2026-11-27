// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The continuity policy. Its idle path: in an
//! interactive session with autocompact off, or with relay on, an idle
//! session is kept warm from every turn's end, one `keepwarm:` wake
//! `margin` before its cache would lapse, counted from the operator's last
//! prompt and allowed only inside the configured windows. At the first due
//! point past the count or outside the windows the session gets one
//! `wrap:` turn.
//!
//! What follows a wrap depends on the mode (`relay`). In handoff mode, with
//! `after_wrap_keepalives` the keepalives go on that many more times after
//! the wrap; then the cache lapses, or with `after_wrap: compact` the
//! session gets one compaction with the focus the wrap's closing `/compact`
//! line gave. In relay mode the wrap ends with a relay marker; with a fresh
//! handoff the mod compacts the conversation with it and submits a turn
//! that resumes the work, up to `max_relays` times. A stop the agent
//! reports, the cap, a stale handoff or an ended goal stops the relay.
//! Relay also has a fill path: a wrap once the context reaches
//! `relay_at_pct` of the window. Nothing more follows a stop until the
//! operator prompts again.
//!
//! The judgment lives here; the attend mod supplies what only it can see
//! (the operator's prompts, through [`reset`], the autocompact gate, the
//! context fill, the wrap's marker) and carries out what [`step`] returns.
//! The counts and their marks ride the keepwarm ledger ([`Policy`]), so the
//! plain check and the sensor keep them.
//!
//! References:
//! - ADR-414: the keepalive policy, which is the idle path here.
//! - ADR-417: one continuity policy with a relay mode and a fill path.

use crate::windows::Windows;
use crate::{
    cache_lapsed, fmt_duration, fmt_tok, fmt_usd, wake_line, ArmFile, Outcome, Reading, State, BIG_TOKENS, PING_AFTER_SECS, TTL_SECS,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `mods.attend.continuity.max_keepalives`.
pub const DEFAULT_MAX: u32 = 8;
pub const MAX_RANGE: (u32, u32) = (0, 48);
/// `mods.attend.continuity.margin_minutes`. The default puts the wake at
/// the 50-minute point on the hour tier, where keepwarm's own wake falls.
///
/// References:
/// - ADR-182: keepwarm wakes an idle session after 50 minutes on the hour tier.
pub const DEFAULT_MARGIN_MIN: u64 = 10;
pub const MARGIN_RANGE: (u64, u64) = (3, 30);
/// `mods.attend.continuity.after_wrap_keepalives`.
pub const DEFAULT_AFTER_MAX: u32 = 0;
pub const AFTER_RANGE: (u32, u32) = (0, 48);
/// `mods.attend.continuity.relay_at_pct`: the context fill, in percent of
/// the window, at which relay mode asks for a wrap.
///
/// References:
/// - ADR-417 item 2a: a context fill past the threshold starts a wrap.
pub const DEFAULT_RELAY_AT_PCT: u32 = 75;
pub const RELAY_AT_RANGE: (u32, u32) = (50, 95);
/// `mods.attend.continuity.max_relays`: resumes since the operator's last
/// prompt.
///
/// References:
/// - ADR-417 item 5: the resumes are capped per operator prompt.
pub const DEFAULT_MAX_RELAYS: u32 = 5;
pub const MAX_RELAYS_RANGE: (u32, u32) = (0, 50);
/// With Claude Code's autocompact on, the fill trigger sits this many
/// points of the window under its threshold, so the relay's wrap comes
/// first.
const NATIVE_MARGIN_PCT: u64 = 5;
/// `mods.attend.continuity.wrap_skill`: the skill's name and its arguments.
pub const DEFAULT_WRAP_SKILL: &str = "wrap stay";
/// The longest skill name `wrap_skill` may give.
const SKILL_MAX_LEN: usize = 32;
/// `wrap_skill`'s arguments: this many words at most, each this long at
/// most, enough for `stay` and too few for a sentence of instructions.
const SKILL_MAX_ARGS: usize = 2;
const SKILL_ARG_MAX_LEN: usize = 16;
/// How much of a refused configured value the card and the reasons echo.
const ECHO_MAX_LEN: usize = 64;

/// A configured value as the card and the reasons echo it: control
/// characters replaced and the length bounded, since a project's
/// checked-in config reaches the operator's terminal this way.
fn echo(value: &str) -> String {
    let clean: String = value.chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    match clean.char_indices().nth(ECHO_MAX_LEN) {
        Some((at, _)) => format!("{}…", &clean[..at]),
        None => clean,
    }
}
/// `mods.attend.continuity.wrap_file`.
pub const DEFAULT_WRAP_FILE: &str = "{state}/wrap/{session}.md";
/// How long a wake may go without its turn before the verdict stops the
/// policy (ADR-182's ten minutes).
const VERDICT_SECS: u64 = TTL_SECS - PING_AFTER_SECS;
/// When the check should be asked again while the policy does not run: the
/// plain check's cadence.
const PLAIN_EVERY_SECS: u64 = 60;

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(f: &f64) -> bool {
    *f == 0.0
}

fn is_zero_count(n: &u32) -> bool {
    *n == 0
}

/// The policy's part of the ledger (ADR-414 item 7).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    /// Keepalives sent since the operator's last prompt.
    #[serde(default)]
    pub count: u32,
    /// The operator's last prompt, as the mod reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_at: Option<u64>,
    /// Where no operator prompt was reported: the end of the turn the first
    /// keepalive of this count followed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretch_from: Option<u64>,
    /// When the wrap turn was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapped_at: Option<u64>,
    /// Why the policy stopped until the operator's next prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    /// Where the wrap's reply went, as the mod reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap_outcome: Option<String>,
    /// The autocompact gate as the mod last read it: `on`, `off`, `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// A keepalive this policy sent waits for its turn's verdict.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pending: bool,
    /// What this count's keepalives cost, from their verdicts.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub spent_usd: f64,
    /// Keepalives sent since the wrap (`after_wrap_keepalives`).
    #[serde(default, skip_serializing_if = "is_zero_count")]
    pub after_count: u32,
    /// The focus of the wrap's closing `/compact` line, as the mod reported
    /// it once the wrap answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_focus: Option<String>,
    /// When the compaction after the wrap was asked for, in handoff mode;
    /// in relay mode, when the mod reported that it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<u64>,
    /// A stopped relay still owes the compaction with the handoff, until
    /// the mod reports it ran or failed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub compact_due: bool,
    /// Why that compaction failed, as the mod reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_failed: Option<String>,
    /// Whether the last check found `wrap_skill` among the session's skills,
    /// for the status card, which has no listing of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_seen: Option<SkillSeen>,
    /// The last check came from a host that passes no skills flag, a mod
    /// from before it, which gets no stretch after the wrap; the card,
    /// which reads the settings alone, follows it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub old_host: bool,
    /// The wrap now judged runs in relay mode: a compaction and a resume
    /// follow it, not `after_wrap`.
    ///
    /// References:
    /// - ADR-417 item 4: in relay mode a compaction and a resume follow the wrap.
    #[serde(default, skip_serializing_if = "is_false")]
    pub relay: bool,
    /// What started that wrap: `idle`, `fill`, or `wrap` (a `/wrap`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// The id of the wrap now judged: issued in a wrap turn the policy
    /// sent, or read from a `/wrap`'s marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap_id: Option<String>,
    /// The id of this session's last recorded wrap, which outlives the
    /// operator's prompts: `attend keepwarm wrap-id` gives it to the
    /// PreCompact hook.
    ///
    /// References:
    /// - ADR-417 item 3: the PreCompact hook asks attend for the last wrap id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_wrap_id: Option<String>,
    /// The relay marker the wrap's reply ended with, as the mod reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// Resumes submitted since the operator's last prompt.
    #[serde(default, skip_serializing_if = "is_zero_count")]
    pub relays: u32,
    /// When the last resume went in; cleared once its turn has made a
    /// request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_at: Option<u64>,
    /// Why the relay stopped, in a few words for the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_stop: Option<String>,
    /// The stop's handoff went to the mod to send, once.
    #[serde(default, skip_serializing_if = "is_false")]
    pub handed: bool,
    /// Whether a `/goal` is active, as the mod saw it typed or the last
    /// marker reported it: `active` or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
}

/// What the last check found of `wrap_skill` in the session's listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSeen {
    Listed,
    Missing,
    Unread,
}

impl Policy {
    pub fn is_empty(&self) -> bool {
        *self == Policy::default()
    }

    /// Whether the next judgment reads the handoff: a relay wrap's marker
    /// waits to be judged or compacted on, or the conversation was compacted
    /// after it and the resume never went in.
    pub fn wants_handoff(&self) -> bool {
        self.relay && self.marker.is_some() && (self.stopped.is_none() || (self.compact_due && self.compacted_at.is_none()))
    }

    /// Where the count runs from: the operator's prompt, else the stretch.
    fn since(&self) -> Option<u64> {
        self.operator_at.or(self.stretch_from)
    }
}

/// The operator prompted (`attend keepwarm reset`): both counts start
/// again, and a sent wrap and a stop are cleared. The last wrap's id and
/// the goal stay: the PreCompact hook still checks the handoff against the
/// id, and a prompt does not end a goal.
pub fn reset(p: &mut Policy, now: u64) {
    *p = Policy { operator_at: Some(now), gate: p.gate.take(), last_wrap_id: p.last_wrap_id.take(), goal: p.goal.take(), ..Policy::default() };
}

/// A relay's resume went in (`attend keepwarm resumed`): the relay count
/// goes up by one, the keepalive count starts again, and the wrap is
/// cleared, so the next generation can wrap again.
///
/// References:
/// - ADR-417 item 4: a submitted resume counts one relay and restarts the
///   keepalive count.
pub fn resumed(p: &mut Policy, now: u64) {
    *p = Policy {
        operator_at: p.operator_at,
        stretch_from: p.stretch_from,
        gate: p.gate.take(),
        spent_usd: p.spent_usd,
        skill_seen: p.skill_seen,
        old_host: p.old_host,
        last_wrap_id: p.last_wrap_id.take(),
        goal: p.goal.take(),
        relays: p.relays + 1,
        resumed_at: Some(now),
        ..Policy::default()
    };
}

/// The relay stops until the operator's next prompt: `why` for the card,
/// `short` for the row.
pub fn stop_relay(p: &mut Policy, short: &str, why: impl Into<String>) {
    p.stopped = Some(why.into());
    p.relay_stop = Some(short.into());
}

/// `attend keepwarm wrapped`: where the wrap's reply went (`line`, the
/// card's words for `outcome`), the focus of its closing `/compact` line,
/// and its relay marker. A marker with no relay wrap waiting is a `/wrap`
/// run in relay mode, recorded as a wrap now. An unfinished relay wrap
/// stops the relay.
///
/// References:
/// - ADR-417 item 2b: a `/wrap` in relay mode ends with the marker and is
///   handled as a wrap the policy sent.
pub fn wrapped(p: &mut Policy, outcome: WrapOutcome, line: String, focus: Option<String>, marker: Option<String>, now: u64) {
    // An unfinished wrap wrote no handoff: nothing compacts after it.
    let focus = focus.filter(|_| outcome != WrapOutcome::Unfinished);
    if let Some(m) = marker.filter(|_| outcome != WrapOutcome::Unfinished) {
        if !(p.relay && p.wrapped_at.is_some() && p.marker.is_none() && p.stopped.is_none()) {
            p.wrapped_at = Some(now);
            p.relay = true;
            p.trigger = Some("wrap".into());
            p.wrap_id = None;
            p.compacted_at = None;
            p.compact_failed = None;
            p.compact_due = false;
            p.handed = false;
        }
        p.marker = Some(m);
    } else if outcome == WrapOutcome::Unfinished && p.relay && p.wrapped_at.is_some() && p.stopped.is_none() {
        stop_relay(p, "wrap unfinished", line.clone());
    }
    p.wrap_outcome = Some(line);
    if focus.is_some() {
        p.compact_focus = focus;
    }
}

/// `attend keepwarm compacted`: the compaction after the wrap ran, or why
/// it failed. Only a wrapped stretch is compacted: a report that lands
/// after the operator's prompt reset the ledger changes nothing. A relay
/// compaction is recorded here and not when it is asked for, so one the
/// mod deferred is asked for again. A relay compaction that failed stops
/// the relay.
///
/// References:
/// - ADR-417 item 4: a compaction that is skipped or fails stops the relay.
pub fn compacted(p: &mut Policy, failed: Option<String>, now: u64) {
    if p.wrapped_at.is_none() {
        return;
    }
    p.compacted_at.get_or_insert(now);
    p.compact_due = false;
    if let (true, Some(why), None) = (p.relay, &failed, &p.stopped) {
        stop_relay(p, "compaction failed", format!("the compaction failed: {why}"));
    }
    p.compact_failed = failed;
}

/// `attend keepwarm resumed --failed`: the resume did not go in, which
/// stops the relay. A resume that went in is [`resumed`].
pub fn resume_failed(p: &mut Policy, why: &str) {
    if p.relay && p.wrapped_at.is_some() {
        stop_relay(p, "resume failed", format!("the resume did not go in: {why}"));
    }
}

/// `attend keepwarm stopped`: a resumed generation ended a turn with a
/// `relay: stop <reason>` line. Only a session that has relayed since the
/// operator's prompt has one; false where nothing was recorded.
pub fn stopped_by_agent(p: &mut Policy, reason: &str, detail: &str) -> bool {
    if p.relays == 0 || !STOP_REASONS.contains(&reason) {
        return false;
    }
    let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let why = if detail.is_empty() { format!("the agent stopped the relay: {reason}") } else { format!("the agent stopped the relay: {reason}, {}", echo(&detail)) };
    stop_relay(p, reason, why);
    true
}

/// `attend keepwarm goal`: the mod saw a `/goal` typed, setting one
/// (`active`) or clearing it (`none`).
pub fn goal(p: &mut Policy, active: bool) {
    p.goal = Some(if active { "active" } else { "none" }.into());
}

/// A wrap's relay marker, the last line of its reply:
/// `relay: continue tasks=3 goal=none wrap=<id> head=<sha>`, or
/// `relay: stop <empty|decision|doubt>` and the same fields.
///
/// References:
/// - ADR-417 item 8: the marker's form and fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    /// `None` for `continue`; `empty`, `decision` or `doubt` for a stop.
    pub stop: Option<&'static str>,
    pub tasks: u32,
    pub goal_active: bool,
    pub wrap: String,
    pub head: String,
}

/// The reasons a stop names.
const STOP_REASONS: [&str; 3] = ["empty", "decision", "doubt"];

impl Marker {
    /// The marker `line` is, whole: `^relay: (continue|stop
    /// (empty|decision|doubt)) tasks=\d+ goal=(active|none)
    /// wrap=[0-9A-Za-z-]{8,64} head=[0-9a-f]{7,40}$`.
    pub fn parse(line: &str) -> Option<Marker> {
        let rest = line.strip_prefix("relay: ")?;
        let words: Vec<&str> = rest.split(' ').collect();
        let (stop, fields) = match *words.first()? {
            "continue" => (None, &words[1..]),
            "stop" => (Some(*STOP_REASONS.iter().find(|r| Some(**r) == words.get(1).copied())?), words.get(2..)?),
            _ => return None,
        };
        let [tasks, goal, wrap, head] = fields else { return None };
        let tasks = tasks.strip_prefix("tasks=").filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))?.parse().ok()?;
        let goal_active = match goal.strip_prefix("goal=")? {
            "active" => true,
            "none" => false,
            _ => return None,
        };
        let wrap = wrap.strip_prefix("wrap=").filter(|w| is_wrap_id(w))?;
        let head = head.strip_prefix("head=").filter(|h| (7..=40).contains(&h.len()) && h.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)))?;
        Some(Marker { stop, tasks, goal_active, wrap: wrap.into(), head: head.into() })
    }
}

/// The reason a resumed turn's closing `relay: stop <reason> ...` line
/// names, where the line is one and not a whole marker.
pub fn stop_line(line: &str) -> Option<&'static str> {
    if Marker::parse(line).is_some() {
        return None;
    }
    let rest = line.strip_prefix("relay: stop ")?;
    let word = rest.split(|c: char| c.is_whitespace() || matches!(c, ':' | ',' | '.' | ';')).next()?;
    STOP_REASONS.iter().find(|r| **r == word).copied()
}

/// `attend keepwarm wrap-id --record <id>`: a handoff-mode wrap names its
/// id, which becomes the session's last. False, recording nothing, for an
/// id the marker could not carry.
///
/// References:
/// - ADR-417 item 3: the PreCompact hook asks attend for the last wrap's id.
pub fn record_wrap_id(p: &mut Policy, id: &str) -> bool {
    if !is_wrap_id(id) {
        return false;
    }
    p.last_wrap_id = Some(id.to_string());
    true
}

/// Whether `id` has a wrap id's shape: 8 to 64 letters, digits and `-`.
fn is_wrap_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `attend keepwarm off`: the policy stops until the operator's next prompt.
pub fn stop_by_operator(p: &mut Policy) {
    p.stopped = Some("attend keepwarm off".into());
}

/// `attend keepwarm on`: the operator's word clears a stop.
pub fn clear_stop(p: &mut Policy) {
    p.stopped = None;
}

/// Where the wrap's final reply goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapDest {
    Transcript,
    File,
    Send,
    Off,
}

impl WrapDest {
    pub fn as_str(self) -> &'static str {
        match self {
            WrapDest::Transcript => "transcript",
            WrapDest::File => "file",
            WrapDest::Send => "send",
            WrapDest::Off => "off",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "transcript" => WrapDest::Transcript,
            "file" => WrapDest::File,
            "send" => WrapDest::Send,
            "off" => WrapDest::Off,
            _ => return None,
        })
    }
}

/// What follows the wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterWrap {
    Lapse,
    Compact,
}

impl AfterWrap {
    pub fn as_str(self) -> &'static str {
        match self {
            AfterWrap::Lapse => "lapse",
            AfterWrap::Compact => "compact",
        }
    }
}

/// `mods.attend.keepalive.wrap_skill`: the skill the wrap turn asks for,
/// and its arguments. A project's checked-in config can set it, and its
/// text goes into a turn nobody watches, so it is held to a skill name and
/// at most two short argument words: enough for `stay`, too few to carry a
/// sentence of instructions. This bounds the field to a skill invocation;
/// it is no defence against a project whose checked-in instructions are
/// hostile, which reach every turn anyway. Empty or refused, it names no
/// skill, and no wrap or compaction runs ([`Config::no_wrap`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrapSkill {
    /// Empty where none is set or the value was refused.
    pub name: String,
    pub args: String,
    /// The configured value, refused, and why.
    pub refused: Option<(String, &'static str)>,
}

impl Default for WrapSkill {
    fn default() -> Self {
        let (name, args) = DEFAULT_WRAP_SKILL.split_once(' ').unwrap_or((DEFAULT_WRAP_SKILL, ""));
        WrapSkill { name: name.into(), args: args.into(), refused: None }
    }
}

impl WrapSkill {
    /// `<skill> [arguments]`, a leading `/` allowed.
    pub fn parse(value: &str) -> WrapSkill {
        let v = value.trim();
        let v = v.strip_prefix('/').unwrap_or(v);
        if v.is_empty() {
            return WrapSkill { name: String::new(), args: String::new(), refused: None };
        }
        let (name, args) = v.split_once(char::is_whitespace).map_or((v, ""), |(n, a)| (n, a.trim()));
        let args = args.split_whitespace().collect::<Vec<_>>().join(" ");
        let refuse = |why| WrapSkill { name: String::new(), args: String::new(), refused: Some((echo(value.trim()), why)) };
        let name_ok = name.len() <= SKILL_MAX_LEN
            && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'));
        if !name_ok {
            return refuse("a skill name is letters, digits and : _ - . only");
        }
        let words: Vec<&str> = args.split(' ').filter(|w| !w.is_empty()).collect();
        let word_ok = |w: &&str| {
            w.len() <= SKILL_ARG_MAX_LEN
                && !w.starts_with('-')
                && w.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | '=' | ','))
        };
        if words.len() > SKILL_MAX_ARGS || !words.iter().all(word_ok) {
            return refuse("its arguments are at most two words of 16 letters, digits or : _ - . = , each, none opening with -");
        }
        WrapSkill { name: name.into(), args, refused: None }
    }

    /// `Run the wrap skill with the argument stay`.
    fn ask(&self) -> String {
        match self.args.split(' ').filter(|a| !a.is_empty()).count() {
            0 => format!("Run the {} skill", self.name),
            1 => format!("Run the {} skill with the argument {}", self.name, self.args),
            _ => format!("Run the {} skill with the arguments {}", self.name, self.args),
        }
    }
}

/// The autocompact gate as the mod read it (ADR-414 item 2). Unknown counts
/// as on: the side that starts no unbidden turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    On,
    Off,
    Unknown,
}

impl Gate {
    pub fn parse(s: &str) -> Gate {
        match s {
            "off" => Gate::Off,
            "on" => Gate::On,
            _ => Gate::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Gate::On => "on",
            Gate::Off => "off",
            Gate::Unknown => "unknown",
        }
    }
}

/// The `mods.attend` keys the policy reads (ADR-414 item 8).
#[derive(Clone, Debug)]
pub struct Config {
    /// The settings were read. Unread, the policy is off: `enabled` fails
    /// closed (ADR-414 item 8).
    pub settings_ok: bool,
    /// `mods.attend.keepwarm`: off turns off every cache wake the mod drives.
    pub keepwarm_on: bool,
    pub enabled: bool,
    pub max: u32,
    pub margin_secs: u64,
    pub windows: Windows,
    pub wrap: WrapDest,
    pub wrap_file: String,
    pub wrap_to: String,
    pub after_wrap: AfterWrap,
    /// Keepalives after the wrap before `after_wrap` applies.
    pub after_max: u32,
    pub wrap_skill: WrapSkill,
    /// `relay`: what follows a wrap is a compaction and a resume.
    ///
    /// References:
    /// - ADR-417 item 1: the `relay` setting picks relay mode or handoff mode.
    pub relay: bool,
    pub relay_at_pct: u32,
    pub max_relays: u32,
    /// The skills the session lists, as the host read them. Not a setting:
    /// the caller fills it.
    pub session_skills: SessionSkills,
    /// The context fill the host measured at a turn's end. Not a setting:
    /// the caller fills it.
    pub fill: Option<Fill>,
}

/// The context fill at a main-loop turn's end (`--fill`, `--window`,
/// `--native-at`), for relay's fill path.
///
/// References:
/// - ADR-417 item 2a: a context fill past the threshold starts a wrap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fill {
    pub tokens: u64,
    /// The model's whole window.
    pub window: u64,
    /// Claude Code's own autocompact threshold in tokens, where the host
    /// read one.
    pub native_at: Option<u64>,
}

/// What the host said of the session's skills (`--skills`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SessionSkills {
    /// The host passed nothing, as a mod from before the flag does: the
    /// wrap is not held to the listing.
    #[default]
    NotChecked,
    /// The host could not read the listing: no wrap.
    Unread,
    Listed(Vec<String>),
    /// A listing the host's token budget cut short: a skill missing from it
    /// may still be the session's, so it is not held against the wrap.
    Partial(Vec<String>),
}

impl Default for Config {
    fn default() -> Self {
        Config {
            settings_ok: true,
            keepwarm_on: true,
            enabled: true,
            max: DEFAULT_MAX,
            margin_secs: DEFAULT_MARGIN_MIN * 60,
            windows: Windows::default(),
            wrap: WrapDest::Transcript,
            wrap_file: DEFAULT_WRAP_FILE.into(),
            wrap_to: String::new(),
            after_wrap: AfterWrap::Lapse,
            after_max: DEFAULT_AFTER_MAX,
            wrap_skill: WrapSkill::default(),
            relay: false,
            relay_at_pct: DEFAULT_RELAY_AT_PCT,
            max_relays: DEFAULT_MAX_RELAYS,
            session_skills: SessionSkills::NotChecked,
            fill: None,
        }
    }
}

impl Config {
    /// From `ways settings list mods.attend --json --effective`. A key that
    /// is absent, or of the wrong type or out of range, keeps its default,
    /// except `keepalive`, which fails closed: only an explicit true runs
    /// the idle path; and `relay`, which only an explicit `on` turns on. No
    /// document, or one that does not parse, is the policy off. ways reads
    /// a key's name from before the rename to `mods.attend.continuity` where
    /// a layer still holds it, so every key arrives here under its
    /// continuity name.
    ///
    /// References:
    /// - ADR-417 item 1: the keys move to `mods.attend.continuity`, and the old
    ///   names are still read for a while.
    pub fn from_settings(doc: Option<&str>) -> Config {
        let d = Config::default();
        let Some(v) = doc.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else {
            return Config { settings_ok: false, enabled: false, ..d };
        };
        let key = |k: &str| v.get("keys").and_then(|keys| keys.get(format!("mods.attend.{k}"))).and_then(|e| e.get("value")).cloned();
        let text = |k: &str| key(k).and_then(|x| x.as_str().map(str::to_string));
        let int = |k: &str| key(k).and_then(|x| x.as_u64());
        let ranged = |k: &str, (lo, hi): (u32, u32), dflt: u32| int(k).filter(|n| (lo as u64..=hi as u64).contains(n)).map_or(dflt, |n| n as u32);
        let windows = match key("continuity.windows") {
            Some(serde_json::Value::Array(items)) => {
                Some(items.iter().map(|i| i.as_str().map_or_else(|| i.to_string(), str::to_string)).collect::<Vec<_>>())
            }
            _ => None,
        };
        let zone = text("continuity.timezone").unwrap_or_default();
        Config {
            settings_ok: true,
            keepwarm_on: text("keepwarm").as_deref() != Some("off"),
            enabled: key("continuity.keepalive").and_then(|x| x.as_bool()) == Some(true),
            max: ranged("continuity.max_keepalives", MAX_RANGE, d.max),
            margin_secs: int("continuity.margin_minutes").filter(|n| (MARGIN_RANGE.0..=MARGIN_RANGE.1).contains(n)).map_or(d.margin_secs, |n| n * 60),
            windows: match windows {
                Some(w) => Windows::parse(&w, &zone),
                None => Windows::parse(&[crate::windows::DEFAULT_WINDOW.to_string()], &zone),
            },
            wrap: text("continuity.wrap").and_then(|s| WrapDest::parse(&s)).unwrap_or(d.wrap),
            wrap_file: text("continuity.wrap_file").filter(|s| !s.trim().is_empty()).unwrap_or(d.wrap_file),
            wrap_to: text("continuity.wrap_to").unwrap_or_default().trim().to_string(),
            after_wrap: match text("continuity.after_wrap").as_deref() {
                Some("compact") => AfterWrap::Compact,
                _ => d.after_wrap,
            },
            after_max: ranged("continuity.after_wrap_keepalives", AFTER_RANGE, d.after_max),
            wrap_skill: text("continuity.wrap_skill").map_or(d.wrap_skill, |s| WrapSkill::parse(&s)),
            relay: text("continuity.relay").as_deref() == Some("on"),
            relay_at_pct: ranged("continuity.relay_at_pct", RELAY_AT_RANGE, d.relay_at_pct),
            max_relays: ranged("continuity.max_relays", MAX_RELAYS_RANGE, d.max_relays),
            session_skills: SessionSkills::NotChecked,
            fill: None,
        }
    }

    /// The fill at which relay mode wraps: `relay_at_pct` of the window,
    /// lowered with autocompact on to five points of the window under
    /// Claude Code's own threshold where that is lower. `None` without a
    /// fill or with relay off.
    pub fn relay_at_tokens(&self, gate: Gate) -> Option<u64> {
        let f = self.fill.filter(|f| self.relay && f.window > 0)?;
        let at = f.window * self.relay_at_pct as u64 / 100;
        Some(match (gate, f.native_at) {
            (Gate::On, Some(n)) if n > 0 => at.min(n.saturating_sub(f.window * NATIVE_MARGIN_PCT / 100)),
            _ => at,
        })
    }

    /// Why no wrap follows the keepalives, or `None` when one does: `wrap`
    /// is off, or `wrap_skill` names no skill the session has. Without a
    /// wrap there is no compaction either, and the policy stops at the bound.
    pub fn no_wrap(&self) -> Option<String> {
        if self.wrap == WrapDest::Off {
            return Some("wrap is off".into());
        }
        if let Some((value, _)) = &self.wrap_skill.refused {
            return Some(format!("wrap_skill \"{value}\" is refused"));
        }
        let name = &self.wrap_skill.name;
        if name.is_empty() {
            return Some("no wrap_skill is set".into());
        }
        match &self.session_skills {
            SessionSkills::Unread => Some("the session's skills could not be read".into()),
            SessionSkills::Listed(list) if !list.iter().any(|s| s == name) => Some(format!("the {name} skill is not among the session's skills")),
            SessionSkills::Listed(_) | SessionSkills::Partial(_) | SessionSkills::NotChecked => None,
        }
    }

    /// What a check with this config finds of `wrap_skill`; `None` where
    /// the host passed no listing.
    fn skill_seen(&self) -> Option<SkillSeen> {
        match &self.session_skills {
            SessionSkills::NotChecked => None,
            SessionSkills::Unread => Some(SkillSeen::Unread),
            SessionSkills::Listed(list) | SessionSkills::Partial(list) if list.contains(&self.wrap_skill.name) => Some(SkillSeen::Listed),
            SessionSkills::Listed(_) => Some(SkillSeen::Missing),
            SessionSkills::Partial(_) => None,
        }
    }

    /// This config as the last check, recorded in `p`, saw the host: its
    /// skill listing, and no stretch after the wrap for an older mod. For a
    /// reader such as the status card that has neither of its own.
    fn with_seen(&self, p: &Policy) -> Config {
        let mut c = self.for_mode();
        if p.old_host {
            c.after_max = 0;
        }
        if c.session_skills == SessionSkills::NotChecked {
            c.session_skills = match p.skill_seen {
                Some(SkillSeen::Unread) => SessionSkills::Unread,
                Some(SkillSeen::Missing) => SessionSkills::Listed(Vec::new()),
                Some(SkillSeen::Listed) | None => SessionSkills::NotChecked,
            };
        }
        c
    }

    /// This config with what the mode rules out: in relay mode no
    /// keepalives and no `after_wrap` follow the wrap, since a compaction
    /// and a resume do.
    ///
    /// References:
    /// - ADR-417 item 2c: in relay mode the idle wrap is followed by a
    ///   compaction and a resume.
    pub fn for_mode(&self) -> Config {
        let mut c = self.clone();
        if c.relay {
            c.after_max = 0;
            c.after_wrap = AfterWrap::Lapse;
        }
        c
    }

    /// Why the policy does not run with `gate`, or `None` when it runs.
    /// With relay on it runs whatever autocompact says: relay drives its own
    /// compaction, and its fill path runs even where `keepalive` turns the
    /// idle path off.
    ///
    /// References:
    /// - ADR-417 item 2: with relay on, the idle path's gate ignores autocompact.
    pub fn why_off(&self, gate: Gate) -> Option<&'static str> {
        if !self.settings_ok {
            Some("the mods.attend settings could not be read")
        } else if !self.keepwarm_on {
            Some("mods.attend.keepwarm is off")
        } else if self.relay {
            None
        } else if !self.enabled {
            Some("mods.attend.continuity.keepalive is false")
        } else {
            match gate {
                Gate::Off => None,
                Gate::On => Some("autocompact is on"),
                Gate::Unknown => Some("autocompact could not be read, so it counts as on"),
            }
        }
    }
}

/// Who and where this session is, for the wrap's file, its recipient and
/// its handoff line.
#[derive(Clone, Debug, Default)]
pub struct Site {
    pub session_id: String,
    /// The project directory (the session's origin path).
    pub project: String,
    /// The session's name, as peers see it.
    pub name: String,
    /// The operator's member id (ADR-170).
    pub operator: String,
    /// The ways state root, `{state}`.
    pub state_root: PathBuf,
    pub home: PathBuf,
    /// The repository's handoff, `.claude/handoff.wrap.md`, as the caller
    /// read it while a relay wrap waits to be judged.
    pub handoff: Option<Handoff>,
}

/// The handoff file as the caller read it (`ways_core::handoff`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Handoff {
    /// Absolute.
    pub path: PathBuf,
    /// The wrap id its first line carries.
    pub wrap_id: Option<String>,
    /// The compaction's instructions built from it.
    pub instructions: String,
    /// The file's text, whole: what a stopped relay sends the operator
    /// when the wrap's reply is gone.
    pub text: String,
}

impl Site {
    fn project_name(&self) -> String {
        std::path::Path::new(&self.project).file_name().map_or_else(|| self.project.clone(), |n| n.to_string_lossy().into_owned())
    }

    /// The default wrap file, `{state}/wrap/<session>.md`.
    pub fn default_wrap_file(&self) -> PathBuf {
        self.state_root.join("wrap").join(format!("{}.md", self.session_id))
    }

    /// `wrap_file` expanded ([`Site::wrap_file`]) and confined
    /// ([`check_wrap_path`]): a project's checked-in config can set it, and
    /// the reply is written wherever it points. A refused path falls back to
    /// the default file, checked the same way; when that fails too, the
    /// path is empty and nothing is written. The note says why.
    pub fn confined_wrap_file(&self, template: &str, windows: &Windows, now: u64) -> (String, Option<String>) {
        let default = self.default_wrap_file();
        let fallback = |why: String| match check_wrap_path(&self.state_root, &self.session_id, &default) {
            Ok(()) => {
                let note = format!("wrap_file \"{}\" refused: {why}; the reply goes to {}", echo(template), default.display());
                (default.to_string_lossy().into_owned(), Some(note))
            }
            Err(also) => (String::new(), Some(format!("wrap_file \"{}\" refused: {why}; the default {} is refused too: {also}", echo(template), default.display()))),
        };
        if !template.contains("{session}") {
            return fallback("it names no {session}, so sessions would overwrite one another's wrap".into());
        }
        let path = PathBuf::from(self.wrap_file(template, windows, now));
        match check_wrap_path(&self.state_root, &self.session_id, &path) {
            Ok(()) => (normalize(&path).unwrap_or(path).to_string_lossy().into_owned(), None),
            Err(why) => fallback(why),
        }
    }

    /// `wrap_file` with `{state}`, `{project}`, `{session}` and `{date}`
    /// expanded, a leading `~` read as home, and a relative path read from
    /// the project.
    pub fn wrap_file(&self, template: &str, windows: &Windows, now: u64) -> String {
        let text = template
            .replace("{state}", &self.state_root.to_string_lossy())
            .replace("{project}", &self.project_name())
            .replace("{session}", &self.session_id)
            .replace("{date}", &windows.date(now));
        let path = match text.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None => PathBuf::from(&text),
        };
        let path = if path.is_absolute() { path } else { PathBuf::from(&self.project).join(path) };
        path.to_string_lossy().into_owned()
    }
}

/// `path` with `.` and `..` resolved lexically; `None` when `..` climbs
/// above the root.
fn normalize(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    Some(out)
}

/// Whether `path` is a safe place for session `session_id`'s wrap reply:
/// under `{state}/wrap/` once `.` and `..` are resolved, its file name
/// naming the session, and no component from the wrap directory down a
/// symlink. Checked when the route is made and again when the reply is
/// written ([`write_wrap`]).
pub fn check_wrap_path(state_root: &Path, session_id: &str, path: &Path) -> Result<(), String> {
    use std::path::Component;
    let base = normalize(&state_root.join("wrap")).ok_or("the state root climbs above the root")?;
    // The path checked is the path written: a `..` resolved here would be
    // resolved by the system through whatever link sits before it.
    if path.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir)) {
        return Err("it has a . or .. component".into());
    }
    let rest = path.strip_prefix(&base).map_err(|_| "it is outside {state}/wrap/".to_string())?;
    if rest.as_os_str().is_empty() {
        return Err("it names the wrap directory itself".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if session_id.is_empty() || !name.contains(session_id) {
        return Err("its file name does not name the session".into());
    }
    let is_link = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    if is_link(&base) {
        return Err(format!("{} is a symlink", base.display()));
    }
    let mut at = base.clone();
    for part in rest.components() {
        at.push(part);
        if is_link(&at) {
            return Err(format!("{} is a symlink", at.display()));
        }
    }
    Ok(())
}

/// Writes `reply` to `path` for session `session_id`. The path is checked
/// again first ([`check_wrap_path`]), so a link present then is refused;
/// the write goes through a temp file created new beside it and renamed
/// over it, so a link planted at the path after the check is replaced, not
/// followed. A residual race remains: a directory on the path swapped for a
/// link between the last check and the rename is followed. Closing it needs
/// `openat` with `O_NOFOLLOW` down the path (ADR-414 item 6).
pub fn write_wrap(state_root: &Path, session_id: &str, path: &Path, reply: &str) -> Result<(), String> {
    check_wrap_path(state_root, session_id, path)?;
    let dir = path.parent().ok_or("it has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // A directory made just now could have been swapped for a link.
    check_wrap_path(state_root, session_id, path)?;
    replace_file(dir, path, reply)
}

/// Writes `reply` to a temp file created new in `dir` and renames it over
/// `path`: whatever sits at `path`, a link included, is replaced, never
/// written through.
fn replace_file(dir: &Path, path: &Path, reply: &str) -> Result<(), String> {
    use std::io::Write;
    // Unique per write, so a temp a crashed write left cannot fail this one.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp.{}.{nanos}", std::process::id()));
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| e.to_string())?;
    let done = f.write_all(reply.as_bytes()).and_then(|()| f.sync_all()).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = done {
        std::fs::remove_file(&tmp).ok();
        return Err(e.to_string());
    }
    Ok(())
}

/// Whether `to` names the `#open` channel, which reaches every listening
/// session on the host.
fn is_open_channel(to: &str) -> bool {
    to.strip_prefix('#').is_some_and(|c| c.trim().eq_ignore_ascii_case("open"))
}

/// What the configuration asks for that the route refuses, for the card.
pub fn route_notes(cfg: &Config, site: &Site, now: u64) -> Vec<String> {
    let mut notes = Vec::new();
    if let (_, Some(n)) = site.confined_wrap_file(&cfg.wrap_file, &cfg.windows, now) {
        notes.push(n);
    }
    if cfg.wrap == WrapDest::Send && is_open_channel(&cfg.wrap_to) {
        notes.push(format!("wrap_to \"{}\" refused: #open reaches every listening session; the reply goes to {}", cfg.wrap_to, site.operator));
    }
    if let Some((value, why)) = &cfg.wrap_skill.refused {
        notes.push(format!("wrap_skill \"{value}\" refused: {why}; no wrap or compaction runs"));
    }
    notes
}

/// `none`, `keepalive`, `wrap` or `compact`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    None,
    Keepalive,
    Wrap,
    /// Compact the conversation between turns; `text` is the focus, or in
    /// relay mode the handoff's instructions, and `resume` the turn to
    /// submit after it.
    Compact,
}

/// Where the wrap's reply goes, as the mod routes it (ADR-414 item 6).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WrapRoute {
    pub dest: &'static str,
    /// The expanded `wrap_file`: the destination of `file`, and the
    /// fallback of a refused `send`.
    pub file: String,
    /// The recipient of `send`: `wrap_to`, or the operator's member id
    /// when it is empty; `#name` is a channel. A relay stop sends the
    /// reply here whatever `dest` says.
    pub to: String,
    /// `lapse`, `compact`, or `relay`, where a compaction and a resume
    /// follow the wrap and the mod waits for attend's word.
    pub after: &'static str,
    /// Keepalives after the wrap before `after` applies. With none, the mod
    /// compacts once the wrap has answered; otherwise the policy asks for
    /// the compaction at a due point (`Action::Compact`).
    pub after_keepalives: u32,
    /// The line a `send` body opens with.
    pub handoff: String,
    /// What the route refused of the configuration (a `wrap_file` outside
    /// `{state}/wrap/`, `wrap_to` `#open`), and where the reply goes instead.
    pub notes: Vec<String>,
}

/// What `attend keepwarm check --policy --json` prints.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Decision {
    pub action: Action,
    /// The turn to submit, for `keepalive` and `wrap`; the compaction's
    /// focus, for `compact`.
    pub text: Option<String>,
    /// When to ask again, in epoch seconds; `None` when nothing is due
    /// before the next main-loop turn.
    pub next_at: Option<u64>,
    pub count: u32,
    pub max: u32,
    /// When the wrap goes out if nothing else happens.
    pub wraps_at: Option<u64>,
    pub reason: String,
    /// Whether the policy runs in this session; false is ADR-182's armed
    /// windows alone.
    pub policy: bool,
    /// `running`, `wrapped`, `compacted`, `stopped`, or `off` where the
    /// policy does not run.
    pub state: &'static str,
    pub wraps_at_hm: Option<String>,
    pub wrapped_at_hm: Option<String>,
    /// Keepalives sent since the wrap, of `after_max`.
    pub after_count: u32,
    pub after_max: u32,
    /// When the compaction after the wrap goes out if nothing else happens.
    pub compacts_at: Option<u64>,
    pub compacts_at_hm: Option<String>,
    pub compacted_at_hm: Option<String>,
    pub wrap: Option<WrapRoute>,
    /// The judgment could not be made (the transcript was unreadable): ask
    /// again at `next_at`, a bounded number of times.
    pub retry: bool,
    /// Why a running policy has no due point, in a few words for the mod's
    /// row: `under 50k`, `5m tier`, `no request yet`.
    pub idle: Option<&'static str>,
    /// Whether relay mode is on.
    pub relay: bool,
    /// Resumes since the operator's last prompt, of `max_relays`.
    pub relay_count: u32,
    pub max_relays: u32,
    /// The fill at which relay mode wraps, where the host gave a fill.
    pub relay_at_tokens: Option<u64>,
    /// With `compact`: the turn to submit once the compaction has run
    /// without a skip. `None` for a compaction that stops the relay.
    pub resume: Option<String>,
    /// Why the relay stopped, in a few words for the row.
    pub relay_stop: Option<String>,
    /// The mod sends the wrap's reply over attend to `wrap.to`, opening
    /// with `wrap.handoff`; set once, on the judgment that stops the relay.
    pub send_handoff: bool,
    /// The id of the wrap sent or judged.
    pub wrap_id: Option<String>,
}

impl Decision {
    /// The wrap's and the compaction's estimates, from the due point `due`
    /// of a stretch whose cache lives `ttl`.
    fn estimate(&mut self, cfg: &Config, p: &Policy, due: u64, ttl: u64) {
        if p.wrapped_at.is_none() {
            self.wraps_at = wraps_at(cfg, p.count, due, ttl);
            self.wraps_at_hm = self.wraps_at.map(|w| cfg.windows.hm(w));
        }
        self.compacts_at = compacts_at(cfg, p, due, ttl);
        self.compacts_at_hm = self.compacts_at.map(|c| cfg.windows.hm(c));
    }

    fn new(cfg: &Config, p: &Policy, policy: bool, state: &'static str, reason: impl Into<String>) -> Self {
        Decision {
            action: Action::None,
            text: None,
            next_at: None,
            count: p.count,
            max: cfg.max,
            wraps_at: None,
            reason: reason.into(),
            policy,
            state,
            wraps_at_hm: None,
            wrapped_at_hm: p.wrapped_at.map(|t| cfg.windows.hm(t)),
            after_count: p.after_count,
            after_max: cfg.after_max,
            compacts_at: None,
            compacts_at_hm: None,
            compacted_at_hm: p.compacted_at.map(|t| cfg.windows.hm(t)),
            wrap: None,
            retry: false,
            idle: None,
            relay: cfg.relay,
            relay_count: p.relays,
            max_relays: cfg.max_relays,
            relay_at_tokens: None,
            resume: None,
            relay_stop: p.relay_stop.clone(),
            send_handoff: false,
            wrap_id: p.wrap_id.clone(),
        }
    }
}

/// Why a stretch has no due point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Idle {
    NoRequest,
    FiveMinuteTier,
    SmallContext,
}

impl Idle {
    fn reason(self) -> &'static str {
        match self {
            Idle::NoRequest => "no request yet in this session",
            Idle::FiveMinuteTier => "the session is on the five-minute cache tier",
            Idle::SmallContext => "the context is under 50k tokens, where a cold re-write is cheap",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Idle::NoRequest => "no request yet",
            Idle::FiveMinuteTier => "5m tier",
            Idle::SmallContext => "under 50k",
        }
    }
}

/// The due point of the stretch `reading` ends with, that stretch's TTL and
/// its last request, or why there is none: no request yet, the five-minute
/// tier, a context under the floor.
fn due_point(state: &State, reading: &Reading, cfg: &Config) -> Result<(u64, u64, u64), Idle> {
    let last = match reading.usage_tail.last() {
        Some(l) if l.at_epoch > 0 => l,
        _ => return Err(Idle::NoRequest),
    };
    if last.ttl_secs() < TTL_SECS {
        return Err(Idle::FiveMinuteTier);
    }
    if state.ctx < BIG_TOKENS {
        return Err(Idle::SmallContext);
    }
    let ttl = last.ttl_secs();
    Ok((last.at_epoch + ttl.saturating_sub(cfg.margin_secs), ttl, last.at_epoch))
}

/// When the wrap goes out if nothing else happens: the first due point,
/// from `due` on, past the count or outside the windows. `None` with no
/// wrap.
fn wraps_at(cfg: &Config, count: u32, due: u64, ttl: u64) -> Option<u64> {
    if cfg.no_wrap().is_some() {
        return None;
    }
    let step = ttl.saturating_sub(cfg.margin_secs).max(60);
    let (mut t, mut c) = (due, count);
    while c < cfg.max && cfg.windows.contains(t) {
        c += 1;
        t += step;
    }
    Some(t)
}

/// When the compaction after the wrap goes out if nothing else happens:
/// the first due point after the wrap past `after_max` keepalives or
/// outside the windows, from `due` on. `None` where no compaction follows
/// at a due point.
fn compacts_at(cfg: &Config, p: &Policy, due: u64, ttl: u64) -> Option<u64> {
    if cfg.after_wrap != AfterWrap::Compact || cfg.after_max == 0 || p.compacted_at.is_some() {
        return None;
    }
    let step = ttl.saturating_sub(cfg.margin_secs).max(60);
    let (mut t, mut c) = match p.wrapped_at {
        Some(_) => (due, p.after_count),
        // The wrap's turn restarts the clock: the first keepalive after it
        // is a step on.
        None => (wraps_at(cfg, p.count, due, ttl)? + step, 0),
    };
    while c < cfg.after_max && cfg.windows.contains(t) {
        c += 1;
        t += step;
    }
    Some(t)
}

/// Why nothing more follows the wrap, or `None` while keepalives or the
/// compaction after it are still to come. `None` too before the wrap.
fn after_wrap_ends(cfg: &Config, p: &Policy) -> Option<String> {
    let wrapped = format!("wrapped at {}", cfg.windows.hm(p.wrapped_at?));
    if let Some(c) = p.compacted_at {
        return Some(match &p.compact_failed {
            Some(why) => format!("{wrapped}; the compaction at {} failed: {why}", cfg.windows.hm(c)),
            None => format!("{wrapped} and compacted at {}", cfg.windows.hm(c)),
        });
    }
    if let Some(why) = &p.stopped {
        return Some(format!("{wrapped}, then stopped: {why}"));
    }
    if cfg.after_max == 0 {
        return Some(wrapped);
    }
    if p.after_count < cfg.after_max || (cfg.after_wrap == AfterWrap::Compact && p.compact_focus.is_some()) {
        return None;
    }
    let tail = if cfg.after_wrap == AfterWrap::Compact { "; the wrap left no reply to compact with" } else { "" };
    Some(format!("{wrapped} and {} after it{tail}", keepalives(p.after_count)))
}

/// `1 keepalive`, `3 keepalives`.
fn keepalives(n: u32) -> String {
    format!("{n} {}", noun(n))
}

fn noun(n: u32) -> &'static str {
    if n == 1 { "keepalive" } else { "keepalives" }
}

/// The wrap turn's last sentence: what the mod does once it has answered.
fn after_wrap_line(cfg: &Config) -> String {
    match (cfg.after_max, cfg.after_wrap) {
        (0, AfterWrap::Lapse) => "Nothing keeps the prompt cache warm after this turn.".into(),
        (0, AfterWrap::Compact) => "Once you answer, the mod compacts the conversation with the focus of your closing /compact line.".into(),
        (m, AfterWrap::Lapse) => format!("The mod keeps the prompt cache warm for up to {m} more {}, then lets it lapse.", noun(m)),
        (m, AfterWrap::Compact) => format!(
            "The mod keeps the prompt cache warm for up to {m} more {}, then compacts the conversation with the focus of your closing /compact line.",
            noun(m),
        ),
    }
}

/// Why a wrap goes out: the first line of its turn.
#[derive(Clone, Copy, Debug)]
enum Cause {
    /// The keepalive count is spent.
    Bound,
    /// Outside the keepalive hours.
    Hours,
    /// The context reached relay's threshold.
    ///
    /// References:
    /// - ADR-417 item 2a: a context fill past the threshold starts a wrap.
    Fill { tokens: u64, window: u64, at: u64 },
}

/// `760k`, `1M`, `1.5M`.
fn tokens(n: u64) -> String {
    fmt_tok(n).replace(".0M", "M")
}

/// The wrap turn's text: its first line says why the wrap goes out.
///
/// References:
/// - ADR-414 item 6: the wrap turn the policy submits.
/// - ADR-417 item 2: the fill threshold as one more reason for a wrap.
fn wrap_text(cfg: &Config, site: &Site, p: &Policy, route: &WrapRoute, cause: Cause, now: u64) -> String {
    let since = p.since().map_or_else(|| "a long idle stretch".to_string(), |t| format!("{} since the operator's last prompt", fmt_duration(now.saturating_sub(t))));
    let first = match cause {
        Cause::Bound => format!("wrap: {since} and {} keepalives, the most this session allows.", p.count),
        Cause::Hours if cfg.windows.list.is_empty() => format!("wrap: {since}, and no keepalive hours are set."),
        Cause::Hours => format!("wrap: {since}, and the keepalive hours ({}) have ended.", cfg.windows.describe()),
        Cause::Fill { tokens: t, window, at } => format!(
            "wrap: context at {}% ({} of {}), past the relay threshold of {}%.",
            t * 100 / window.max(1),
            tokens(t),
            tokens(window),
            at * 100 / window.max(1),
        ),
    };
    let after = match &p.wrap_id {
        Some(id) if p.relay => format!(
            "This wrap is in relay mode with wrap id {id}: write wrap={id} on the handoff's first line and end your reply with the relay marker. Once you answer, the mod compacts the conversation with the handoff and starts a turn that resumes the work."
        ),
        _ => after_wrap_line(cfg),
    };
    let mut body = format!(
        "The operator is away. {}: square the task list and write the continuation prompt. Ask no questions, land nothing and start no new work. {after}",
        cfg.wrap_skill.ask(),
    );
    match cfg.wrap {
        // A relay keeps the reply in the transcript; a stop sends it.
        _ if p.relay => {}
        WrapDest::File => body.push_str(&format!(" The mod saves your final reply to {}.", route.file)),
        WrapDest::Send if route.to == site.operator => {
            body.push_str(&format!(" The mod sends your final reply to your operator, {}, over attend.", route.to));
        }
        WrapDest::Send if route.to.starts_with('#') => {
            body.push_str(&format!(" The mod posts your final reply in {} over attend, where another session may take up the work.", route.to));
        }
        WrapDest::Send => body.push_str(&format!(" The mod sends your final reply to {} over attend as a handoff.", route.to)),
        WrapDest::Transcript | WrapDest::Off => {}
    }
    format!("{first}\n{body}")
}

fn route(cfg: &Config, site: &Site, now: u64) -> WrapRoute {
    let to = if cfg.wrap_to.is_empty() || is_open_channel(&cfg.wrap_to) { site.operator.clone() } else { cfg.wrap_to.clone() };
    WrapRoute {
        dest: cfg.wrap.as_str(),
        file: site.confined_wrap_file(&cfg.wrap_file, &cfg.windows, now).0,
        notes: route_notes(cfg, site, now),
        to,
        after: if cfg.relay { "relay" } else { cfg.after_wrap.as_str() },
        after_keepalives: cfg.after_max,
        handoff: format!(
            "handoff: continuation prompt from {} ({}), wrapped at {} while its operator was away.",
            site.name,
            site.project_name(),
            cfg.windows.hm(now),
        ),
    }
}

/// A wrap id for a wrap the policy sends: the UTC time to the second and
/// the session id's first eight characters, as `20261126T214012Z-3f9a1c2e`.
///
/// References:
/// - ADR-417 item 8: each wrap carries an id unique to it.
fn issue_wrap_id(site: &Site, now: u64) -> String {
    let at = jiff::Timestamp::from_second(now as i64).map_or_else(|_| now.to_string(), |t| t.strftime("%Y%m%dT%H%M%SZ").to_string());
    let sid: String = site.session_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    format!("{at}-{}", if sid.is_empty() { "session" } else { &sid })
}

/// The resume turn's text, for relay `n` of `max`.
///
/// References:
/// - ADR-417 item 4: the exact text of the resume.
fn resume_text(handoff: &Path, n: u32, max: u32) -> String {
    format!(
        "relay: the conversation was compacted after a wrap; this is relay {n} of {max} since the operator's last prompt. Read {}. The task list survived the compaction; work from it and the handoff's TASKS, then start its DO NEXT. The operator is away: ask no questions. If a step needs a decision only the operator can make, or your doubt about the direction is high, do not start it; end your reply with one line, relay: stop decision or relay: stop doubt, followed by the reason, and the mod sends the handoff to the operator over attend.",
        handoff.display(),
    )
}

/// Sends the wrap: records it, with its id and mode, and returns the
/// decision that submits its turn.
fn send_wrap(state: &mut State, cfg: &Config, site: &Site, cause: Cause, reason: &str, now: u64) -> Decision {
    let p = &mut state.policy;
    p.wrapped_at = Some(now);
    if cfg.relay {
        let id = issue_wrap_id(site, now);
        p.relay = true;
        p.trigger = Some(if matches!(cause, Cause::Fill { .. }) { "fill" } else { "idle" }.into());
        p.wrap_id = Some(id.clone());
        p.last_wrap_id = Some(id);
    }
    let p = state.policy.clone();
    let r = route(cfg, site, now);
    let mut d = Decision::new(cfg, &p, true, "wrapped", reason);
    d.action = Action::Wrap;
    d.text = Some(wrap_text(cfg, site, &p, &r, cause, now));
    d.wrap = Some(r);
    d
}

/// The relay's judgment of a wrap that ran in relay mode, once the mod
/// reported its answer: a compaction and a resume, or a stop. `None` where
/// no relay wrap waits, or where a `/wrap` came while relay is off, which is
/// cleared.
///
/// References:
/// - ADR-417 items 4 and 5: the compaction and resume, and the stops that
///   end the relay.
fn judge_relay(state: &mut State, cfg: &Config, site: &Site, now: u64) -> Option<Decision> {
    // A stop a report recorded (a resumed turn's stop line, a compaction or
    // a resume that failed, an unfinished wrap) hands the reply over once.
    if state.policy.relay_stop.is_some() && !state.policy.handed {
        return Some(hand(state, cfg, site, false, now));
    }
    let p = &state.policy;
    if !p.relay || p.wrapped_at.is_none() {
        return None;
    }
    if p.stopped.is_some() {
        let mut d = Decision::new(cfg, p, true, "stopped", format!("stopped: {}; nothing more until the operator's next prompt", p.stopped.as_deref().unwrap_or_default()));
        d.wrap = Some(route(cfg, site, now));
        // A stop's compaction the mod has not run yet (a turn came first)
        // is asked for again, while the handoff is still the wrap's.
        let fresh = site.handoff.as_ref().filter(|h| h.wrap_id.is_some() && h.wrap_id == p.wrap_id);
        if let (true, None, Some(h)) = (p.compact_due, p.compacted_at, fresh) {
            d.reason = format!("{}; the mod compacts with the handoff", d.reason);
            d.action = Action::Compact;
            d.text = Some(h.instructions.clone());
        }
        return Some(d);
    }
    // Compacted, and the resume never reported: the mod that compacted is
    // gone, since a live one reports the resume before it lets another
    // check run. Whether its resume went in cannot be told, so the relay
    // stops and the operator gets the handoff, rather than a second
    // generation starting on a guess.
    //
    // References:
    // - ADR-417 item 4: a resume that does not go in stops the relay.
    if p.compacted_at.is_some() {
        let text = site.handoff.as_ref().filter(|h| h.wrap_id.is_some() && h.wrap_id == p.wrap_id).map(|h| h.text.clone());
        let why = "the conversation was compacted after the wrap and the resume never went in (the mod reloaded or the session restarted)".to_string();
        let mut d = stop(state, cfg, site, "resume lost", why, false, now);
        d.text = text;
        return Some(d);
    }
    // A `/wrap` whose skill saw relay on, judged once relay is off: the
    // wrap stands as a handoff-mode one, and its id is the session's last.
    if p.trigger.as_deref() == Some("wrap") && !cfg.relay {
        let p = &mut state.policy;
        if let Some(m) = p.marker.as_deref().and_then(Marker::parse) {
            p.last_wrap_id = Some(m.wrap);
        }
        p.relay = false;
        p.marker = None;
        p.trigger = None;
        return None;
    }
    let Some(line) = p.marker.clone() else {
        if p.wrap_outcome.is_some() {
            return Some(stop(state, cfg, site, "no marker", "the wrap ended with no relay marker".into(), false, now));
        }
        return Some(Decision::new(cfg, p, true, "wrapped", "waiting for the wrap's answer"));
    };
    let Some(m) = Marker::parse(&line) else {
        return Some(stop(state, cfg, site, "no marker", "the wrap's relay marker does not parse".into(), false, now));
    };
    // A `/wrap`'s marker that repeats the last wrap's id wrote no new
    // handoff: a resumed turn that ends with the previous wrap's marker.
    if p.wrap_id.is_none() && p.last_wrap_id.as_deref() == Some(m.wrap.as_str()) {
        let why = format!("the marker repeats wrap {}, which an earlier wrap already used", m.wrap);
        return Some(stop(state, cfg, site, "stale handoff", why, false, now));
    }
    if let Some(id) = p.wrap_id.as_deref().filter(|id| *id != m.wrap) {
        let why = format!("the marker names wrap {}, not this wrap's {id}", m.wrap);
        return Some(stop(state, cfg, site, "stale handoff", why, false, now));
    }
    {
        let p = &mut state.policy;
        p.wrap_id = Some(m.wrap.clone());
        p.last_wrap_id = Some(m.wrap.clone());
    }
    let Some(handoff) = site.handoff.clone().filter(|h| h.wrap_id.as_deref() == Some(m.wrap.as_str())) else {
        let why = format!("the handoff's first line does not carry wrap={}", m.wrap);
        return Some(stop(state, cfg, site, "stale handoff", why, false, now));
    };
    let goal_ended = state.policy.goal.as_deref() == Some("active") && !m.goal_active;
    state.policy.goal = Some(if m.goal_active { "active" } else { "none" }.into());
    let relays = state.policy.relays;
    let ended = if goal_ended {
        Some(("goal ended".to_string(), "the /goal that was active has ended".to_string()))
    } else if let Some(r) = m.stop {
        let what = match r {
            "empty" => "the task list holds no open work",
            "decision" => "the next step needs a decision only the operator can make",
            _ => "its doubt about the direction is high",
        };
        Some((r.to_string(), format!("the wrap reported stop {r}: {what}")))
    } else if relays >= cfg.max_relays {
        Some((format!("{relays} of {}", cfg.max_relays), format!("{relays} relays since the operator's last prompt, the most this session allows")))
    } else {
        None
    };
    // The compaction is recorded once the mod reports it ran ([`compacted`]):
    // a turn that starts first defers it, and the next check asks again.
    if let Some((short, why)) = ended {
        state.policy.compact_due = true;
        let mut d = stop(state, cfg, site, &short, why, true, now);
        d.action = Action::Compact;
        d.text = Some(handoff.instructions);
        return Some(d);
    }
    let p = state.policy.clone();
    let mut d = Decision::new(cfg, &p, true, "compacted", format!("compacting with the handoff, then relay {} of {}", relays + 1, cfg.max_relays));
    d.wrap = Some(route(cfg, site, now));
    d.action = Action::Compact;
    d.text = Some(handoff.instructions);
    d.resume = Some(resume_text(&handoff.path, relays + 1, cfg.max_relays));
    Some(d)
}

/// The relay stops: recorded, and the decision that hands the wrap's reply
/// to the mod to send, once. `compacts` says the mod compacts first.
fn stop(state: &mut State, cfg: &Config, site: &Site, short: &str, why: String, compacts: bool, now: u64) -> Decision {
    stop_relay(&mut state.policy, short, why);
    hand(state, cfg, site, compacts, now)
}

/// The decision for a stopped relay, which hands the wrap's reply to the
/// mod to send the first time it is given. `compacts` says the mod
/// compacts first.
fn hand(state: &mut State, cfg: &Config, site: &Site, compacts: bool, now: u64) -> Decision {
    let send = !state.policy.handed;
    state.policy.handed = true;
    let p = state.policy.clone();
    let tail = if compacts { "; the mod compacts with the handoff and sends it to the operator" } else { "; the mod sends the wrap's reply to the operator" };
    let mut d = Decision::new(cfg, &p, true, "stopped", format!("the relay stopped: {}{tail}", p.stopped.as_deref().unwrap_or_default()));
    let mut r = route(cfg, site, now);
    r.handoff = format!("{} The relay stopped: {}.", r.handoff, p.stopped.as_deref().unwrap_or_default());
    d.wrap = Some(r);
    d.send_handoff = send;
    d
}

/// One judgment (ADR-414 item 7). Records `gate`; where the policy does not
/// run it is the plain check's judgment (ADR-182 as ADR-406 item 13 has
/// it). The returned outcome is settled by the caller as the plain check's
/// is.
pub fn step(state: &mut State, reading: &Reading, arm: Option<&ArmFile>, cfg: &Config, gate: Gate, site: &Site, now: u64) -> (Outcome, Decision) {
    let (mut out, mut d) = step_mode(state, reading, arm, &cfg.for_mode(), gate, site, now);
    d.relay_at_tokens = cfg.relay_at_tokens(gate);
    out.changed |= d.action != Action::None;
    (out, d)
}

fn step_mode(state: &mut State, reading: &Reading, arm: Option<&ArmFile>, cfg: &Config, gate: Gate, site: &Site, now: u64) -> (Outcome, Decision) {
    state.policy.gate = Some(gate.as_str().into());
    state.policy.skill_seen = cfg.skill_seen();
    if let Some(why) = cfg.why_off(gate) {
        let mut d = Decision::new(cfg, &state.policy, false, "off", why);
        if !cfg.keepwarm_on {
            return (Outcome { changed: true, ..Outcome::default() }, d);
        }
        let out = state.step(reading, arm, now);
        if let Some((_, line)) = out.observations.first() {
            d.action = Action::Keepalive;
            d.text = Some(line.clone());
            d.reason = format!("{why}; an armed window's wake");
        }
        d.next_at = Some(now + PLAIN_EVERY_SECS);
        return (out, d);
    }

    // The sensor's own arm after a paid cold write is no word of the
    // operator's: the policy reads it as no window, so it cannot carry
    // uncounted wakes past the count. The cursor forgets it the same way,
    // so the step sees no change of the arm file.
    let arm = match arm {
        Some(a) if a.auto => {
            if state.deadline == a.deadline {
                state.deadline = 0;
            }
            None
        }
        other => other,
    };
    let had_ping = state.pinged_at.is_some();
    let ping_before = state.last_ping.clone();
    let resumed = state.baselined;
    let out = state.step_with(reading, arm, now, false);
    // A wake's verdict came in, or the margin ran out without its turn. A
    // first step only baselines: a wake it finds answered has no verdict.
    if !resumed {
        state.policy.pending = false;
    } else if had_ping && state.pinged_at.is_none() {
        let landed = state.last_ping != ping_before;
        let warm = landed && state.last_ping.as_ref().is_some_and(|p| p.warm);
        if state.policy.pending && landed {
            state.policy.spent_usd += state.last_ping.as_ref().and_then(|p| p.usd).unwrap_or(0.0);
        }
        state.policy.pending = false;
        if !warm {
            state.policy.stopped = Some(state.stopped.clone().unwrap_or_else(|| "the wake's turn read a cold cache".into()));
        }
    }
    // The resume's turn has made a request: the generation runs.
    if state.policy.resumed_at.is_some_and(|t| reading.usage_tail.last().is_some_and(|l| l.at_epoch >= t)) {
        state.policy.resumed_at = None;
    }
    // A relay wrap waiting to be judged, compacted, or stopped.
    if let Some(d) = judge_relay(state, cfg, site, now) {
        return (out, d);
    }
    // Relay's fill path: one wrap per generation, once the context reaches
    // the threshold.
    //
    // References:
    // - ADR-417 item 2a: a context fill past the threshold starts a wrap.
    let fill_at = cfg.relay_at_tokens(gate);
    if let (Some(at), Some(f)) = (fill_at, cfg.fill) {
        let p = &state.policy;
        if f.tokens >= at && p.wrapped_at.is_none() && p.stopped.is_none() && state.pinged_at.is_none() && cfg.no_wrap().is_none() {
            let mut out = out;
            out.changed = true;
            let d = send_wrap(state, cfg, site, Cause::Fill { tokens: f.tokens, window: f.window, at }, "the context reached the relay threshold", now);
            return (out, d);
        }
    }
    let p = state.policy.clone();

    // An armed window is the operator's word: ADR-182's wake, uncounted.
    if let Some((_, line)) = out.observations.first() {
        let mut d = Decision::new(cfg, &p, true, "running", "an armed window (attend keepwarm on) overrides the count");
        d.action = Action::Keepalive;
        d.text = Some(line.clone());
        d.next_at = Some(now + VERDICT_SECS + 1);
        return (out, d);
    }
    if state.deadline > now {
        let mut d = Decision::new(cfg, &p, true, "running", format!("an armed window (attend keepwarm on) overrides the count for {}", fmt_duration(state.deadline - now)));
        d.next_at = Some(match state.pinged_at {
            Some(t) => t + VERDICT_SECS + 1,
            None => (state.last_seen_at + PING_AFTER_SECS).max(now + 1).min(state.deadline),
        });
        return (out, d);
    }
    if let Some(why) = after_wrap_ends(cfg, &p) {
        let state = if p.compacted_at.is_some() && p.compact_failed.is_none() { "compacted" } else { "wrapped" };
        return (out, Decision::new(cfg, &p, true, state, format!("{why}; nothing more until the operator's next prompt")));
    }
    // Wrapped, with keepalives or the compaction after it still to come.
    let after = if p.wrapped_at.is_some() { "wrapped" } else { "running" };
    if let Some(why) = &p.stopped {
        return (out, Decision::new(cfg, &p, true, "stopped", format!("stopped: {why}; nothing more until the operator's next prompt")));
    }
    if !cfg.enabled {
        let state = if p.resumed_at.is_some() { "resumed" } else { "running" };
        return (out, Decision::new(cfg, &p, true, state, "mods.attend.continuity.keepalive is false, so only relay's fill path runs"));
    }
    if let Some(t) = state.pinged_at {
        let mut d = Decision::new(cfg, &p, true, after, "waiting for the keepalive's turn");
        d.next_at = Some(t + VERDICT_SECS + 1);
        if let Ok((_, ttl, _)) = due_point(state, reading, cfg) {
            d.estimate(cfg, &p, t + ttl.saturating_sub(cfg.margin_secs), ttl);
        }
        return (out, d);
    }
    let (due, ttl, last_at) = match due_point(state, reading, cfg) {
        Ok(x) => x,
        Err(why) => {
            let mut d = Decision::new(cfg, &p, true, after, why.reason());
            d.idle = Some(why.short());
            return (out, d);
        }
    };
    if now < due {
        let after = if after == "running" && p.resumed_at.is_some() { "resumed" } else { after };
        let mut d = Decision::new(cfg, &p, true, after, format!("next due at {}", cfg.windows.hm(due)));
        d.next_at = Some(due);
        d.estimate(cfg, &p, due, ttl);
        return (out, d);
    }
    let mut out = out;
    out.changed = true;
    if cache_lapsed(Some(reading), now) {
        let why = format!("the cache had lapsed at the due point {} (the host may have slept)", cfg.windows.hm(due));
        state.policy.stopped = Some(why);
        let p = state.policy.clone();
        let reason = format!("stopped: {}; nothing more until the operator's next prompt", p.stopped.as_deref().unwrap_or_default());
        let state = if p.wrapped_at.is_some() { "wrapped" } else { "stopped" };
        return (out, Decision::new(cfg, &p, true, state, reason));
    }
    if state.policy.wrapped_at.is_some() {
        return (out, after_wrap_due(state, cfg, ttl, last_at, now));
    }

    // A due point.
    if state.policy.since().is_none() {
        state.policy.stretch_from = Some(last_at);
    }
    let at_bound = state.policy.count >= cfg.max;
    if !at_bound && cfg.windows.contains(now) {
        state.policy.count += 1;
        state.policy.pending = true;
        state.pinged_at = Some(now);
        let p = state.policy.clone();
        let mut d = Decision::new(cfg, &p, true, "running", format!("keepalive {} of {}", p.count, cfg.max));
        d.action = Action::Keepalive;
        d.text = Some(wake_line(now.saturating_sub(last_at), ttl, state.ctx));
        d.next_at = Some(now + VERDICT_SECS + 1);
        d.estimate(cfg, &p, now + ttl.saturating_sub(cfg.margin_secs), ttl);
        return (out, d);
    }
    if let Some(why) = cfg.no_wrap() {
        state.policy.stopped = Some(if at_bound {
            format!("{} keepalives, the most this session allows, and {why}", state.policy.count)
        } else {
            format!("the keepalive hours have ended, and {why}")
        });
        let p = state.policy.clone();
        let reason = format!("stopped: {}; nothing more until the operator's next prompt", p.stopped.as_deref().unwrap_or_default());
        return (out, Decision::new(cfg, &p, true, "stopped", reason));
    }
    let (cause, reason) = if at_bound { (Cause::Bound, "the count is spent") } else { (Cause::Hours, "outside the keepalive hours") };
    let mut d = send_wrap(state, cfg, site, cause, reason, now);
    // The wrap's turn restarts the clock; with keepalives after it, ask
    // again when the first of them could be due.
    if cfg.after_max > 0 {
        let p = state.policy.clone();
        d.next_at = Some(now + ttl.saturating_sub(cfg.margin_secs));
        d.estimate(cfg, &p, now + ttl.saturating_sub(cfg.margin_secs), ttl);
    }
    (out, d)
}

/// A due point after the wrap, while [`after_wrap_ends`] says more is to
/// come: a keepalive while `after_max` allows and the windows hold, then the
/// compaction with the wrap's focus, else a stop.
fn after_wrap_due(state: &mut State, cfg: &Config, ttl: u64, last_at: u64, now: u64) -> Decision {
    let inside = cfg.windows.contains(now);
    if state.policy.after_count < cfg.after_max && inside {
        state.policy.after_count += 1;
        state.policy.pending = true;
        state.pinged_at = Some(now);
        let p = state.policy.clone();
        let mut d = Decision::new(cfg, &p, true, "wrapped", format!("keepalive {} of {} after the wrap", p.after_count, cfg.after_max));
        d.action = Action::Keepalive;
        d.text = Some(wake_line(now.saturating_sub(last_at), ttl, state.ctx));
        d.next_at = Some(now + VERDICT_SECS + 1);
        d.estimate(cfg, &p, now + ttl.saturating_sub(cfg.margin_secs), ttl);
        return d;
    }
    if let (AfterWrap::Compact, Some(focus)) = (cfg.after_wrap, state.policy.compact_focus.clone()) {
        state.policy.compacted_at = Some(now);
        let p = state.policy.clone();
        let why = if inside { format!("{} after the wrap", keepalives(p.after_count)) } else { "the keepalive hours have ended".into() };
        let mut d = Decision::new(cfg, &p, true, "compacted", format!("compacting with the wrap's focus: {why}"));
        d.action = Action::Compact;
        d.text = Some(focus);
        return d;
    }
    state.policy.stopped = Some(if cfg.after_wrap == AfterWrap::Compact {
        "the wrap left no reply to compact with".into()
    } else {
        "the keepalive hours have ended".into()
    });
    let p = state.policy.clone();
    let reason = format!("{}; nothing more until the operator's next prompt", after_wrap_ends(cfg, &p).unwrap_or_default());
    Decision::new(cfg, &p, true, "wrapped", reason)
}

/// Where the mod reports the wrap's reply went (`attend keepwarm wrapped`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapOutcome {
    Transcript,
    File,
    Sent,
    Refused,
    Unfinished,
}

impl WrapOutcome {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "transcript" => WrapOutcome::Transcript,
            "file" => WrapOutcome::File,
            "sent" => WrapOutcome::Sent,
            "refused" => WrapOutcome::Refused,
            "unfinished" => WrapOutcome::Unfinished,
            _ => return None,
        })
    }

    /// The card's words for it, with the mod's detail (a path, a target, a
    /// refusal).
    pub fn describe(self, detail: &str) -> String {
        let d = detail.trim();
        match self {
            WrapOutcome::Transcript => "the reply is in the transcript".into(),
            WrapOutcome::File => format!("the reply is saved to {d}"),
            WrapOutcome::Sent => format!("the reply was sent to {d}"),
            WrapOutcome::Refused => format!("handoff refused: {d}"),
            WrapOutcome::Unfinished if d.is_empty() => "the wrap did not finish".into(),
            WrapOutcome::Unfinished => format!("the wrap did not finish ({d})"),
        }
    }
}

/// The status card's keepalive and cost lines (ADR-414 item 9), for a
/// session whose ledger is `state` and whose transcript reads `reading`.
pub fn card(state: &State, reading: &Reading, cfg: &Config, site: &Site, now: u64) -> Vec<String> {
    let p = &state.policy;
    let cfg = &cfg.with_seen(p);
    let Some(gate) = p.gate.as_deref() else {
        return vec!["keepalive   not run in this session (the attend mod runs it in an interactive session while autocompact is off)".into()];
    };
    if let Some(why) = cfg.why_off(Gate::parse(gate)) {
        let tail = if cfg.keepwarm_on { " (armed windows only)" } else { "" };
        return vec![format!("keepalive   off: {why}{tail}")];
    }
    let mut lines = vec![if p.wrapped_at.is_some() {
        wrapped_line(state, reading, cfg, now)
    } else if let Some(why) = &p.stopped {
        format!("keepalive   stopped: {why}; your next prompt starts it again")
    } else if !cfg.enabled {
        "keepalive   off: mods.attend.continuity.keepalive is false; relay's fill path runs".to_string()
    } else {
        running_line(state, reading, cfg, now)
    }];
    for (entry, why) in &cfg.windows.dropped {
        lines.push(format!("keepalive   window entry \"{}\" dropped: {why}", echo(entry)));
    }
    if let Some(z) = &cfg.windows.bad_zone {
        lines.push(format!("keepalive   timezone \"{}\" is unknown; the host's zone is used", echo(z)));
    }
    if cfg.wrap != WrapDest::Off {
        for n in route_notes(cfg, site, now) {
            lines.push(format!("keepalive   {n}"));
        }
        if let (None, Some(why)) = (p.wrapped_at, cfg.no_wrap()) {
            lines.push(format!("keepalive   no wrap or compaction: {why}; the keepalives stop at the bound"));
        }
    }
    lines.push(cost_line(state, cfg));
    lines.push(relay_line(p, cfg));
    lines
}

/// The card's relay line: the mode, the count against the cap, the
/// threshold, and where the relay stands or why it stopped.
fn relay_line(p: &Policy, cfg: &Config) -> String {
    if !cfg.relay {
        return "relay       off: a wrap ends with a /compact line for the operator (handoff mode)".into();
    }
    if let (Some(_), Some(why)) = (&p.relay_stop, &p.stopped) {
        return format!("relay       stopped: {why}; your next prompt starts it again");
    }
    let mut line = format!("relay       on · {} of {}{} · wraps at {}% of the window", p.relays, cfg.max_relays, since_text(p, cfg), cfg.relay_at_pct);
    if p.relay && p.compacted_at.is_some() {
        line.push_str(" · compacted, waiting for the resume");
    } else if let (true, Some(t)) = (p.relay, p.wrapped_at) {
        line.push_str(&format!(" · wrapped at {}, waiting for its answer", cfg.windows.hm(t)));
    } else if p.resumed_at.is_some() {
        line.push_str(" · the resume's turn has not started yet");
    }
    line
}

/// ` since your prompt at 14:02`, ` since 14:02`, or nothing.
fn since_text(p: &Policy, cfg: &Config) -> String {
    match (p.operator_at, p.stretch_from) {
        (Some(t), _) => format!(" since your prompt at {}", cfg.windows.hm(t)),
        (None, Some(t)) => format!(" since {}", cfg.windows.hm(t)),
        (None, None) => String::new(),
    }
}

/// The due point the next keepalive or compaction waits for, with its TTL:
/// a sent keepalive's own when one waits for its turn.
fn next_due(state: &State, reading: &Reading, cfg: &Config) -> Option<(u64, u64)> {
    due_point(state, reading, cfg).ok().map(|(due, ttl, _)| match state.pinged_at {
        Some(t) => (t + ttl.saturating_sub(cfg.margin_secs), ttl),
        None => (due, ttl),
    })
}

/// ` · waiting for the keepalive's turn` or ` · next in 41m`.
fn next_text(state: &State, due: u64, now: u64) -> String {
    match state.pinged_at {
        Some(_) => " · waiting for the keepalive's turn".into(),
        None => format!(" · next in {}", fmt_duration(due.saturating_sub(now))),
    }
}

/// The card's keepalive line while the policy runs before the wrap.
fn running_line(state: &State, reading: &Reading, cfg: &Config, now: u64) -> String {
    let p = &state.policy;
    let mut line = format!("keepalive   {} of {}{}", p.count, cfg.max, since_text(p, cfg));
    if state.deadline > now {
        line.push_str(&format!(" · an armed window overrides it for {}", fmt_duration(state.deadline - now)));
    }
    match (due_point(state, reading, cfg), next_due(state, reading, cfg)) {
        (Err(why), _) => line.push_str(&format!(" · idle: {}", why.reason())),
        (Ok(_), Some((due, ttl))) => {
            line.push_str(&next_text(state, due, now));
            if let Some(w) = wraps_at(cfg, p.count, due, ttl) {
                line.push_str(&format!(" · wraps about {}", cfg.windows.hm(w)));
            }
            if let Some(c) = compacts_at(cfg, p, due, ttl) {
                line.push_str(&format!(" · compacts about {}", cfg.windows.hm(c)));
            }
        }
        (Ok(_), None) => {}
    }
    line.push_str(&format!(" · windows {}", cfg.windows.describe()));
    line
}

/// The card's keepalive line after the wrap: where its reply went, then
/// the compaction, or the keepalives after the wrap and what is next.
fn wrapped_line(state: &State, reading: &Reading, cfg: &Config, now: u64) -> String {
    let p = &state.policy;
    let outcome = p.wrap_outcome.clone().unwrap_or_else(|| "no answer recorded yet".into());
    let at = cfg.windows.hm(p.wrapped_at.unwrap_or_default());
    let mut line = format!("keepalive   wrapped at {at} after {} keepalives{} · {outcome}", p.count, since_text(p, cfg));
    if let Some(c) = p.compacted_at {
        match &p.compact_failed {
            Some(why) => line.push_str(&format!(" · the compaction at {} failed: {why}", cfg.windows.hm(c))),
            None => line.push_str(&format!(" · compacted at {}", cfg.windows.hm(c))),
        }
        return line;
    }
    if cfg.after_max == 0 {
        return line;
    }
    line.push_str(&format!(" · {} of {} after it", p.after_count, cfg.after_max));
    if let Some(why) = &p.stopped {
        line.push_str(&format!(" · stopped: {why}"));
    } else if after_wrap_ends(cfg, p).is_none() {
        if let Some((due, ttl)) = next_due(state, reading, cfg) {
            line.push_str(&next_text(state, due, now));
            if let Some(c) = compacts_at(cfg, p, due, ttl) {
                line.push_str(&format!(" · compacts about {}", cfg.windows.hm(c)));
            }
        }
    }
    line
}

/// The card's cost line: what the wakes cost so far, the most still to
/// spend, and a cold re-write now.
fn cost_line(state: &State, cfg: &Config) -> String {
    let p = &state.policy;
    let sent = p.count + p.after_count;
    let wakes = if sent == 1 { "wake" } else { "wakes" };
    let no_wrap = p.wrapped_at.is_none() && cfg.no_wrap().is_some();
    let after_left = match p.wrapped_at {
        _ if no_wrap => 0,
        Some(_) if after_wrap_ends(cfg, p).is_some() => 0,
        _ => cfg.after_max.saturating_sub(p.after_count),
    };
    let before_left = if p.wrapped_at.is_some() { 0 } else { cfg.max.saturating_sub(p.count) };
    let more = fmt_usd(state.warm_usd().map(|w| w * (before_left + after_left) as f64));
    let tail = if no_wrap {
        format!("at most {more} more in wakes before the bound")
    } else if p.wrapped_at.is_some() && cfg.after_max > 0 {
        format!("at most {more} more in wakes after the wrap")
    } else if cfg.after_max > 0 {
        format!("at most {more} more in wakes, {after_left} of them after the wrap, and the wrap reads the context once per tool round")
    } else {
        format!("at most {more} more in wakes before the wrap, and the wrap reads the context once per tool round")
    };
    format!("cost        {sent} {wakes} {} so far · {tail} · a cold re-write now {}", fmt_usd(Some(p.spent_usd)), fmt_usd(state.cold_usd()))
}

#[cfg(test)]
mod tests;
