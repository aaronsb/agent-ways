//! Keepwarm sensor (ADR-182): one Monitor wake per idle stretch, so the
//! session's prompt cache is read before its hour lapses instead of
//! rewritten after.
//!
//! The transcript is the clock and the scorecard. `ways context --json
//! --session <id>` hands the sensor the last assistant usage entries;
//! the newest one's timestamp is the idle clock, and the first entry
//! after a wake carries the verdict (a cache read with a small write
//! means the wake kept the cache; a write of a tenth or more of the read
//! means the cache was already gone).
//!
//! Two files under attend's cache dir, both keyed by session id:
//! - the arm file, written by `attend keepwarm on|off` and by the
//!   sensor's auto-arm after a paid cold write; read every poll
//! - the ledger, written by the sensor and read by `attend keepwarm
//!   status` for the card
//!
//! The pure decision lives in [`State::step`], with the file and
//! process reads around it in [`KeepwarmSensor::poll`].

use sensor_trait::{Focus, Sensor};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The main conversation's cache tier is one hour.
pub const TTL_SECS: u64 = 60 * 60;
/// The wake fires this far into an idle stretch: ten minutes of margin
/// for the poll interval, the governor, and the turn itself.
pub const PING_AFTER_SECS: u64 = 50 * 60;
/// A paid cold write arms this much of a window when none covers it.
pub const AUTO_WARM_SECS: u64 = 3 * 60 * 60;
/// `attend keepwarm on` with no window.
pub const DEFAULT_WINDOW_SECS: u64 = 6 * 60 * 60;
/// Below this context size a cold rewrite is cheap and the wake is not.
pub const BIG_TOKENS: u64 = 50_000;
/// A cold write is only scored against a prior context above this.
pub const MISS_FLOOR_TOKENS: u64 = 20_000;

// ── Prices ─────────────────────────────────────────────────────

/// Dollars per million tokens: cache read, 1-hour cache write, output.
/// List prices, September 2026. The 1-hour write is twice the base
/// input rate; cache reads are a tenth of it except on Fable 5.1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub read: f64,
    pub write_1h: f64,
    pub output: f64,
}

/// Longer family names first: a model id matches the first row it contains.
const PRICES: &[(&str, Price)] = &[
    ("fable-5-1", Price { read: 0.25, write_1h: 20.0, output: 50.0 }),
    ("fable-5", Price { read: 1.0, write_1h: 20.0, output: 50.0 }),
    ("opus-5", Price { read: 0.5, write_1h: 10.0, output: 25.0 }),
    ("opus-4", Price { read: 0.5, write_1h: 10.0, output: 25.0 }),
    ("sonnet-5", Price { read: 0.2, write_1h: 4.0, output: 10.0 }),
    ("sonnet", Price { read: 0.3, write_1h: 6.0, output: 15.0 }),
    ("haiku", Price { read: 0.1, write_1h: 2.0, output: 5.0 }),
];

pub fn price_of(model: &str) -> Option<Price> {
    let m = model.to_lowercase().replace(['.', ' '], "-");
    PRICES.iter().find(|(family, _)| m.contains(family)).map(|(_, p)| *p)
}

pub fn fmt_usd(usd: Option<f64>) -> String {
    match usd {
        None => "n/a".to_string(),
        Some(u) if u >= 100.0 => format!("${u:.0}"),
        Some(u) => format!("${u:.2}"),
    }
}

pub fn fmt_tok(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{}k", (n + 500) / 1000)
    } else {
        n.to_string()
    }
}

/// `6h00m`, `35m`, `2d 3h`.
pub fn fmt_duration(secs: u64) -> String {
    let total = (secs + 30) / 60;
    let h = total / 60;
    let m = total % 60;
    if h >= 48 {
        format!("{}d {}h", h / 24, h % 24)
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

/// `6h`, `90m`, `2h30m` to seconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (hours, rest) = match t.split_once('h') {
        Some((h, rest)) => (Some(h.parse::<u64>().ok()?), rest),
        None => (None, t),
    };
    let minutes = match rest {
        "" => None,
        r => Some(r.strip_suffix('m')?.parse::<u64>().ok()?),
    };
    if hours.is_none() && minutes.is_none() {
        return None;
    }
    Some(hours.unwrap_or(0) * 3600 + minutes.unwrap_or(0) * 60)
}

