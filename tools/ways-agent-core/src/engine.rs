//! The session's judge engine (ADR-708 §2).
//!
//! Each session resolves its engine once, at its first judge request, and
//! holds it for every lane the gate covers. The resolution is a pure function
//! of `gate.engine`, the profiles, whether a Claude Code judge host has
//! registered, and which providers have a key; the caller stores it. No job
//! falls back from one engine to another, so no candidate is judged twice.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::{self, Mode, Profile, Provider, Settings, UserLayer};

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

/// Whether `session_id` names one directory directly under a root of
/// per-session state: no separator, no leading dot, not empty. Every path
/// built from a session id, from a hook payload, the command line or a
/// session record, checks it first.
pub fn is_plain_session_id(session_id: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut chars = session_id.chars();
    match chars.next() {
        Some(c) if ok(c) => chars.all(|c| ok(c) || c == '.'),
        _ => false,
    }
}

/// The reason a session has no engine while `gate.mode` is off.
pub const OFF: &str = "gate.mode off";

/// agent.yaml as the gate reads it before any session resolves (ADR-708 §2).
#[derive(Debug, Clone)]
pub enum GateReading {
    /// The gate judges: the layer, and the settings it resolves to.
    Judges { user: Box<UserLayer>, settings: Box<Settings> },
    /// `gate.mode: off`: the gate judges nothing.
    Off,
    /// No engine named and no key file: the gate is off, as before ADR-708.
    NoKey,
}

/// Reads the layer at `path` as the gate does: a finding that fails closed,
/// a file that cannot be read, or an engine that names a profile that does
/// not build is a [`CONFIG`] reason, and the gate is off until it is fixed.
/// `say` prints the layer's diagnostics on stderr, as a hook does; a screen
/// or a status command passes false.
pub fn read_gate(path: &Path, has_key: impl Fn(Provider) -> bool, say: bool) -> Result<GateReading, String> {
    let user = if say { profile::gate_layer(path) } else { profile::load_gate_layer(path).map(|(u, _)| u) };
    let user = user.map_err(|e| format!("{CONFIG}: {e:#}"))?;
    if user.mode == Some(Mode::Off) {
        return Ok(GateReading::Off);
    }
    match profile::resolve(&user, has_key) {
        Err(e) => Err(format!("{CONFIG}: {e:#}")),
        Ok(None) => Ok(GateReading::NoKey),
        Ok(Some(settings)) => Ok(GateReading::Judges { user: Box::new(user), settings: Box::new(settings) }),
    }
}

/// A session's engine under the layer `user`: [`resolve_session`] over its
/// profiles, with the provider of the engine it names.
pub fn session_engine(user: &UserLayer, host: bool, has_key: impl Fn(Provider) -> bool) -> SessionEngine {
    let profiles = profile::profiles(user);
    let resolution = resolve_session(user.engine.as_deref(), &profiles, host, has_key);
    let provider = resolution.engine.as_ref().and_then(|e| profiles.get(e)).map(|p| p.provider);
    SessionEngine { resolution, provider }
}

/// What a session resolves to under `reading`, as the gate resolves it:
/// with no reading that judges, no engine and the reason.
pub fn resolve_reading(reading: &Result<GateReading, String>, host: bool, has_key: impl Fn(Provider) -> bool) -> SessionEngine {
    let none = |reason: &str| SessionEngine { resolution: Resolution { engine: None, reason: reason.to_string() }, provider: None };
    match reading {
        Err(reason) => none(reason),
        Ok(GateReading::Off) => none(OFF),
        Ok(GateReading::NoKey) => none(NO_KEY),
        Ok(GateReading::Judges { user, .. }) => session_engine(user, host, has_key),
    }
}

/// What a session resolves to now (ADR-708 §2), from the layer at `path`,
/// the key files `has_key` reports and whether a host registered: the
/// gate's reading and resolution in one call, printing nothing.
pub fn resolve_now(path: &Path, has_key: impl Fn(Provider) -> bool, host: bool) -> Resolution {
    resolve_reading(&read_gate(path, &has_key, false), host, &has_key).resolution
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

    fn layer(tag: &str, text: Option<&str>) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-engine-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("agent.yaml");
        if let Some(t) = text {
            std::fs::write(&path, t).unwrap();
        }
        path
    }

    #[test]
    fn resolve_now_reads_agent_yaml_as_the_gate_does() {
        let key = only(Provider::Anthropic);
        // claude-code with no host and a key: the key engine stands in.
        let p = layer("cc", Some("engine: claude-code\n"));
        assert_eq!(resolve_now(&p, &key, false), Resolution { engine: Some("anthropic".into()), reason: NO_HOST.into() });
        assert_eq!(resolve_now(&p, &key, true).engine.as_deref(), Some("claude-code"));
        // No key: unavailable, with the remedy.
        assert!(resolve_now(&p, |_| false, false).is_unavailable());
        // gate.mode off: no engine, whatever is named.
        let p = layer("off", Some("engine: claude-code\nmode: off\n"));
        assert_eq!(resolve_now(&p, &key, true), Resolution { engine: None, reason: OFF.into() });
        // No file, no key: off as before ADR-708.
        let p = layer("none", None);
        assert_eq!(resolve_now(&p, |_| false, false).reason, NO_KEY);
    }

    #[test]
    fn a_layer_the_gate_cannot_use_is_a_config_reason() {
        // Does not parse: the gate fails closed and says why.
        let p = layer("broken", Some("engine: [claude-code\n"));
        let r = resolve_now(&p, |_| true, true);
        assert_eq!(r.engine, None);
        assert!(r.reason.starts_with("config: ") && r.reason.ends_with("the gate is off until it is fixed"), "{}", r.reason);
        // Cannot be read: a directory where the file should be.
        let p = layer("unreadable", None);
        std::fs::create_dir_all(&p).unwrap();
        let r = resolve_now(&p, |_| true, true);
        assert!(r.engine.is_none() && r.reason.starts_with("config: "), "{r:?}");
        // The engine is a profile of your own that does not build.
        let p = layer("half", Some("engine: half\nprofiles:\n  half:\n    provider: openrouter\n"));
        let r = resolve_now(&p, |_| true, true);
        assert!(r.reason.starts_with("config: engine 'half' names a profile that does not build"), "{}", r.reason);
    }

    #[test]
    fn plain_session_ids_name_one_directory() {
        assert!(is_plain_session_id("abc-123") && is_plain_session_id("a.b_c"));
        for bad in ["", "../x", ".hidden", "a/b", "a\\b"] {
            assert!(!is_plain_session_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_resolution_round_trips_as_json() {
        let r = Resolution { engine: Some("anthropic".into()), reason: NO_HOST.into() };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"engine":"anthropic","reason":"claude-code unavailable: no Claude Code host"}"#);
        assert_eq!(serde_json::from_str::<Resolution>(&line).unwrap(), r);
    }
}
