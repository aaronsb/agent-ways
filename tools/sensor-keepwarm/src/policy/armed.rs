// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The armed exit. At a work boundary the judge's `stop` verdict arms it
//! (`attend keepwarm judged`). From then on each idle due point re-reads
//! the handoff's first line. When the line still carries the wrap id, the
//! ski-rental rule decides the tick. With something pending, or with a
//! handoff that holds detail, the tick pings while the rent paid plus the
//! next ping stays within the purchase price. Otherwise it returns
//! `finish`: no ping goes out, the cache lapses, and the session is
//! finished. A missing or stale handoff means the tick pings as it does
//! today. An operator prompt ([`super::reset`]) clears both states.
//!
//! The rent is one cache read of the whole context per ping. The purchase
//! is a fresh start ([`FRESH_START_TOKENS`] written at the one-hour rate)
//! for a sufficient handoff, and a cold reload of the whole context for
//! one that holds detail.
//!
//! References:
//! - ADR-418 item 5: the ski-rental rule and its numbers.
//! - ADR-418 item 6: how long an armed exit stays warm.
//! - ADR-418 item 9: the `armed` and `finished` states, and the wrap id
//!   checked against the handoff at each tick.
//! - ADR-418 item 13: the handoff is the exit flag; an operator prompt
//!   disarms.

use super::{due_point, is_wrap_id, watch, Action, Config, Decision, Handoff, Idle, Policy, Site, PLAIN_EVERY_SECS, VERDICT_SECS};
use crate::{cache_lapsed, fmt_duration, fmt_usd, price_of, wake_line, ArmFile, Outcome, Price, Reading, State, FRESH_START_TOKENS};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The judge's verdict for a boundary, or the working agent's own call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Stop,
    Compact,
    Continue,
    Doubt,
}

impl Verdict {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "stop" => Verdict::Stop,
            "compact" => Verdict::Compact,
            "continue" => Verdict::Continue,
            "doubt" => Verdict::Doubt,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Stop => "stop",
            Verdict::Compact => "compact",
            Verdict::Continue => "continue",
            Verdict::Doubt => "doubt",
        }
    }
}

/// The judge's answer on the handoff: whether a fresh start replaces the
/// session, or getting it back takes a cold reload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sufficiency {
    Sufficient,
    HoldsDetail,
}

impl Sufficiency {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "sufficient" => Sufficiency::Sufficient,
            "holds-detail" => Sufficiency::HoldsDetail,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Sufficiency::Sufficient => "sufficient",
            Sufficiency::HoldsDetail => "holds-detail",
        }
    }
}

/// What `attend keepwarm judged` reports for one boundary wrap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    pub wrap_id: String,
    pub verdict: Verdict,
    pub handoff: Sufficiency,
    /// The working agent's own call, kept apart from the judge's.
    pub own: Verdict,
    pub reason: Option<String>,
}

/// The judgment of the last boundary wrap, in the ledger. A `stop` arms the
/// exit; `finished_at` marks the tick that let the cache lapse.
///
/// References:
/// - ADR-418 item 14, item 4: the ledger records the judge's verdict, its
///   sufficiency answer and the working agent's own call for each boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exit {
    pub wrap_id: String,
    pub verdict: Verdict,
    pub handoff: Sufficiency,
    pub own: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub judged_at: u64,
    /// The handoff file the judgment was checked against. Each tick reads
    /// its first line again, so a check every minute runs no git.
    pub path: PathBuf,
    /// Pings sent while armed.
    #[serde(default)]
    pub pings: u32,
    /// When the tick let the cache lapse and the session finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
}

impl Exit {
    /// Armed: the judge said stop, and no tick has finished the session yet.
    pub fn armed(&self) -> bool {
        self.verdict == Verdict::Stop && self.finished_at.is_none()
    }
}

/// `attend keepwarm judged`: records the boundary's judgment, and with
/// `stop` arms the exit. Refused, changing nothing, when the handoff is
/// missing or its first line does not carry the wrap id.
///
/// References:
/// - ADR-418 item 13: the mod refuses to arm the exit until the handoff
///   exists, and the handoff's first line carries the wrap id.
pub fn judged(p: &mut Policy, j: Judgement, handoff: Option<&Handoff>, now: u64) -> Result<(), String> {
    if !is_wrap_id(&j.wrap_id) {
        return Err(format!("{:?} is no wrap id (8 to 64 letters, digits and -)", j.wrap_id));
    }
    let Some(h) = handoff else {
        return Err("no handoff: .claude/handoff.wrap.md is missing".into());
    };
    if h.wrap_id.as_deref() != Some(j.wrap_id.as_str()) {
        return Err(format!("{}'s first line does not carry wrap={}", h.path.display(), j.wrap_id));
    }
    let reason = j.reason.map(|r| r.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|r| !r.is_empty());
    p.last_wrap_id = Some(j.wrap_id.clone());
    p.exit = Some(Exit {
        wrap_id: j.wrap_id,
        verdict: j.verdict,
        handoff: j.handoff,
        own: j.own,
        reason,
        judged_at: now,
        path: h.path.clone(),
        pings: 0,
        finished_at: None,
    });
    Ok(())
}

