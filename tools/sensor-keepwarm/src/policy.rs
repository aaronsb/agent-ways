//! The keepalive policy (ADR-414): in an interactive session with autocompact
//! off, an idle session is kept warm from every turn's end, one `keepwarm:`
//! wake `margin` before its cache would lapse, counted from the operator's
//! last prompt and allowed only inside the configured windows. At the first
//! due point past the count or outside the windows the session gets one
//! `wrap:` turn. With `after_wrap_keepalives` the keepalives go on that
//! many more times after the wrap; then the cache lapses, or with
//! `after_wrap: compact` the session gets one compaction with the focus the
//! wrap's closing `/compact` line gave. Nothing more follows until the
//! operator prompts again.
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

/// `mods.attend.keepalive.max_keepalives`.
pub const DEFAULT_MAX: u32 = 8;
pub const MAX_RANGE: (u32, u32) = (0, 48);
/// `mods.attend.keepalive.margin_minutes`: ADR-182's 50-minute point on the
/// hour tier.
pub const DEFAULT_MARGIN_MIN: u64 = 10;
pub const MARGIN_RANGE: (u64, u64) = (3, 30);
/// `mods.attend.keepalive.after_wrap_keepalives`.
pub const DEFAULT_AFTER_MAX: u32 = 0;
pub const AFTER_RANGE: (u32, u32) = (0, 48);
/// `mods.attend.keepalive.wrap_skill`: the skill's name and its arguments.
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
/// `mods.attend.keepalive.wrap_file`.
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
    /// When the compaction after the wrap was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<u64>,
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
    /// The skills the session lists, as the host read them. Not a setting:
    /// the caller fills it.
    pub session_skills: SessionSkills,
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
            session_skills: SessionSkills::NotChecked,
        }
    }
}

impl Config {
    /// From `ways settings list mods.attend --json --effective`. A key that
    /// is absent, or of the wrong type or out of range, keeps its default,
    /// except `enabled`, which fails closed: only an explicit true runs the
    /// policy. No document, or one that does not parse, is the policy off.
    pub fn from_settings(doc: Option<&str>) -> Config {
        let d = Config::default();
        let Some(v) = doc.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else {
            return Config { settings_ok: false, enabled: false, ..d };
        };
        let key = |k: &str| v.get("keys").and_then(|keys| keys.get(format!("mods.attend.{k}"))).and_then(|e| e.get("value")).cloned();
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
            after_max: int("keepalive.after_wrap_keepalives")
                .filter(|n| (AFTER_RANGE.0 as u64..=AFTER_RANGE.1 as u64).contains(n))
                .map_or(d.after_max, |n| n as u32),
            wrap_skill: text("keepalive.wrap_skill").map_or(d.wrap_skill, |s| WrapSkill::parse(&s)),
            session_skills: SessionSkills::NotChecked,
        }
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
        let mut c = self.clone();
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

    /// Why the policy does not run with `gate`, or `None` when it runs.
    pub fn why_off(&self, gate: Gate) -> Option<&'static str> {
        if !self.settings_ok {
            Some("the mods.attend settings could not be read")
        } else if !self.keepwarm_on {
            Some("mods.attend.keepwarm is off")
        } else if !self.enabled {
            Some("mods.attend.keepalive.enabled is false")
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
    /// Compact the conversation between turns; `text` is the focus.
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
    /// when it is empty; `#name` is a channel.
    pub to: String,
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
    let mut body = format!(
        "The operator is away. {}: square the task list and write the continuation prompt. Ask no questions, land nothing and start no new work. {}",
        cfg.wrap_skill.ask(),
        after_wrap_line(cfg),
    );
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
        after_keepalives: cfg.after_max,
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
    state.policy.wrapped_at = Some(now);
    let p = state.policy.clone();
    let r = route(cfg, site, now);
    let mut d = Decision::new(cfg, &p, true, "wrapped", if at_bound { "the count is spent" } else { "outside the keepalive hours" });
    d.action = Action::Wrap;
    d.text = Some(wrap_text(cfg, site, &p, &r, at_bound, now));
    // The wrap's turn restarts the clock; with keepalives after it, ask
    // again when the first of them could be due.
    if cfg.after_max > 0 {
        d.next_at = Some(now + ttl.saturating_sub(cfg.margin_secs));
        d.estimate(cfg, &p, now + ttl.saturating_sub(cfg.margin_secs), ttl);
    }
    d.wrap = Some(r);
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
    lines
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
