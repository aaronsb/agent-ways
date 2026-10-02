//! Attend's cache root and the directories under it.
//!
//! Every attend-owned path (signal trays, seen-sets, heartbeats, the
//! instance registry, keepwarm's ledger) sits under one root:
//! `$XDG_CACHE_HOME/attend`, or `~/.cache/attend` when `XDG_CACHE_HOME`
//! is unset, empty or relative, as the XDG base-directory spec requires.
//! Attend owns this tree; the CLI is the contract for reading it.

use std::path::PathBuf;

/// The attend cache root.
pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| claude_sessions::home_dir().join(".cache"));
    base.join("attend")
}

/// The signals base: project trays, `_broadcast`, `@group` rooms and
/// `_groups.yaml`.
pub fn signals_dir() -> PathBuf {
    dir().join("signals")
}

/// Per-session sensor state: seen-sets, drain counters, last-inbound ids.
pub fn state_dir() -> PathBuf {
    dir().join("state")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn with_env(home: &str, xdg: Option<&str>, f: impl FnOnce()) {
        let _g = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (prev_home, prev_xdg) = (std::env::var_os("HOME"), std::env::var_os("XDG_CACHE_HOME"));
        std::env::set_var("HOME", home);
        match xdg {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        f();
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
    }

    #[test]
    fn xdg_cache_home_is_honoured() {
        with_env("/home/u", Some("/var/cache/u"), || {
            assert_eq!(dir(), PathBuf::from("/var/cache/u/attend"));
            assert_eq!(signals_dir(), PathBuf::from("/var/cache/u/attend/signals"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn unset_empty_or_relative_falls_back_to_home() {
        for xdg in [None, Some(""), Some("relative/cache")] {
            with_env("/home/u", xdg, || {
                assert_eq!(dir(), PathBuf::from("/home/u/.cache/attend"), "XDG_CACHE_HOME={xdg:?}");
            });
        }
    }
}