/// What is pending: threads awaiting a reply, and the agents, shells and
/// watchers the mod reports still running (`--pending`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub awaiting: u32,
    pub agents: u32,
    pub shells: u32,
    pub watchers: u32,
}

/// `--pending` as the host gave it: the counts, and whether it named shells
/// and watchers. The mod names agents alone, and the shells and watchers
/// come from the last Stop's record ([`crate::background`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Given {
    pub counts: Pending,
    pub shells: bool,
    pub watchers: bool,
}

impl Given {
    /// `agents=N,shells=N,watchers=N`, each key at most once, any left out
    /// read as zero and as not given.
    pub fn parse(s: &str) -> Result<Given, String> {
        let mut g = Given::default();
        let mut seen = Vec::new();
        for part in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
            let (k, v) = part.split_once('=').ok_or_else(|| format!("{part:?} is not key=N"))?;
            let n: u32 = v.trim().parse().map_err(|_| format!("{part:?}: {v:?} is not a count"))?;
            let k = k.trim();
            if seen.contains(&k) {
                return Err(format!("{k} is given twice"));
            }
            match k {
                "agents" => g.counts.agents = n,
                "shells" => (g.counts.shells, g.shells) = (n, true),
                "watchers" => (g.counts.watchers, g.watchers) = (n, true),
                _ => return Err(format!("{k:?} is not agents, shells or watchers")),
            }
            seen.push(k);
        }
        Ok(g)
    }

    /// The counts, with the shells and watchers `bg` recorded in place of
    /// any the host did not name. A count the host gave wins.
    pub fn with(self, bg: Option<&crate::background::Background>) -> Pending {
        let mut p = self.counts;
        if let Some(b) = bg {
            if !self.shells {
                p.shells = b.shells.len() as u32;
            }
            if !self.watchers {
                p.watchers = b.watchers.len() as u32;
            }
        }
        p
    }
}

impl Pending {
    pub fn any(&self) -> bool {
        self.awaiting + self.agents + self.shells + self.watchers > 0
    }

    /// `1 thread awaiting a reply, 2 agents`.
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        let mut put = |n: u32, one: &str, many: &str| {
            if n > 0 {
                parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        };
        put(self.awaiting, "thread awaiting a reply", "threads awaiting a reply");
        put(self.agents, "agent", "agents");
        put(self.shells, "shell", "shells");
        put(self.watchers, "watcher", "watchers");
        parts.join(", ")
    }
}

/// The ski-rental bound at a context of `ctx` tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    /// One ping: a cache read of the whole context.
    pub rent: f64,
    /// What replaces the session.
    pub purchase: f64,
    /// Pings whose rent stays within the purchase.
    pub worth: u32,
}

/// The bound for `handoff` at `ctx` tokens priced at `price`: a fresh start
/// for a sufficient handoff, a cold reload of the whole context for one
/// that holds detail.
pub fn bound(handoff: Sufficiency, ctx: u64, price: Price) -> Bound {
    let rent = ctx as f64 * price.read / 1e6;
    let bought = match handoff {
        Sufficiency::Sufficient => FRESH_START_TOKENS,
        Sufficiency::HoldsDetail => ctx,
    };
    let purchase = bought as f64 * price.write_1h / 1e6;
    // The ratio of two prices from the table: a hair of slack keeps 40.0
    // from reading as 39.999.
    let worth = if rent > 0.0 { (purchase / rent + 1e-9).floor().max(0.0) as u32 } else { 0 };
    Bound { rent, purchase, worth }
}

/// The exit as `attend keepwarm check --policy --json` and the card show
/// it: its state, the rent paid, the purchase price and the pings left.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExitView {
    /// `armed`, `finished`, or the verdict of a judgment that armed nothing.
    pub state: &'static str,
    pub wrap_id: String,
    pub verdict: &'static str,
    pub handoff: &'static str,
    pub own: &'static str,
    pub pings: u32,
    /// `None` where the model has no price.
    pub rent_usd: Option<f64>,
    pub purchase_usd: Option<f64>,
    pub pings_left: Option<u32>,
    pub finished_at: Option<u64>,
}

