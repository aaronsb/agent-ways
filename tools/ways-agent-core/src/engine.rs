//! The session's judge engine (ADR-708 §2).
//!
//! Each session resolves its engine once, at its first judge request, and
//! holds it for every lane the gate covers. The resolution is a pure function
//! of `gate.engine`, the profiles, whether a Claude Code judge host has
//! registered, and which providers have a key; the caller stores it. No job
//! falls back from one engine to another, so no candidate is judged twice.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::{self, Profile, Provider};

/// The engine was named in `gate.engine` and can run.
pub const EXPLICIT: &str = "explicit";
/// `gate.engine` is unset; the first shipped profile with a key was taken.
pub const KEY_FOUND: &str = "key found";
/// `claude-code` was named and no host registered; a key engine was taken.
pub const NO_HOST: &str = "claude-code unavailable: no Claude Code host";
/// The prefix of a named engine that cannot run: the session has no judge,
/// and each lane logs this as its `gate_fallback` reason (ADR-708 §7).
pub const ENGINE_UNAVAILABLE: &str = "engine_unavailable";
/// `gate.engine` is unset and no provider has a key: the gate is off, as
/// before ADR-708.
pub const NO_KEY: &str = "no key";
/// The prefix of a configuration error: the gate's `config:` fallback.
pub const CONFIG: &str = "config";

/// What an unavailable engine's reason tells the operator to do.
const REMEDY: &str = "configure an Anthropic or OpenRouter key (`ways agent key add --provider anthropic` or `--provider openrouter`)";

/// A session's engine and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    /// The profile the session judges with; `None` when it has none.
    pub engine: Option<String>,
    /// [`EXPLICIT`], [`KEY_FOUND`] or [`NO_HOST`] with an engine; with none,
    /// [`NO_KEY`], a reason that starts with [`ENGINE_UNAVAILABLE`] and
    /// names the remedy, or one that starts with [`CONFIG`] for an engine
    /// that names no profile.
    pub reason: String,
}

impl Resolution {
    fn engine(name: &str, reason: &str) -> Resolution {
        Resolution { engine: Some(name.to_string()), reason: reason.to_string() }
    }

    fn unavailable(why: String) -> Resolution {
        Resolution { engine: None, reason: format!("{ENGINE_UNAVAILABLE}: {why}") }
    }

    /// True when a named engine cannot run, as opposed to a gate with no
    /// engine named and no key.
    pub fn is_unavailable(&self) -> bool {
        self.engine.is_none() && self.reason.starts_with(ENGINE_UNAVAILABLE)
    }
}

/// Resolves a session's engine from `engine` (`gate.engine`, `None` when
/// unset), the profiles after the user's patches, whether a Claude Code judge
/// host registered for the session, and which providers have a key.
///
/// - A profile whose provider is `claude-code` runs with a host. Without one
///   the session takes the first shipped key engine, anthropic then
///   openrouter, with reason [`NO_HOST`]; with no key either it has none.
/// - A key engine named runs when its provider has a key; else it has none.
/// - Unset keeps the rule from before: the first shipped profile with a key.
///   The host never changes it.
pub fn resolve_session(
    engine: Option<&str>,
    profiles: &BTreeMap<String, Profile>,
    host: bool,
    has_key: impl Fn(Provider) -> bool,
) -> Resolution {
    let first_key = || profile::first_with_key(profiles, &has_key);
    let Some(name) = engine else {
        return match first_key() {
            Some(found) => Resolution::engine(found, KEY_FOUND),
            None => Resolution { engine: None, reason: NO_KEY.to_string() },
        };
    };
    let Some(p) = profiles.get(name) else {
        // A configuration error, as `profile::resolve` reports it.
        return Resolution { engine: None, reason: format!("{CONFIG}: {}", profile::names_no_profile(name)) };
    };
    match p.provider {
        Provider::ClaudeCode if host => Resolution::engine(name, EXPLICIT),
        Provider::ClaudeCode => match first_key() {
            Some(found) => Resolution::engine(found, NO_HOST),
            None => Resolution::unavailable(format!("{NO_HOST}, and no key is configured; load the ways mod or {REMEDY}")),
        },
        provider if has_key(provider) => Resolution::engine(name, EXPLICIT),
        provider => Resolution::unavailable(format!(
            "engine '{name}' has no {provider} key; add one with `ways agent key add --provider {provider}`, or name another engine"
        )),
    }
}

/// The file a session's resolution is published in, in its judge state
/// directory.
pub const RESOLUTION_FILE: &str = "resolution.json";