// ── Files ──────────────────────────────────────────────────────

/// Written by the CLI and by the sensor's auto-arm. A deadline of zero
/// is disarmed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ArmFile {
    pub deadline: u64,
    pub armed_at: u64,
    pub window_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Miss {
    pub at: u64,
    pub tokens: u64,
    pub usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PingRecord {
    pub at: u64,
    pub read: u64,
    pub write: u64,
    pub usd: Option<f64>,
    pub warm: bool,
}

/// Written by the sensor after every change, read by the status card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub misses: Vec<Miss>,
    pub last_ping: Option<PingRecord>,
    pub stopped: Option<String>,
    pub pinged_at: Option<u64>,
    pub updated_at: u64,
}

pub fn arm_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.arm.json"))
}

pub fn ledger_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.ledger.json"))
}

pub fn read_arm(dir: &Path, session_id: &str) -> Option<ArmFile> {
    let text = std::fs::read_to_string(arm_path(dir, session_id)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_arm(dir: &Path, session_id: &str, arm: &ArmFile) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string(arm).map_err(std::io::Error::other)?;
    std::fs::write(arm_path(dir, session_id), text)
}

pub fn read_ledger(dir: &Path, session_id: &str) -> Option<Ledger> {
    let text = std::fs::read_to_string(ledger_path(dir, session_id)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_ledger(dir: &Path, session_id: &str, ledger: &Ledger) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string(ledger).map_err(std::io::Error::other)?;
    std::fs::write(ledger_path(dir, session_id), text)
}

// ── Reading ────────────────────────────────────────────────────

/// One assistant message's usage, from `ways context --json`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct UsageEntry {
    #[serde(default)]
    pub at_epoch: u64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_creation: u64,
    #[serde(default)]
    pub output: u64,
}

impl UsageEntry {
    /// The context the API saw on this request.
    pub fn context(&self) -> u64 {
        self.input + self.cache_read + self.cache_creation
    }
}

/// The slice of `ways context --json` the sensor reads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Reading {
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub usage_tail: Vec<UsageEntry>,
}

impl Reading {
    pub fn last_at(&self) -> u64 {
        self.usage_tail.last().map(|u| u.at_epoch).unwrap_or(0)
    }
}

/// Run `ways context --json --session <id>` and parse it.
pub fn read_context(session_id: &str, working_dir: &str) -> Option<Reading> {
    let output = Command::new("ways")
        .args(["context", "--json", "--session", session_id])
        .current_dir(working_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

// ── State and the decision ─────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub deadline: u64,
    pub baselined: bool,
    pub last_seen_at: u64,
    pub ctx: u64,
    pub model: String,
    pub pinged_at: Option<u64>,
    pub misses: Vec<Miss>,
    pub last_ping: Option<PingRecord>,
    pub stopped: Option<String>,
}

/// What one poll decided. `arm_write` is a new deadline for the arm file
/// (zero disarms); `changed` means the ledger should be rewritten.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub observations: Vec<(f64, String)>,
    pub logs: Vec<String>,
    pub arm_write: Option<u64>,
    pub changed: bool,
}

impl State {
    pub fn is_cold(&self, now: u64) -> bool {
        self.last_seen_at > 0 && now.saturating_sub(self.last_seen_at) >= TTL_SECS
    }

    pub fn cold_usd(&self) -> Option<f64> {
        price_of(&self.model).map(|p| self.ctx as f64 * p.write_1h / 1e6)
    }

    pub fn warm_usd(&self) -> Option<f64> {
        price_of(&self.model).map(|p| self.ctx as f64 * p.read / 1e6)
    }

    fn disarm(&mut self, out: &mut Outcome, why: Option<String>) {
        self.deadline = 0;
        self.pinged_at = None;
        self.stopped = why;
        out.arm_write = Some(0);
        out.changed = true;
    }