/// The policy's exit as the check and the card show it, at the context and
/// model `state` last saw.
pub fn view(state: &State) -> Option<ExitView> {
    let e = state.policy.exit.as_ref()?;
    let b = price_of(&state.model).map(|p| bound(e.handoff, state.ctx, p));
    Some(ExitView {
        state: if e.finished_at.is_some() {
            "finished"
        } else if e.armed() {
            "armed"
        } else {
            e.verdict.as_str()
        },
        wrap_id: e.wrap_id.clone(),
        verdict: e.verdict.as_str(),
        handoff: e.handoff.as_str(),
        own: e.own.as_str(),
        pings: e.pings,
        rent_usd: b.map(|b| b.rent * e.pings as f64),
        purchase_usd: b.map(|b| b.purchase),
        pings_left: b.map(|b| b.worth.saturating_sub(e.pings)),
        finished_at: e.finished_at,
    })
}

/// The status card's exit line, or `None` with no judgment on record.
pub fn card_line(state: &State, now: u64) -> Option<String> {
    let v = view(state)?;
    let money = match (v.rent_usd, v.purchase_usd) {
        (Some(r), Some(p)) => {
            let what = if v.handoff == "sufficient" { "a fresh start" } else { "a cold reload" };
            format!("{} rent {} paid of {} ({what})", pings(v.pings), fmt_usd(Some(r)), fmt_usd(Some(p)))
        }
        _ => format!("{} · no price for {}", pings(v.pings), if state.model.is_empty() { "the model" } else { &state.model }),
    };
    let left = v.pings_left.map_or_else(|| " · pings left unknown".to_string(), |n| format!(" · {} left", pings(n)));
    let wrap = format!("wrap {}, the judge said {}, the handoff is {}, own call {}", v.wrap_id, v.verdict, v.handoff.replace('-', " "), v.own);
    Some(match v.state {
        "finished" => format!(
            "exit        finished {} ago · {wrap} · {money} · your next prompt clears it",
            fmt_duration(now.saturating_sub(v.finished_at.unwrap_or(now)))
        ),
        "armed" if v.handoff == "sufficient" => format!("exit        armed · {wrap} · {money}{left} while something is pending, else it finishes at the next tick"),
        "armed" => format!("exit        armed · {wrap} · {money}{left}"),
        _ => format!("exit        not armed · {wrap}"),
    })
}

/// Whether a stop exit is in force: the judge said stop and the session
/// finished, or the handoff still carries the wrap id. A stale or missing
/// handoff leaves the session to the checks of today.
pub(super) fn in_force(state: &State, site: &Site) -> bool {
    state.policy.exit.as_ref().is_some_and(|e| {
        e.verdict == Verdict::Stop && (e.finished_at.is_some() || site.handoff.as_ref().and_then(|h| h.wrap_id.as_deref()) == Some(e.wrap_id.as_str()))
    })
}

/// A stop exit in force where the policy is off: relay off with
/// autocompact on or unknown, or keepalive false. Its ticks run as they do
/// where the policy runs, and an armed window (`attend keepwarm on`)
/// overrides them as it does there. The decision is the plain check's
/// (`policy: false`), so the mod submits a ping as it submits a window's
/// wake, whatever autocompact says.
///
/// References:
/// - ADR-418 item 14, item 2(c): after a `stop` the idle path runs the
///   armed exit.
/// - ADR-182: the plain check's wake, which goes in with autocompact on.
pub(super) fn off(state: &mut State, reading: &Reading, arm: Option<&ArmFile>, cfg: &Config, site: &Site, why: &str, now: u64) -> (Outcome, Decision) {
    let out = watch(state, reading, arm, now);
    let judged = if out.observations.is_empty() && state.deadline <= now { judge(state, reading, cfg, site, now) } else { None };
    let d = judged.unwrap_or_else(|| {
        let mut d = Decision::new(cfg, &state.policy, false, "off", why);
        if let Some((_, line)) = out.observations.first() {
            d.action = Action::Keepalive;
            d.text = Some(line.clone());
            d.reason = format!("{why}; an armed window's wake");
        }
        d.next_at = Some(now + PLAIN_EVERY_SECS);
        d
    });
    (out, Decision { policy: false, ..d })
}