/// The root of every session's judge state, one directory per session id:
/// `$XDG_STATE_HOME/agent-ways/judge`. It outlives the session's runtime
/// directory, since a session holds its engine for its whole life.
pub fn judge_state_root() -> PathBuf {
    ways_core::paths::state_root().join("judge")
}

/// A session's resolved engine, as its resolution file holds it: the
/// resolution, and the provider of the engine it names, so a reader can tell
/// whether the session judges inside Claude Code without reading the
/// profiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEngine {
    #[serde(flatten)]
    pub resolution: Resolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
}

impl SessionEngine {
    /// True when the session judges inside Claude Code: the host claims its
    /// jobs. Otherwise the host stands by for the session's life.
    pub fn is_claude_code(&self) -> bool {
        self.provider == Some(Provider::ClaudeCode)
    }

    /// The resolution in the session judge state directory `dir`, once one
    /// was written; `None` before, or when the file does not parse.
    pub fn read(dir: &Path) -> Option<SessionEngine> {
        let text = std::fs::read_to_string(dir.join(RESOLUTION_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// The file whose presence says a Claude Code judge host registered for the
/// session, in its judge state directory.
pub const HOST_FILE: &str = "host";

/// A running Claude Code session and its engine. Only a resolution that
/// names an engine is held in the session's file; a session with none
/// resolves again at each request, so its `engine` is that resolution,
/// computed now as the gate computes it. `engine` is `None` while the
/// session would resolve to an engine and has made no judge request.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSession {
    pub session: String,
    pub engine: Option<SessionEngine>,
}

impl LiveSession {
    /// The session id as the screens show it: its first eight characters.
    pub fn short_id(&self) -> String {
        self.session.chars().take(8).collect()
    }
}

/// The sessions a running Claude Code process holds, each with its engine
/// (ADR-708 §8): those Claude Code's session records name, whose process is
/// alive. A session with no held engine is resolved as the gate resolves it,
/// from agent.yaml, the key files and its host registration.
pub fn live_sessions() -> Vec<LiveSession> {
    let records = ways_core::paths::claude_dir().session_records();
    let live = records.iter().filter(|r| process_alive(r.pid)).map(|r| r.session_id.as_str());
    let user = profile::UserLayer::load_with_findings(&profile::user_layer_path()).map(|(u, _)| u).unwrap_or_default();
    let profiles = profile::profiles(&user);
    let has_key = |p| crate::keys::locate_file(p).is_some();
    live_sessions_in(&judge_state_root(), live, |host| {
        if user.mode == Some(profile::Mode::Off) {
            // The gate judges nothing, so no session resolves.
            return Resolution { engine: None, reason: OFF.to_string() };
        }
        resolve_session(user.engine.as_deref(), &profiles, host, has_key)
    })
}

/// The reason a session has no engine while `gate.mode` is off.
pub const OFF: &str = "gate.mode off";

/// [`live_sessions`] for the session ids `live`: each one's held resolution
/// under `root`, or, with none held, `resolve` given whether a host
/// registered, kept when it names no engine. An id that is not a plain
/// directory name is left out, and each id is listed once, in id order, so a
/// list reads the same however the records were found.
pub fn live_sessions_in<'a>(root: &Path, live: impl IntoIterator<Item = &'a str>, resolve: impl Fn(bool) -> Resolution) -> Vec<LiveSession> {
    let plain = |id: &str| {
        let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        id.chars().next().is_some_and(ok) && id.chars().all(|c| ok(c) || c == '.')
    };
    let ids: BTreeSet<&str> = live.into_iter().filter(|id| plain(id)).collect();
    ids.into_iter()
        .map(|id| {
            let dir = root.join(id);
            let engine = SessionEngine::read(&dir).or_else(|| {
                let now = resolve(dir.join(HOST_FILE).is_file());
                now.engine.is_none().then_some(SessionEngine { resolution: now, provider: None })
            });
            LiveSession { session: id.to_string(), engine }
        })
        .collect()
}

/// Whether a process holds `pid`. Off Unix a session record is taken at its
/// word: Claude Code removes it when the session ends.
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
        // SAFETY: signal 0 delivers nothing; the call only checks the pid.
        let ret = unsafe { libc::kill(pid, 0) };
        ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// An engine as the operator knows it: its profile name with the profile's
/// display name, such as `claude-code (Claude Code internal)`; a profile of
/// your own has no display name, and no engine reads `no engine`.
pub fn engine_label(name: Option<&str>, profiles: &BTreeMap<String, Profile>) -> String {
    let Some(name) = name else { return "no engine".to_string() };
    match profiles.get(name).and_then(|p| p.display_name.as_deref()) {
        Some(shown) => format!("{name} ({shown})"),
        None => name.to_string(),
    }
}

/// One live session as `ways status` and `ways agent status` list it: the
/// short id, the engine, and the reason, which for an unavailable engine
/// names the remedy (ADR-708 §7).
pub fn session_line(s: &LiveSession, profiles: &BTreeMap<String, Profile>) -> String {
    match &s.engine {
        None => format!("{}  not resolved yet: no judge request so far", s.short_id()),
        Some(e) => format!("{}  {} — {}", s.short_id(), engine_label(e.resolution.engine.as_deref(), profiles), e.resolution.reason),
    }
}

/// What an unset `gate.engine` resolves to now (ADR-708 §8): the first
/// shipped profile whose provider has a key file. `claude-code` is chosen
/// only by name, so it is never the answer.
pub fn unset_engine_line(profiles: &BTreeMap<String, Profile>, has_key: impl Fn(Provider) -> bool) -> String {
    match profile::first_with_key(profiles, has_key) {
        Some(name) => engine_label(Some(name), profiles),
        None => "none: no Anthropic or OpenRouter key file, so an unset engine leaves the gate off".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::shipped;

    fn only(p: Provider) -> impl Fn(Provider) -> bool {
        move |q| q == p
    }

    #[test]
    fn claude_code_with_no_host_takes_the_anthropic_key() {
        let r = resolve_session(Some("claude-code"), &shipped(), false, only(Provider::Anthropic));
        assert_eq!(r, Resolution { engine: Some("anthropic".into()), reason: NO_HOST.into() });
        assert_eq!(r.reason, "claude-code unavailable: no Claude Code host");
    }

    #[test]
    fn claude_code_with_no_host_tries_anthropic_then_openrouter() {
        let both = resolve_session(Some("claude-code"), &shipped(), false, |_| true);
        assert_eq!(both.engine.as_deref(), Some("anthropic"));
        let r = resolve_session(Some("claude-code"), &shipped(), false, only(Provider::Openrouter));
        assert_eq!((r.engine.as_deref(), r.reason.as_str()), (Some("openrouter"), NO_HOST));
    }

    #[test]
    fn claude_code_with_a_host_runs_there_whatever_keys_exist() {
        for keys in [false, true] {
            let r = resolve_session(Some("claude-code"), &shipped(), true, |_| keys);
            assert_eq!(r, Resolution { engine: Some("claude-code".into()), reason: EXPLICIT.into() }, "keys {keys}");
        }
    }

    #[test]
    fn claude_code_with_no_host_and_no_key_is_unavailable_and_names_the_remedy() {
        let r = resolve_session(Some("claude-code"), &shipped(), false, |_| false);
        assert_eq!(r.engine, None);
        assert!(r.is_unavailable(), "{r:?}");
        assert!(r.reason.starts_with("engine_unavailable: claude-code unavailable: no Claude Code host"), "{}", r.reason);
        assert!(r.reason.contains("Anthropic or OpenRouter key"), "{}", r.reason);
    }

    #[test]
    fn unset_with_a_host_keeps_the_key_rule() {
        let r = resolve_session(None, &shipped(), true, only(Provider::Anthropic));
        assert_eq!(r, Resolution { engine: Some("anthropic".into()), reason: KEY_FOUND.into() });
        // Every key present: still anthropic, never claude-code.
        assert_eq!(resolve_session(None, &shipped(), true, |_| true).engine.as_deref(), Some("anthropic"));
        let r = resolve_session(None, &shipped(), true, only(Provider::Openrouter));
        assert_eq!(r.engine.as_deref(), Some("openrouter"));
    }

    #[test]
    fn unset_with_no_key_is_off_not_unavailable() {
        for host in [false, true] {
            let r = resolve_session(None, &shipped(), host, |_| false);
            assert_eq!(r, Resolution { engine: None, reason: NO_KEY.into() }, "host {host}");
            assert!(!r.is_unavailable());
        }
    }

    #[test]
    fn a_named_key_engine_runs_on_its_key_and_ignores_the_host() {
        for host in [false, true] {
            let r = resolve_session(Some("openrouter"), &shipped(), host, |_| true);
            assert_eq!(r, Resolution { engine: Some("openrouter".into()), reason: EXPLICIT.into() }, "host {host}");
        }
        // No key for it: unavailable, never another engine.
        let r = resolve_session(Some("openrouter"), &shipped(), true, only(Provider::Anthropic));
        assert_eq!(r.engine, None);
        assert!(r.is_unavailable() && r.reason.contains("openrouter"), "{}", r.reason);
    }

    #[test]
    fn a_profile_of_your_own_follows_its_provider() {
        let user: crate::profile::UserLayer = serde_yaml::from_str(
            "profiles:\n  cc-sonnet:\n    provider: claude-code\n    model: sonnet\n  mine:\n    provider: anthropic\n    model: claude-sonnet-5-5\n",
        )
        .unwrap();
        let all = crate::profile::profiles(&user);
        assert_eq!(resolve_session(Some("cc-sonnet"), &all, true, |_| false).engine.as_deref(), Some("cc-sonnet"));
        let r = resolve_session(Some("cc-sonnet"), &all, false, only(Provider::Anthropic));
        assert_eq!((r.engine.as_deref(), r.reason.as_str()), (Some("anthropic"), NO_HOST));
        assert_eq!(resolve_session(Some("mine"), &all, false, only(Provider::Anthropic)).engine.as_deref(), Some("mine"));
        // A name with no profile is a configuration error, in the words
        // `profile::resolve` fails with and the gate logs as `config:`.
        let r = resolve_session(Some("nosuch"), &all, true, |_| true);
        assert_eq!((r.engine, r.reason.as_str()), (None, "config: engine 'nosuch' names no profile"));
        let user: crate::profile::UserLayer = serde_yaml::from_str("engine: nosuch\n").unwrap();
        let e = crate::profile::resolve(&user, |_| true).unwrap_err();
        assert_eq!(format!("{CONFIG}: {e:#}"), "config: engine 'nosuch' names no profile");
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-engine-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn resolved(root: &Path, id: &str, engine: Option<&str>, provider: Option<Provider>, reason: &str) {
        let e = SessionEngine { resolution: Resolution { engine: engine.map(str::to_string), reason: reason.into() }, provider };
        std::fs::create_dir_all(root.join(id)).unwrap();
        std::fs::write(root.join(id).join(RESOLUTION_FILE), serde_json::to_vec(&e).unwrap()).unwrap();
    }

    #[test]
    fn each_live_session_shows_its_engine_and_why() {
        let root = scratch("live");
        let fallen = resolve_session(Some("claude-code"), &shipped(), false, only(Provider::Anthropic));
        resolved(&root, "3f6c2a91-aaaa", Some("claude-code"), Some(Provider::ClaudeCode), EXPLICIT);
        resolved(&root, "bbbbbbbb-0000", fallen.engine.as_deref(), Some(Provider::Anthropic), &fallen.reason);
        // No engine is not held: cccccccc has no file and resolves now to
        // none; dddddddd has a host and resolves now to claude-code, which
        // it has not yet asked for.
        std::fs::create_dir_all(root.join("dddddddd-0000")).unwrap();
        std::fs::write(root.join("dddddddd-0000").join(HOST_FILE), "1").unwrap();
        let now = |host| resolve_session(Some("claude-code"), &shipped(), host, |_| false);
        let ids = ["dddddddd-0000", "3f6c2a91-aaaa", "bbbbbbbb-0000", "cccccccc-0000", "3f6c2a91-aaaa", "../x"];
        let live = live_sessions_in(&root, ids, now);
        assert_eq!(live.len(), 4, "each id once, in id order, and no path that is not an id: {live:?}");
        let lines: Vec<String> = live.iter().map(|s| session_line(s, &shipped())).collect();
        assert_eq!(lines[0], "3f6c2a91  claude-code (Claude Code internal) — explicit");
        assert_eq!(lines[1], "bbbbbbbb  anthropic (Anthropic API key) — claude-code unavailable: no Claude Code host");
        assert!(lines[2].starts_with("cccccccc  no engine — engine_unavailable: claude-code unavailable"), "{}", lines[2]);
        assert!(lines[2].contains("configure an Anthropic or OpenRouter key"), "{}", lines[2]);
        assert!(!lines.iter().any(|l| l.contains("--provider claude-code")), "{lines:?}");
        assert_eq!(lines[3], "dddddddd  not resolved yet: no judge request so far");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unset_engine_names_the_first_key_engine_never_claude_code() {
        assert_eq!(unset_engine_line(&shipped(), |_| true), "anthropic (Anthropic API key)");
        assert_eq!(unset_engine_line(&shipped(), only(Provider::Openrouter)), "openrouter (OpenRouter API key)");
        assert!(unset_engine_line(&shipped(), |_| false).starts_with("none: no Anthropic or OpenRouter key file"));
        assert_eq!(engine_label(Some("mine"), &shipped()), "mine");
    }

    #[test]
    fn a_resolution_round_trips_as_json() {
        let r = Resolution { engine: Some("anthropic".into()), reason: NO_HOST.into() };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"engine":"anthropic","reason":"claude-code unavailable: no Claude Code host"}"#);
        assert_eq!(serde_json::from_str::<Resolution>(&line).unwrap(), r);
    }
}
