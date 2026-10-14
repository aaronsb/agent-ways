//! The keepalive policy (ADR-414): in an interactive session with autocompact
//! off, an idle session is kept warm from every turn's end, one `keepwarm:`
//! wake `margin` before its cache would lapse, counted from the operator's
//! last prompt and allowed only inside the configured windows. At the first
//! due point past the count or outside the windows the session gets one
//! `wrap:` turn, and nothing more until the operator prompts again.
//!
//! The judgment lives here; the attend mod supplies what only it can see
//! (the operator's prompts, through [`reset`], and the autocompact gate) and
//! submits what [`step`] returns. The count and its marks ride the keepwarm
//! ledger ([`Policy`]), so the plain check and the sensor keep them.

use crate::windows::Windows;
use crate::{
    cache_lapsed, fmt_duration, fmt_usd, wake_line, ArmFile, Outcome, Reading, State, BIG_TOKENS, PING_AFTER_SECS, TTL_SECS,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `attend_mod.keepalive.max_keepalives`.
pub const DEFAULT_MAX: u32 = 8;
pub const MAX_RANGE: (u32, u32) = (0, 48);
/// `attend_mod.keepalive.margin_minutes`: ADR-182's 50-minute point on the
/// hour tier.
pub const DEFAULT_MARGIN_MIN: u64 = 10;
pub const MARGIN_RANGE: (u64, u64) = (3, 30);
/// `attend_mod.keepalive.wrap_file`.
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
}

impl Policy {
    pub fn is_empty(&self) -> bool {
        *self == Policy::default()
    }

    /// Where the count runs from: the operator's prompt, else the stretch.
    fn since(&self) -> Option<u64> {
        self.operator_at.or(self.stretch_from)
    }
}

/// The operator prompted (`attend keepwarm reset`): the count starts again,
/// and a sent wrap and a stop are cleared.
pub fn reset(p: &mut Policy, now: u64) {
    *p = Policy { operator_at: Some(now), gate: p.gate.take(), ..Policy::default() };
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

/// The `attend_mod` keys the policy reads (ADR-414 item 8).
#[derive(Clone, Debug)]
pub struct Config {
    /// The settings were read. Unread, the policy is off: `enabled` fails
    /// closed (ADR-414 item 8).
    pub settings_ok: bool,
    /// `attend_mod.keepwarm`: off turns off every cache wake the mod drives.
    pub keepwarm_on: bool,
    pub enabled: bool,
    pub max: u32,
    pub margin_secs: u64,
    pub windows: Windows,
    pub wrap: WrapDest,
    pub wrap_file: String,
    pub wrap_to: String,
    pub after_wrap: AfterWrap,
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
        }
    }
}

impl Config {
    /// From `ways settings list attend_mod --json --effective`. A key that
    /// is absent, or of the wrong type or out of range, keeps its default,
    /// except `enabled`, which fails closed: only an explicit true runs the
    /// policy. No document, or one that does not parse, is the policy off.
    pub fn from_settings(doc: Option<&str>) -> Config {
        let d = Config::default();
        let Some(v) = doc.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else {
            return Config { settings_ok: false, enabled: false, ..d };
        };
        let key = |k: &str| v.get("keys").and_then(|keys| keys.get(format!("attend_mod.{k}"))).and_then(|e| e.get("value")).cloned();
        let text = |k: &str| key(k).and_then(|x| x.as_str().map(str::to_string));
        let int = |k: &str| key(k).and_then(|x| x.as_u64());
        let windows = match key("keepalive.windows") {
            Some(serde_json::Value::Array(items)) => {
                Some(items.iter().map(|i| i.as_str().map_or_else(|| i.to_string(), str::to_string)).collect::<Vec<_>>())
            }
            _ => None,
        };
        let zone = text("keepalive.timezone").unwrap_or_default();
        Config {
            settings_ok: true,
            keepwarm_on: text("keepwarm").as_deref() != Some("off"),
            enabled: key("keepalive.enabled").and_then(|x| x.as_bool()) == Some(true),
            max: int("keepalive.max_keepalives")
                .filter(|n| (MAX_RANGE.0 as u64..=MAX_RANGE.1 as u64).contains(n))
                .map_or(d.max, |n| n as u32),
            margin_secs: int("keepalive.margin_minutes").filter(|n| (MARGIN_RANGE.0..=MARGIN_RANGE.1).contains(n)).map_or(d.margin_secs, |n| n * 60),
            windows: match windows {
                Some(w) => Windows::parse(&w, &zone),
                None => Windows::parse(&[crate::windows::DEFAULT_WINDOW.to_string()], &zone),
            },
            wrap: text("keepalive.wrap").and_then(|s| WrapDest::parse(&s)).unwrap_or(d.wrap),
            wrap_file: text("keepalive.wrap_file").filter(|s| !s.trim().is_empty()).unwrap_or(d.wrap_file),
            wrap_to: text("keepalive.wrap_to").unwrap_or_default().trim().to_string(),
            after_wrap: match text("keepalive.after_wrap").as_deref() {
                Some("compact") => AfterWrap::Compact,
                _ => d.after_wrap,
            },
        }
    }