    /// One poll. `arm` is the arm file as it is on disk right now.
    pub fn step(&mut self, reading: &Reading, arm: Option<&ArmFile>, now: u64) -> Outcome {
        let mut out = Outcome::default();

        // The arm file is the operator's word. A fresh deadline clears a
        // stop; a cleared file disarms.
        let file_deadline = arm.map(|a| a.deadline).unwrap_or(0);
        if file_deadline != self.deadline {
            if file_deadline > now {
                self.stopped = None;
            }
            self.deadline = file_deadline;
            out.changed = true;
        }

        if !self.baselined {
            self.baselined = true;
            self.last_seen_at = reading.last_at();
            self.ctx = reading.tokens_used;
            self.model = reading.model.clone();
            out.logs.push(format!(
                "baseline: {} tokens, last request {} ago, {}",
                fmt_tok(self.ctx),
                fmt_duration(now.saturating_sub(self.last_seen_at)),
                if self.deadline > now { format!("armed for {}", fmt_duration(self.deadline - now)) } else { "off".to_string() },
            ));
            return out;
        }

        let cutoff = self.last_seen_at;
        for entry in reading.usage_tail.iter().filter(|u| u.at_epoch > cutoff) {
            let prior = self.ctx;
            if !entry.model.is_empty() {
                self.model = entry.model.clone();
            }
            let price = price_of(&self.model);

            // A write of half the prior context or more is a paid cold write.
            if prior > MISS_FLOOR_TOKENS && entry.cache_creation >= prior / 2 {
                let usd = price.map(|p| entry.cache_creation as f64 * p.write_1h / 1e6);
                self.misses.push(Miss { at: entry.at_epoch, tokens: entry.cache_creation, usd });
                out.changed = true;
                let mut line = format!("cold write of {} tokens paid ({})", fmt_tok(entry.cache_creation), fmt_usd(usd));
                if self.deadline < now + AUTO_WARM_SECS {
                    self.deadline = now + AUTO_WARM_SECS;
                    self.stopped = None;
                    out.arm_write = Some(self.deadline);
                    line.push_str(&format!(", keeping the cache warm for {} so it is not paid again today", fmt_duration(AUTO_WARM_SECS)));
                }
                out.logs.push(line);
            }

            // The first request after a wake carries the verdict.
            if let Some(pinged) = self.pinged_at {
                if entry.at_epoch >= pinged {
                    let warm = entry.cache_read > 0 && entry.cache_creation < entry.cache_read / 10;
                    let usd = price.map(|p| {
                        (entry.cache_read as f64 * p.read
                            + entry.cache_creation as f64 * p.write_1h
                            + entry.input as f64 * p.write_1h / 2.0
                            + entry.output as f64 * p.output)
                            / 1e6
                    });
                    self.last_ping = Some(PingRecord {
                        at: entry.at_epoch,
                        read: entry.cache_read,
                        write: entry.cache_creation,
                        usd,
                        warm,
                    });
                    self.pinged_at = None;
                    out.changed = true;
                    if warm {
                        out.logs.push(format!("wake read {} tokens from cache ({})", fmt_tok(entry.cache_read), fmt_usd(usd)));
                    } else {
                        let why = format!(
                            "the wake read {} and wrote {} tokens ({}), the cache was already gone",
                            fmt_tok(entry.cache_read),
                            fmt_tok(entry.cache_creation),
                            fmt_usd(usd),
                        );
                        out.logs.push(format!("stopped: {why}"));
                        self.disarm(&mut out, Some(why));
                    }
                }
            }

            self.ctx = entry.context();
            self.last_seen_at = entry.at_epoch;
        }
        if reading.tokens_used > 0 {
            self.ctx = reading.tokens_used;
        }

        if self.deadline > 0 && now >= self.deadline {
            out.logs.push("window ended".to_string());
            self.disarm(&mut out, None);
        }

        // A wake that produced no turn inside the margin: the cache has
        // lapsed and the next real message pays. Stop rather than wake
        // a session that is not listening.
        if let Some(pinged) = self.pinged_at {
            if now.saturating_sub(pinged) > TTL_SECS - PING_AFTER_SECS {
                let why = "the wake produced no turn within ten minutes".to_string();
                out.logs.push(format!("stopped: {why}"));
                self.disarm(&mut out, Some(why));
            }
        }

        if self.deadline > now && self.pinged_at.is_none() && self.last_seen_at > 0 {
            let idle = now.saturating_sub(self.last_seen_at);
            if idle >= TTL_SECS {
                let why = format!("the cache lapsed {} ago, before a wake could fire", fmt_duration(idle - TTL_SECS));
                out.logs.push(format!("stopped: {why}"));
                self.disarm(&mut out, Some(why));
            } else if idle >= PING_AFTER_SECS && self.ctx >= BIG_TOKENS {
                self.pinged_at = Some(now);
                out.changed = true;
                out.observations.push((
                    3.0,
                    format!(
                        "keepwarm: {} idle, the prompt cache lapses in {} on {} tokens. Reply with one word and no tools.",
                        fmt_duration(idle),
                        fmt_duration(TTL_SECS - idle),
                        fmt_tok(self.ctx),
                    ),
                ));
            }
        }

        out
    }

