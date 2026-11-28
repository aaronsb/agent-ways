// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The operator hold. Some stops wait on the operator: a relay that stopped
//! on `decision` or `doubt`, a judged boundary `doubt`, and a boundary `stop` whose
//! handoff lists a decision with a one-way lean (`reversible: no`). When
//! such a stop is recorded, keepwarm works out when the operator is likely
//! back ([`expected_return`]) and keeps it as a [`Wait`]. At the first tick
//! it decides once whether to hold the cache that long: it holds when
//! `ceil(wait / tick)` pings fit within the rent-or-buy bound, and
//! otherwise lets the cache lapse at that tick, with no partial pinging. A
//! hold ends at the expected return or at the operator's prompt, which
//! clears the wait ([`super::reset`]).
//!
//! The expected return is the stop plus `operator_horizon` (10h by
//! default). Where the zone is known ([`crate::windows::Zone`]), the local
//! night, from midnight to `morning` (09:00 by default), moves it: a stop in
//! the night waits until that morning, and a horizon that would end in the
//! night runs on to the morning. Where the zone is unknown, the plain
//! horizon applies.
//!
//! References:
//! - ADR-418 item 5: the rent-or-buy bound the hold is held to.
//! - ADR-418 item 6: waiting on the operator is pending, until the
//!   operator is likely back.
//! - ADR-418 item 11: a one-way decision waits for the operator.

use super::armed::{bound, Sufficiency};
use super::{Config, Decision, Policy, VERDICT_SECS};
use crate::{cache_lapsed, price_of, wake_line, Reading, State, TTL_SECS};
use jiff::tz::TimeZone;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// `mods.attend.continuity.operator_horizon`: how long after a stop the
/// operator is likely back, by default.
pub const DEFAULT_HORIZON_SECS: u64 = 10 * 3600;
/// The horizons the setting takes, in seconds.
pub const HORIZON_RANGE: (u64, u64) = (3600, 48 * 3600);
/// `mods.attend.continuity.morning`, in minutes after local midnight.
pub const DEFAULT_MORNING_MIN: u16 = 9 * 60;

/// What a stop waits on the operator for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    /// The relay stopped on `decision`.
    Decision,
    /// The relay stopped on `doubt`, and its handoff went to the operator.
    #[serde(rename = "relay-doubt")]
    RelayDoubt,
    /// The boundary judge's `doubt`, whose handoff goes to the operator.
    Doubt,
    /// A boundary `stop` whose handoff lists a one-way decision.
    Decisions,
}

impl Cause {
    pub fn describe(self) -> &'static str {
        match self {
            Cause::Decision => "the relay stopped on a decision",
            Cause::RelayDoubt => "the relay stopped on doubt",
            Cause::Doubt => "the judge's doubt went to the operator",
            Cause::Decisions => "the handoff lists a one-way decision",
        }
    }

    /// The wait a relay stop for `reason` records: `decision` and `doubt`
    /// both send the handoff to the operator and wait on the answer.
    pub fn of_relay_stop(reason: &str) -> Option<Cause> {
        match reason {
            "decision" => Some(Cause::Decision),
            "doubt" => Some(Cause::RelayDoubt),
            _ => None,
        }
    }

    /// Whether the wait is a relay stop's, which holds by itself; a
    /// boundary's rides the armed exit.
    fn is_relay(self) -> bool {
        matches!(self, Cause::Decision | Cause::RelayDoubt)
    }
}

/// A stop that waits on the operator, recorded when the stop is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wait {
    pub cause: Cause,
    /// When the stop was recorded.
    pub at: u64,
    /// When the operator is likely back.
    pub until: u64,
    /// `until` as `HH:MM`, in the zone it was read in.
    pub until_hm: String,
    /// The IANA zone the night was read in; `None` where the zone was
    /// unknown and the plain horizon applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// The decision the first tick made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<Decided>,
    /// Pings sent for a relay stop's hold. An armed exit counts its own.
    #[serde(default)]
    pub pings: u32,
    /// When the hold of a relay stop ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
}

/// Hold or lapse, decided once at the first tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decided {
    pub hold: bool,
    /// `ceil(wait / tick)`: the pings that keep the cache warm to `until`.
    pub needed: u32,
    /// The bound at the first tick's context.
    pub worth: u32,
}

/// The operator's expected return for a stop at `at`, `horizon` seconds
/// later, with the night read in `tz` up to `morning` minutes after local
/// midnight. `tz` is `None` where the zone is unknown: the plain horizon.
pub fn expected_return(at: u64, horizon: u64, morning: u16, tz: Option<&TimeZone>) -> u64 {
    let plain = at + horizon;
    let Some(tz) = tz else { return plain };
    let in_night = |epoch: u64| -> Option<u64> {
        let z = Timestamp::from_second(epoch as i64).ok()?.to_zoned(tz.clone());
        let minute = z.hour() as u16 * 60 + z.minute() as u16;
        if minute >= morning {
            return None;
        }
        let m = z.date().at((morning / 60) as i8, (morning % 60) as i8, 0, 0).to_zoned(tz.clone()).ok()?;
        Some(m.timestamp().as_second().max(0) as u64)
    };
    // A stop in the night waits until that morning; a horizon ending in
    // the night runs on to the morning.
    in_night(at).or_else(|| in_night(plain)).unwrap_or(plain)
}

