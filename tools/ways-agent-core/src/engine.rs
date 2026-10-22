//! The session's judge engine (ADR-708 §2).
//!
//! Each session resolves its engine once, at its first judge request, and
//! holds it for every lane the gate covers. The resolution is a pure function
//! of `gate.engine`, the profiles, whether a Claude Code judge host has
//! registered, and which providers have a key; the caller stores it. No job
//! falls back from one engine to another, so no candidate is judged twice.

use std::collections::BTreeMap;

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

/// What an unavailable engine's reason tells the operator to do.
const REMEDY: &str = "configure an Anthropic or OpenRouter key (`ways agent key add --provider anthropic` or `--provider openrouter`)";

/// A session's engine and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    /// The profile the session judges with; `None` when it has none.
    pub engine: Option<String>,
    /// [`EXPLICIT`], [`KEY_FOUND`] or [`NO_HOST`] with an engine; with none,
    /// [`NO_KEY`] or a reason that starts with [`ENGINE_UNAVAILABLE`] and
    /// names the remedy.
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
        return Resolution::unavailable(format!("engine '{name}' names no profile; name a profile or {REMEDY}"));
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
        assert!(resolve_session(Some("nosuch"), &all, true, |_| true).is_unavailable());
    }

    #[test]
    fn a_resolution_round_trips_as_json() {
        let r = Resolution { engine: Some("anthropic".into()), reason: NO_HOST.into() };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"engine":"anthropic","reason":"claude-code unavailable: no Claude Code host"}"#);
        assert_eq!(serde_json::from_str::<Resolution>(&line).unwrap(), r);
    }
}