/// The armed exit's judgment for one check, or `None` where no stop exit is
/// in force ([`in_force`]) and the tick pings as it does today. The model's
/// price unknown, the bound is unknown too, and a tick with a reason to
/// stay pings as today.
pub(super) fn judge(state: &mut State, reading: &Reading, cfg: &Config, site: &Site, now: u64) -> Option<Decision> {
    if !in_force(state, site) {
        return None;
    }
    let e = state.policy.exit.clone()?;
    let p = &state.policy;
    if let Some(at) = e.finished_at {
        let why = format!("finished at {}; nothing more until the operator's next prompt", cfg.windows.hm(at));
        return Some(Decision::new(cfg, p, true, "finished", why));
    }
    if let Some(t) = state.pinged_at {
        let mut d = Decision::new(cfg, p, true, "armed", "armed: waiting for the ping's turn");
        d.next_at = Some(t + VERDICT_SECS + 1);
        return Some(d);
    }
    let Some(last) = reading.usage_tail.last().filter(|l| l.at_epoch > 0) else {
        let mut d = Decision::new(cfg, p, true, "armed", "armed: no request yet in this session");
        d.idle = Some("no request yet");
        return Some(d);
    };
    let ttl = last.ttl_secs();
    let tick = last.at_epoch + ttl.saturating_sub(cfg.margin_secs);
    if now < tick {
        let mut d = Decision::new(cfg, p, true, "armed", format!("armed: the next tick is at {}", cfg.windows.hm(tick)));
        d.next_at = Some(tick);
        return Some(d);
    }
    let pending = cfg.pending;
    let stay = match (pending.any(), e.handoff) {
        (true, _) => Some(pending.describe()),
        (false, Sufficiency::HoldsDetail) => Some("the handoff holds detail a cold reload would have to rebuild".to_string()),
        (false, Sufficiency::Sufficient) => None,
    };
    let Some(stay) = stay else {
        return Some(finish(state, cfg, "nothing is pending and the handoff is sufficient", now));
    };
    // The 50k floor prices a cold re-write against no reason to stay. With
    // one, the bound decides.
    //
    // References:
    // - ADR-182: the floor under which a wake is not worth it.
    let no_ping = if let Err(why @ (Idle::NoRequest | Idle::FiveMinuteTier)) = due_point(state, reading, cfg) {
        Some(why.reason().to_string())
    } else if cache_lapsed(Some(reading), now) {
        Some(format!("the cache had lapsed at the tick {}", cfg.windows.hm(tick)))
    } else if let Some(why) = &state.policy.stopped {
        Some(why.clone())
    } else if !cfg.enabled {
        Some("mods.attend.continuity.keepalive is false".to_string())
    } else if !cfg.windows.contains(now) {
        Some("the keepalive hours have ended".to_string())
    } else {
        None
    };
    if let Some(why) = no_ping {
        return Some(finish(state, cfg, &why, now));
    }
    let price = price_of(&state.model)?;
    let b = bound(e.handoff, state.ctx, price);
    if e.pings >= b.worth {
        let why = format!("{stay}, but {} rent {} paid of {} and the next would pass the purchase price", pings(e.pings), fmt_usd(Some(b.rent * e.pings as f64)), fmt_usd(Some(b.purchase)));
        return Some(finish(state, cfg, &why, now));
    }
    let n = e.pings + 1;
    if let Some(x) = state.policy.exit.as_mut() {
        x.pings = n;
    }
    state.policy.pending = true;
    state.pinged_at = Some(now);
    let p = &state.policy;
    let reason = format!("armed: {stay}; ping {n} of {} ({} each against {})", b.worth, fmt_usd(Some(b.rent)), fmt_usd(Some(b.purchase)));
    let mut d = Decision::new(cfg, p, true, "armed", reason);
    d.action = Action::Keepalive;
    d.text = Some(wake_line(now.saturating_sub(last.at_epoch), ttl, state.ctx));
    d.next_at = Some(now + VERDICT_SECS + 1);
    Some(d)
}

/// `1 ping`, `19 pings`.
fn pings(n: u32) -> String {
    format!("{n} {}", if n == 1 { "ping" } else { "pings" })
}

/// The tick lets the cache lapse: no ping, and the session is finished.
fn finish(state: &mut State, cfg: &Config, why: &str, now: u64) -> Decision {
    if let Some(x) = state.policy.exit.as_mut() {
        x.finished_at = Some(now);
    }
    let mut d = Decision::new(cfg, &state.policy, true, "finished", format!("finished: {why}; the cache lapses and attend marks the session finished"));
    d.action = Action::Finish;
    d
}