/// The wait for a stop of `cause` at `now`, under `cfg`'s horizon, morning
/// and zone.
pub fn begin(cause: Cause, cfg: &Config, now: u64) -> Wait {
    let zone = cfg.windows.zone();
    let until = expected_return(now, cfg.horizon_secs, cfg.morning_min, zone.known());
    Wait { cause, at: now, until, until_hm: cfg.windows.hm(until), zone: zone.name.clone(), decided: None, pings: 0, ended_at: None }
}

/// `ceil(wait / tick)`: the pings that keep the cache warm through a wait
/// of `wait` seconds at one ping per `tick` seconds.
pub fn needed(wait: u64, tick: u64) -> u32 {
    wait.div_ceil(tick.max(1)) as u32
}

/// The one-way decisions a handoff lists: each numbered line of its
/// DECISIONS section whose `reversible:` field is `no`, by its question.
/// A section in another form lists none.
///
/// References:
/// - ADR-418 item 11: a one-way lean waits for the operator.
pub fn one_way_decisions(handoff: &str) -> Vec<String> {
    let mut lines = handoff.lines().map(str::trim);
    let header = |l: &str| l == "DECISIONS" || l.starts_with("DECISIONS ") || l.starts_with("DECISIONS:") || l.starts_with("DECISIONS —");
    if lines.by_ref().find(|l| header(l)).is_none() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for l in lines.skip_while(|l| l.is_empty()) {
        let Some((n, rest)) = l.split_once(". ") else { break };
        if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        let fields: Vec<&str> = rest.split(" — ").map(str::trim).collect();
        if fields.len() >= 4 && fields.iter().any(|f| f.eq_ignore_ascii_case("reversible: no")) {
            out.push(fields[0].to_string());
        }
    }
    out
}

/// Whether the wait keeps the session warm at this tick, deciding hold or
/// lapse the first time it is asked. `ctx` and `model` price the bound for
/// `handoff`; `tick` is the spacing of the pings. `None` with no wait, a
/// lapse, a hold whose time has come, or an ended one; the reason to stay
/// otherwise. A model with no price leaves the decision for a tick that
/// can make it, and keeps the session as a reason to stay.
pub fn stay(p: &mut Policy, handoff: Sufficiency, ctx: u64, model: &str, tick: u64, now: u64) -> Option<String> {
    let w = p.wait.as_mut()?;
    if w.ended_at.is_some() {
        return None;
    }
    if w.decided.is_none() {
        let price = price_of(model);
        let Some(price) = price else {
            return Some(format!("{}, until about {}", w.cause.describe(), w.until_hm));
        };
        let worth = bound(handoff, ctx, price).worth;
        let needed = needed(w.until.saturating_sub(w.at), tick);
        w.decided = Some(Decided { hold: needed <= worth, needed, worth });
    }
    let d = w.decided?;
    (d.hold && now < w.until).then(|| format!("{}, holding to {} ({} pings within the bound of {})", w.cause.describe(), w.until_hm, d.needed, d.worth))
}

/// Whether the wait's hold is the reason a tick pings now: decided hold,
/// before the expected return, and not ended.
pub fn holding(p: &Policy, now: u64) -> bool {
    p.wait.as_ref().is_some_and(|w| w.ended_at.is_none() && now < w.until && w.decided.is_some_and(|d| d.hold))
}