    /// Why the policy does not run with `gate`, or `None` when it runs.
    pub fn why_off(&self, gate: Gate) -> Option<&'static str> {
        if !self.settings_ok {
            Some("the attend_mod settings could not be read")
        } else if !self.keepwarm_on {
            Some("attend_mod.keepwarm is off")
        } else if !self.enabled {
            Some("attend_mod.keepalive.enabled is false")
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
                let note = format!("wrap_file \"{template}\" refused: {why}; the reply goes to {}", default.display());
                (default.to_string_lossy().into_owned(), Some(note))
            }
            Err(also) => (String::new(), Some(format!("wrap_file \"{template}\" refused: {why}; the default {} is refused too: {also}", default.display()))),
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
    let base = normalize(&state_root.join("wrap")).ok_or("the state root climbs above the root")?;
    let path = normalize(path).ok_or("it climbs above the root")?;
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

/// Writes `reply` to `path` for session `session_id`, checking the path
/// again first ([`check_wrap_path`]): through a temp file created new
/// beside it and renamed over it, so a symlink planted at the path is
/// replaced, not followed.
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
    let tmp = dir.join(format!(".{}.tmp.{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), std::process::id()));
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
    notes
}

/// `none`, `keepalive` or `wrap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    None,
    Keepalive,
    Wrap,
}

/// Where the wrap's reply goes, as the mod routes it (ADR-414 item 6).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WrapRoute {
    pub dest: &'static str,
    /// The expanded `wrap_file`: the destination of `file`, and the
    /// fallback of a refused `send`.
    pub file: String,
    /// The recipient of `send`: `wrap_to`, or the operator's member id
    /// when it is empty; `#name` is a channel.
    pub to: String,
    pub after: &'static str,
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
    /// The turn to submit, for `keepalive` and `wrap`.
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
    /// `running`, `wrapped`, `stopped`, or `off` where the policy does not run.
    pub state: &'static str,
    pub wraps_at_hm: Option<String>,
    pub wrapped_at_hm: Option<String>,
    pub wrap: Option<WrapRoute>,
    /// The judgment could not be made (the transcript was unreadable): ask
    /// again at `next_at`, a bounded number of times.
    pub retry: bool,
}

impl Decision {
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
            wrap: None,
            retry: false,
        }
    }
}

/// The due point of the stretch `reading` ends with, that stretch's TTL and
/// its last request, or why there is none: no request yet, the five-minute
/// tier, a context under the floor.
fn due_point(state: &State, reading: &Reading, cfg: &Config) -> Result<(u64, u64, u64), &'static str> {
    let last = match reading.usage_tail.last() {
        Some(l) if l.at_epoch > 0 => l,
        _ => return Err("no request yet in this session"),
    };
    if last.ttl_secs() < TTL_SECS {
        return Err("the session is on the five-minute cache tier");
    }
    if state.ctx < BIG_TOKENS {
        return Err("the context is under 50k tokens, where a cold re-write is cheap");
    }
    let ttl = last.ttl_secs();
    Ok((last.at_epoch + ttl.saturating_sub(cfg.margin_secs), ttl, last.at_epoch))
}