    pub fn ledger(&self, now: u64) -> Ledger {
        Ledger {
            misses: self.misses.clone(),
            last_ping: self.last_ping.clone(),
            stopped: self.stopped.clone(),
            pinged_at: self.pinged_at,
            updated_at: now,
        }
    }
}

// ── The card ───────────────────────────────────────────────────

/// The lines `attend keepwarm status` prints.
pub fn card(reading: &Reading, arm: Option<&ArmFile>, ledger: Option<&Ledger>, now: u64) -> Vec<String> {
    let mut s = State {
        baselined: true,
        last_seen_at: reading.last_at(),
        ctx: reading.tokens_used,
        model: reading.model.clone(),
        deadline: arm.map(|a| a.deadline).unwrap_or(0),
        ..State::default()
    };
    if let Some(l) = ledger {
        s.misses = l.misses.clone();
        s.last_ping = l.last_ping.clone();
        s.stopped = l.stopped.clone();
        s.pinged_at = l.pinged_at;
    }
    let mut lines = Vec::new();
    lines.push(format!("model       {}", if s.model.is_empty() { "not seen yet" } else { &s.model }));
    if s.last_seen_at == 0 {
        lines.push("state       no request yet this session".to_string());
    } else if s.is_cold(now) {
        lines.push(format!("state       COLD, last request {} ago", fmt_duration(now - s.last_seen_at)));
    } else {
        lines.push(format!("state       warm, {} left", fmt_duration(s.last_seen_at + TTL_SECS - now)));
    }
    lines.push(format!("context     {} tokens", s.ctx));
    lines.push(format!("cold cost   {} to re-write it (warm turn {})", fmt_usd(s.cold_usd()), fmt_usd(s.warm_usd())));
    let keepwarm = if s.deadline > now {
        let next = if s.pinged_at.is_some() {
            " · waiting for the wake's turn".to_string()
        } else if s.last_seen_at > 0 {
            format!(" · wake in {}", fmt_duration((s.last_seen_at + PING_AFTER_SECS).saturating_sub(now)))
        } else {
            String::new()
        };
        let last = s.last_ping.as_ref().map(|p| format!(" · last wake read {} {}", fmt_tok(p.read), fmt_usd(p.usd))).unwrap_or_default();
        format!("on, {} left{next}{last}", fmt_duration(s.deadline - now))
    } else if let Some(why) = &s.stopped {
        format!("stopped, {why}")
    } else {
        format!("off (attend keepwarm on to arm it for {})", fmt_duration(DEFAULT_WINDOW_SECS))
    };
    lines.push(format!("keepwarm    {keepwarm}"));
    if let Some(p) = price_of(&s.model) {
        let pings = (p.write_1h / p.read).floor() as u64;
        lines.push(format!(
            "break-even  up to {pings} wakes at the read rate cost one cold write, about {} of idle at one wake per {}",
            fmt_duration(pings * PING_AFTER_SECS),
            fmt_duration(PING_AFTER_SECS),
        ));
    }
    let paid = s.misses.iter().filter_map(|m| m.usd).fold(0.0_f64, |a, b| a + b);
    lines.push(format!(
        "session     {} cold write{} paid, {}",
        s.misses.len(),
        if s.misses.len() == 1 { "" } else { "s" },
        fmt_usd(Some(paid)),
    ));
    lines
}