/// The relay stop's hold for one check, or `None` where none applies: the
/// wait is no relay stop on a decision or doubt, an armed exit owns the session, or the
/// hold is over. A relay that stopped on a decision or doubt runs no keepalive
/// otherwise, so its hold pings by itself: once a tick, while the hold
/// lasts, as the armed exit does. Its end lets the cache lapse, and the
/// relay stays stopped until the operator's next prompt.
///
/// References:
/// - ADR-417 item 5: a relay stop holds every check until the operator's
///   next prompt.
pub(super) fn relay_tick(state: &mut State, reading: &Reading, cfg: &Config, now: u64) -> Option<Decision> {
    let w = state.policy.wait.clone()?;
    if !w.cause.is_relay() || w.ended_at.is_some() || state.policy.exit.as_ref().is_some_and(|e| e.armed()) {
        return None;
    }
    let held = |state: &State, reason: String| Decision::new(cfg, &state.policy, true, "stopped", reason);
    if let Some(t) = state.pinged_at {
        let mut d = held(state, "stopped: waiting for the hold's ping's turn".into());
        d.next_at = Some(t + VERDICT_SECS + 1);
        return Some(d);
    }
    let Some(last) = reading.usage_tail.last().filter(|l| l.at_epoch > 0) else {
        return end(state, cfg, "no request yet in this session", now);
    };
    if last.ttl_secs() < TTL_SECS {
        return end(state, cfg, "the session is on the five-minute cache tier", now);
    }
    let tick = super::tick_of(last, cfg);
    if now < tick {
        let mut d = held(state, format!("stopped: {}; the next tick is at {}", w.cause.describe(), cfg.windows.hm(tick)));
        d.next_at = Some(tick);
        return Some(d);
    }
    if cache_lapsed(Some(reading), now) {
        return end(state, cfg, &format!("the cache had lapsed at the tick {}", cfg.windows.hm(tick)), now);
    }
    if !cfg.enabled {
        return end(state, cfg, "mods.attend.continuity.keepalive is false", now);
    }
    if !cfg.windows.contains(now) {
        return end(state, cfg, "the keepalive hours have ended", now);
    }
    let spacing = last.ttl_secs().saturating_sub(cfg.margin_secs);
    let model = state.model.clone();
    let Some(why) = stay(&mut state.policy, Sufficiency::Sufficient, state.ctx, &model, spacing, now) else {
        let why = match state.policy.wait.as_ref().and_then(|w| w.decided) {
            Some(d) if !d.hold => format!("{} pings to {} pass the bound of {}", d.needed, w.until_hm, d.worth),
            _ => format!("the operator's expected return at {} has come", w.until_hm),
        };
        return end(state, cfg, &why, now);
    };
    // With no price the hold is undecided, and the relay stop runs no
    // keepalive, as it did before the hold.
    price_of(&model)?;
    let n = {
        let w = state.policy.wait.as_mut()?;
        w.pings += 1;
        w.pings
    };
    state.policy.pending = true;
    state.pinged_at = Some(now);
    let mut d = held(state, format!("stopped: {why}; ping {n}"));
    d.action = super::Action::Keepalive;
    d.text = Some(wake_line(now.saturating_sub(last.at_epoch), last.ttl_secs(), state.ctx));
    d.next_at = Some(now + VERDICT_SECS + 1);
    Some(d)
}

/// The relay stop's hold ends: no ping, and the cache lapses.
fn end(state: &mut State, cfg: &Config, why: &str, now: u64) -> Option<Decision> {
    if let Some(w) = state.policy.wait.as_mut() {
        w.ended_at = Some(now);
    }
    let stopped = state.policy.stopped.clone().unwrap_or_default();
    Some(Decision::new(cfg, &state.policy, true, "stopped", format!("stopped: {stopped}; the hold ended: {why}; the cache lapses; nothing more until the operator's next prompt")))
}

/// When the relay stop's hold next ticks, or `None` where it does not.
pub(super) fn tick_at(state: &State, reading: &Reading, cfg: &Config) -> Option<u64> {
    let w = state.policy.wait.as_ref()?;
    if !w.cause.is_relay() || w.ended_at.is_some() || state.policy.exit.as_ref().is_some_and(|e| e.armed()) {
        return None;
    }
    reading.usage_tail.last().filter(|l| l.at_epoch > 0).map(|l| super::tick_of(l, cfg))
}

/// `"hold"` in `attend keepwarm status --json`: hold or lapse, the end of a
/// hold, the pings the wait needs, and why. Every field is null with no
/// wait; `decision`, `until` and `pings` are null until the first tick
/// decides, and `until` is null for a lapse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HoldView {
    pub decision: Option<&'static str>,
    pub until: Option<u64>,
    pub pings: Option<u32>,
    pub reason: Option<String>,
}

/// The hold as status shows it.
pub fn view(p: &Policy) -> HoldView {
    let Some(w) = &p.wait else {
        return HoldView { decision: None, until: None, pings: None, reason: None };
    };
    let zone = if w.zone.is_some() { "" } else { ", the zone unknown, so the plain horizon" };
    let (decision, until, pings, reason) = match w.decided {
        None => (None, None, None, format!("{}: the operator is likely back about {}{zone}; the first tick decides hold or lapse", w.cause.describe(), w.until_hm)),
        Some(d) if d.hold => (Some("hold"), Some(w.until), Some(d.needed), format!("{}: holding to {}{zone}, {} pings within the bound of {}", w.cause.describe(), w.until_hm, d.needed, d.worth)),
        Some(d) => (Some("lapse"), None, Some(d.needed), format!("{}: lapsing, {} pings to {}{zone} pass the bound of {}", w.cause.describe(), d.needed, w.until_hm, d.worth)),
    };
    HoldView { decision, until, pings, reason: Some(reason) }
}

/// The status card's hold line, or `None` with no wait.
pub fn card_line(p: &Policy) -> Option<String> {
    let w = p.wait.as_ref()?;
    let line = match w.decided {
        None => format!("waiting on the operator to about {}; the first tick decides hold or lapse", w.until_hm),
        Some(d) if d.hold => format!("holding to {} ({} pings)", w.until_hm, d.needed),
        Some(_) => format!("lapsing: the bound ends before {}", w.until_hm),
    };
    let zone = if w.zone.is_some() { "" } else { " · the zone is unknown, so the plain horizon" };
    Some(format!("hold        {line} · {}{zone}", w.cause.describe()))
}