/// When the wrap goes out if nothing else happens: the first due point,
/// from `due` on, past the count or outside the windows. `None` with no
/// wrap.
fn wraps_at(cfg: &Config, count: u32, due: u64, ttl: u64) -> Option<u64> {
    if cfg.wrap == WrapDest::Off {
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

/// The wrap turn's text (ADR-414 item 6).
fn wrap_text(cfg: &Config, site: &Site, p: &Policy, route: &WrapRoute, at_bound: bool, now: u64) -> String {
    let since = p.since().map_or_else(|| "a long idle stretch".to_string(), |t| format!("{} since the operator's last prompt", fmt_duration(now.saturating_sub(t))));
    let first = if at_bound {
        format!("wrap: {since} and {} keepalives, the most this session allows.", p.count)
    } else if cfg.windows.list.is_empty() {
        format!("wrap: {since}, and no keepalive hours are set.")
    } else {
        format!("wrap: {since}, and the keepalive hours ({}) have ended.", cfg.windows.describe())
    };
    let mut body = "The operator is away. Run the wrap skill with the argument stay: square the task list and write the continuation prompt. Ask no questions, land nothing and start no new work. Nothing keeps the prompt cache warm after this turn.".to_string();
    match cfg.wrap {
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
        after: cfg.after_wrap.as_str(),
        handoff: format!(
            "handoff: continuation prompt from {} ({}), wrapped at {} while its operator was away.",
            site.name,
            site.project_name(),
            cfg.windows.hm(now),
        ),
    }
}

/// One judgment (ADR-414 item 7). Records `gate`; where the policy does not
/// run it is the plain check's judgment (ADR-182 as ADR-406 item 13 has
/// it). The returned outcome is settled by the caller as the plain check's
/// is.
pub fn step(state: &mut State, reading: &Reading, arm: Option<&ArmFile>, cfg: &Config, gate: Gate, site: &Site, now: u64) -> (Outcome, Decision) {
    state.policy.gate = Some(gate.as_str().into());
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
    if let Some(t) = p.wrapped_at {
        return (out, Decision::new(cfg, &p, true, "wrapped", format!("wrapped at {}; nothing more until the operator's next prompt", cfg.windows.hm(t))));
    }
    if let Some(why) = &p.stopped {
        return (out, Decision::new(cfg, &p, true, "stopped", format!("stopped: {why}; nothing more until the operator's next prompt")));
    }
    if let Some(t) = state.pinged_at {
        let mut d = Decision::new(cfg, &p, true, "running", "waiting for the keepalive's turn");
        d.next_at = Some(t + VERDICT_SECS + 1);
        if let Ok((_, ttl, _)) = due_point(state, reading, cfg) {
            d.wraps_at = wraps_at(cfg, p.count, t + ttl.saturating_sub(cfg.margin_secs), ttl);
            d.wraps_at_hm = d.wraps_at.map(|w| cfg.windows.hm(w));
        }
        return (out, d);
    }
    let (due, ttl, last_at) = match due_point(state, reading, cfg) {
        Ok(x) => x,
        Err(why) => return (out, Decision::new(cfg, &p, true, "running", why)),
    };
    if now < due {
        let mut d = Decision::new(cfg, &p, true, "running", format!("next due at {}", cfg.windows.hm(due)));
        d.next_at = Some(due);
        d.wraps_at = wraps_at(cfg, p.count, due, ttl);
        d.wraps_at_hm = d.wraps_at.map(|w| cfg.windows.hm(w));
        return (out, d);
    }
    let mut out = out;
    out.changed = true;
    if cache_lapsed(Some(reading), now) {
        let why = format!("the cache had lapsed at the due point {} (the host may have slept)", cfg.windows.hm(due));
        state.policy.stopped = Some(why);
        let p = state.policy.clone();
        let reason = format!("stopped: {}; nothing more until the operator's next prompt", p.stopped.as_deref().unwrap_or_default());
        return (out, Decision::new(cfg, &p, true, "stopped", reason));
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
        d.wraps_at = wraps_at(cfg, p.count, now + ttl.saturating_sub(cfg.margin_secs), ttl);
        d.wraps_at_hm = d.wraps_at.map(|w| cfg.windows.hm(w));
        return (out, d);
    }
    if cfg.wrap == WrapDest::Off {
        state.policy.stopped = Some(if at_bound {
            format!("{} keepalives, the most this session allows, and wrap is off", state.policy.count)
        } else {
            "the keepalive hours have ended, and wrap is off".to_string()
        });
        let p = state.policy.clone();
        let reason = format!("stopped: {}; nothing more until the operator's next prompt", p.stopped.as_deref().unwrap_or_default());
        return (out, Decision::new(cfg, &p, true, "stopped", reason));
    }
    state.policy.wrapped_at = Some(now);
    let p = state.policy.clone();
    let r = route(cfg, site, now);
    let mut d = Decision::new(cfg, &p, true, "wrapped", if at_bound { "the count is spent" } else { "outside the keepalive hours" });
    d.action = Action::Wrap;
    d.text = Some(wrap_text(cfg, site, &p, &r, at_bound, now));
    d.wrap = Some(r);
    (out, d)
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
    let mut lines = Vec::new();
    let Some(gate) = p.gate.as_deref() else {
        lines.push("keepalive   not run in this session (the attend mod runs it in an interactive session while autocompact is off)".into());
        return lines;
    };
    if let Some(why) = cfg.why_off(Gate::parse(gate)) {
        let tail = if cfg.keepwarm_on { " (armed windows only)" } else { "" };
        lines.push(format!("keepalive   off: {why}{tail}"));
        return lines;
    }
    let since = match (p.operator_at, p.stretch_from) {
        (Some(t), _) => format!(" since your prompt at {}", cfg.windows.hm(t)),
        (None, Some(t)) => format!(" since {}", cfg.windows.hm(t)),
        (None, None) => String::new(),
    };
    if let Some(t) = p.wrapped_at {
        let outcome = p.wrap_outcome.clone().unwrap_or_else(|| "no answer recorded yet".into());
        lines.push(format!("keepalive   wrapped at {} after {} keepalives{since} · {outcome}", cfg.windows.hm(t), p.count));
    } else if let Some(why) = &p.stopped {
        lines.push(format!("keepalive   stopped: {why}; your next prompt starts it again"));
    } else {
        let mut line = format!("keepalive   {} of {}{since}", p.count, cfg.max);
        if state.deadline > now {
            line.push_str(&format!(" · an armed window overrides it for {}", fmt_duration(state.deadline - now)));
        }
        match due_point(state, reading, cfg) {
            Ok((due, ttl, _)) => {
                let due = match state.pinged_at {
                    Some(t) => t + ttl.saturating_sub(cfg.margin_secs),
                    None => due,
                };
                if state.pinged_at.is_some() {
                    line.push_str(" · waiting for the keepalive's turn");
                } else {
                    line.push_str(&format!(" · next in {}", fmt_duration(due.saturating_sub(now))));
                }
                if let Some(w) = wraps_at(cfg, p.count, due, ttl) {
                    line.push_str(&format!(" · wraps about {}", cfg.windows.hm(w)));
                }
            }
            Err(why) => line.push_str(&format!(" · idle: {why}")),
        }
        line.push_str(&format!(" · windows {}", cfg.windows.describe()));
        lines.push(line);
    }
    for (entry, why) in &cfg.windows.dropped {
        lines.push(format!("keepalive   window entry \"{entry}\" dropped: {why}"));
    }
    if let Some(z) = &cfg.windows.bad_zone {
        lines.push(format!("keepalive   timezone \"{z}\" is unknown; the host's zone is used"));
    }
    if cfg.wrap != WrapDest::Off {
        for n in route_notes(cfg, site, now) {
            lines.push(format!("keepalive   {n}"));
        }
    }
    let wakes = if p.count == 1 { "wake" } else { "wakes" };
    let left = cfg.max.saturating_sub(p.count) as f64;
    let more = state.warm_usd().map(|w| w * left);
    let tail = if cfg.wrap == WrapDest::Off {
        format!("at most {} more in wakes before the bound", fmt_usd(more))
    } else {
        format!("at most {} more in wakes before the wrap, and the wrap reads the context once per tool round", fmt_usd(more))
    };
    lines.push(format!(
        "cost        {} {wakes} {} so far · {tail} · a cold re-write now {}",
        p.count,
        fmt_usd(Some(p.spent_usd)),
        fmt_usd(state.cold_usd()),
    ));
    lines
}

#[cfg(test)]
mod tests;