// ── The sensor ─────────────────────────────────────────────────

pub struct KeepwarmSensor {
    session_id: String,
    dir: PathBuf,
    state: State,
}

impl KeepwarmSensor {
    /// `dir` is attend's keepwarm directory; the arm file and ledger for
    /// this session live there.
    pub fn new(session_id: String, dir: PathBuf) -> Self {
        Self { session_id, dir, state: State::default() }
    }
}

impl Sensor for KeepwarmSensor {
    fn name(&self) -> &str {
        "keepwarm"
    }

    sensor_trait::sensor_metadata!();

    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)> {
        let Some(reading) = read_context(&self.session_id, &focus.working_dir) else {
            return Vec::new();
        };
        let now = sensor_trait::epoch_secs();
        let arm = read_arm(&self.dir, &self.session_id);
        let out = self.state.step(&reading, arm.as_ref(), now);
        for line in &out.logs {
            eprintln!("[attend] keepwarm: {line}");
        }
        if let Some(deadline) = out.arm_write {
            let window = arm.as_ref().map(|a| a.window_secs).unwrap_or(0);
            let file = ArmFile { deadline, armed_at: now, window_secs: if deadline == 0 { 0 } else { deadline.saturating_sub(now).max(window) } };
            if let Err(e) = write_arm(&self.dir, &self.session_id, &file) {
                eprintln!("[attend] keepwarm: could not write the arm file: {e}");
            }
        }
        if out.changed {
            if let Err(e) = write_ledger(&self.dir, &self.session_id, &self.state.ledger(now)) {
                eprintln!("[attend] keepwarm: could not write the ledger: {e}");
            }
        }
        out.observations
    }

    fn emission_threshold(&self) -> f64 {
        3.0
    }

    fn base_interval(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(60)
    }

    fn decay_threshold(&self) -> u32 {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at: u64, read: u64, write: u64) -> UsageEntry {
        UsageEntry { at_epoch: at, model: "claude-fable-5-1".into(), input: 20, cache_read: read, cache_creation: write, output: 5 }
    }

    fn reading(entries: Vec<UsageEntry>) -> Reading {
        let tokens_used = entries.last().map(|e| e.context()).unwrap_or(0);
        Reading { tokens_used, model: "claude-fable-5-1".into(), usage_tail: entries }
    }

    fn armed(deadline: u64) -> ArmFile {
        ArmFile { deadline, armed_at: 0, window_secs: DEFAULT_WINDOW_SECS }
    }

    /// A baselined state at t=1000 with a 200k context and one request on record.
    fn warm_state() -> (State, Reading) {
        let r = reading(vec![entry(1000, 200_000, 500)]);
        let mut s = State::default();
        let out = s.step(&r, None, 1000);
        assert!(out.observations.is_empty());
        assert!(s.baselined);
        (s, r)
    }

    #[test]
    fn prices_match_family_longest_first() {
        assert_eq!(price_of("claude-fable-5-1").unwrap().read, 0.25);
        assert_eq!(price_of("claude-fable-5").unwrap().read, 1.0);
        assert_eq!(price_of("claude-sonnet-5").unwrap().write_1h, 4.0);
        assert_eq!(price_of("claude-sonnet-4-6").unwrap().write_1h, 6.0);
        assert!(price_of("gpt-9").is_none());
    }

    #[test]
    fn durations_parse_and_format() {
        assert_eq!(parse_duration("6h"), Some(21_600));
        assert_eq!(parse_duration("90m"), Some(5_400));
        assert_eq!(parse_duration("2h30m"), Some(9_000));
        assert_eq!(parse_duration("always"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(fmt_duration(21_600), "6h00m");
        assert_eq!(fmt_duration(35 * 60), "35m");
        assert_eq!(fmt_duration(50 * 3600), "2d 2h");
    }

    #[test]
    fn unarmed_session_never_wakes() {
        let (mut s, r) = warm_state();
        let out = s.step(&r, None, 1000 + PING_AFTER_SECS + 60);
        assert!(out.observations.is_empty());
        assert_eq!(out.arm_write, None);
    }

    #[test]
    fn armed_session_wakes_once_at_fifty_minutes() {
        let (mut s, r) = warm_state();
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        assert!(s.step(&r, Some(&arm), 1000 + PING_AFTER_SECS - 120).observations.is_empty());
        let now = 1000 + PING_AFTER_SECS + 30;
        let out = s.step(&r, Some(&arm), now);
        assert_eq!(out.observations.len(), 1);
        assert_eq!(out.observations[0].0, 3.0);
        assert!(out.observations[0].1.starts_with("keepwarm: 51m idle, the prompt cache lapses in 10m on 201k tokens."));
        assert!(out.observations[0].1.ends_with("Reply with one word and no tools."));
        assert_eq!(s.pinged_at, Some(now));
        // The next poll with no new turn stays silent.
        assert!(s.step(&r, Some(&arm), now + 60).observations.is_empty());
    }

    #[test]
    fn small_context_is_not_worth_a_wake() {
        let r = reading(vec![entry(1000, 30_000, 100)]);
        let mut s = State::default();
        s.step(&r, None, 1000);
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        assert!(s.step(&r, Some(&arm), 1000 + PING_AFTER_SECS + 30).observations.is_empty());
    }

    #[test]
    fn warm_verdict_keeps_the_window_and_resets_the_clock() {
        let (mut s, r) = warm_state();
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        let pinged = 1000 + PING_AFTER_SECS + 30;
        s.step(&r, Some(&arm), pinged);
        let r2 = reading(vec![entry(1000, 200_000, 500), entry(pinged + 20, 200_600, 60)]);
        let out = s.step(&r2, Some(&arm), pinged + 80);
        assert_eq!(s.pinged_at, None);
        assert!(s.last_ping.as_ref().unwrap().warm);
        assert_eq!(s.last_seen_at, pinged + 20);
        assert_eq!(out.arm_write, None);
        assert!(out.logs.iter().any(|l| l.starts_with("wake read 201k tokens from cache")));
        // A second idle stretch wakes again.
        let out = s.step(&r2, Some(&arm), pinged + 20 + PING_AFTER_SECS + 10);
        assert_eq!(out.observations.len(), 1);
    }

    #[test]
    fn cold_verdict_stops_and_disarms() {
        let (mut s, r) = warm_state();
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        let pinged = 1000 + PING_AFTER_SECS + 30;
        s.step(&r, Some(&arm), pinged);
        // The wake's turn rewrote the whole prefix.
        let r2 = reading(vec![entry(1000, 200_000, 500), entry(pinged + 20, 0, 200_600)]);
        let out = s.step(&r2, Some(&arm), pinged + 80);
        assert_eq!(out.arm_write, Some(0));
        assert_eq!(s.deadline, 0);
        assert!(s.stopped.as_ref().unwrap().contains("the cache was already gone"));
        // That rewrite is also a scored cold write.
        assert_eq!(s.misses.len(), 1);
        assert_eq!(s.misses[0].tokens, 200_600);
    }

    #[test]
    fn a_paid_cold_write_arms_three_hours() {
        let (mut s, _) = warm_state();
        let back = 1000 + TTL_SECS + 600;
        let r2 = reading(vec![entry(1000, 200_000, 500), entry(back, 0, 200_700)]);
        let out = s.step(&r2, None, back + 30);
        assert_eq!(s.misses.len(), 1);
        assert_eq!(s.misses[0].usd.map(|u| (u * 100.0).round() / 100.0), Some(4.01));
        assert_eq!(out.arm_write, Some(back + 30 + AUTO_WARM_SECS));
        assert!(out.logs[0].contains("cold write of 201k tokens paid ($4.01), keeping the cache warm for 3h00m"));
    }

    #[test]
    fn a_growing_context_is_not_a_cold_write() {
        let (mut s, _) = warm_state();
        let r2 = reading(vec![entry(1000, 200_000, 500), entry(1300, 200_500, 40_000)]);
        s.step(&r2, None, 1400);
        assert!(s.misses.is_empty());
        assert_eq!(s.ctx, 240_520);
    }

    #[test]
    fn arm_file_rules_the_deadline() {
        let (mut s, r) = warm_state();
        let arm = armed(5000);
        s.step(&r, Some(&arm), 1100);
        assert_eq!(s.deadline, 5000);
        let out = s.step(&r, None, 1200);
        assert_eq!(s.deadline, 0);
        assert!(out.changed);
        // Window end clears the file.
        s.step(&r, Some(&arm), 1300);
        let out = s.step(&r, Some(&arm), 5000);
        assert_eq!(out.arm_write, Some(0));
        assert!(out.logs.iter().any(|l| l == "window ended"));
    }

    #[test]
    fn a_wake_with_no_turn_stops_after_the_margin() {
        let (mut s, r) = warm_state();
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        let pinged = 1000 + PING_AFTER_SECS + 30;
        s.step(&r, Some(&arm), pinged);
        let out = s.step(&r, Some(&arm), pinged + TTL_SECS - PING_AFTER_SECS + 5);
        assert_eq!(out.arm_write, Some(0));
        assert!(s.stopped.as_ref().unwrap().contains("no turn"));
    }

    #[test]
    fn a_lapse_while_attend_was_down_stops_instead_of_waking_cold() {
        let (mut s, r) = warm_state();
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        let out = s.step(&r, Some(&arm), 1000 + TTL_SECS + 120);
        assert!(out.observations.is_empty());
        assert_eq!(out.arm_write, Some(0));
        assert!(s.stopped.as_ref().unwrap().starts_with("the cache lapsed 2m ago"));
    }

    #[test]
    fn card_reads_warm_armed_and_break_even() {
        let r = reading(vec![entry(1000, 200_000, 500)]);
        let arm = armed(1000 + DEFAULT_WINDOW_SECS);
        let lines = card(&r, Some(&arm), None, 1600);
        assert_eq!(lines[0], "model       claude-fable-5-1");
        assert_eq!(lines[1], "state       warm, 50m left");
        assert_eq!(lines[2], "context     200520 tokens");
        assert_eq!(lines[3], "cold cost   $4.01 to re-write it (warm turn $0.05)");
        assert_eq!(lines[4], "keepwarm    on, 5h50m left · wake in 40m");
        assert!(lines[5].starts_with("break-even  up to 80 wakes at the read rate cost one cold write, about 2d 18h of idle"));
        assert_eq!(lines[6], "session     0 cold writes paid, $0.00");
        let cold = card(&r, None, None, 1000 + TTL_SECS + 3600);
        assert_eq!(cold[1], "state       COLD, last request 2h00m ago");
        assert!(cold[4].starts_with("keepwarm    off (attend keepwarm on"));
    }

    #[test]
    fn arm_and_ledger_round_trip_through_files() {
        let dir = std::env::temp_dir().join(format!("keepwarm-test-{}", std::process::id()));
        let arm = ArmFile { deadline: 42, armed_at: 1, window_secs: 41 };
        write_arm(&dir, "sid", &arm).unwrap();
        assert_eq!(read_arm(&dir, "sid"), Some(arm));
        let ledger = Ledger { misses: vec![Miss { at: 1, tokens: 2, usd: Some(0.5) }], ..Ledger::default() };
        write_ledger(&dir, "sid", &ledger).unwrap();
        assert_eq!(read_ledger(&dir, "sid"), Some(ledger));
        assert_eq!(read_arm(&dir, "other"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reading_parses_ways_context_json() {
        let json = r#"{"tokens_used":193679,"model":"claude-fable-5-1","method":"api","usage_tail":[{"at":"2026-09-17T20:41:38.049Z","at_epoch":1789677698,"model":"claude-fable-5-1","input":32,"cache_read":193679,"cache_creation":486,"output":445,"tier":"1h"}]}"#;
        let r: Reading = serde_json::from_str(json).unwrap();
        assert_eq!(r.last_at(), 1789677698);
        assert_eq!(r.usage_tail[0].context(), 194_197);
    }
}
