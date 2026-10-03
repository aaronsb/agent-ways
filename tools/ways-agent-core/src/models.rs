//! The provider model lists `ways agent models` fetches, kept on disk so the
//! settings can offer them without network I/O (#795).
//!
//! One file per provider under the cache root, `agent/models-<provider>.json`:
//! derived from the provider and safe to delete, so it sits with the other
//! regenerable state and not under the state root. It holds the provider's
//! whole answer, prices included, and the time it was fetched. A reader
//! filters; the file does not. A stale list is still read: the provider
//! adding a model is no reason to refuse one it listed.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::profile::Provider;

/// A model a provider offers, with prices per million tokens when known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
}

/// What the cache file holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub provider: String,
    /// Seconds since the Unix epoch when the list was fetched.
    pub fetched_at: u64,
    pub models: Vec<ModelInfo>,
}

pub fn cache_dir() -> PathBuf {
    ways_core::paths::cache_root().join("agent")
}

pub fn cache_path(dir: &Path, provider: Provider) -> PathBuf {
    dir.join(format!("models-{}.json", provider.as_str()))
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Write a provider's list, atomically, with the time it was fetched.
pub fn write_in(dir: &Path, provider: Provider, models: &[ModelInfo], fetched_at: u64) -> Result<PathBuf> {
    let path = cache_path(dir, provider);
    let body = serde_json::to_string_pretty(&Cached { provider: provider.as_str().into(), fetched_at, models: models.to_vec() })?;
    agent_settings::writer::write_atomic(&path, body + "\n").with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

pub fn write(provider: Provider, models: &[ModelInfo]) -> Result<PathBuf> {
    write_in(&cache_dir(), provider, models, now())
}

/// The cached list, or `None` when absent or unreadable (a damaged file is
/// the same as none: `ways agent models` rewrites it).
pub fn read_in(dir: &Path, provider: Provider) -> Option<Cached> {
    let text = std::fs::read_to_string(cache_path(dir, provider)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn read(provider: Provider) -> Option<Cached> {
    read_in(&cache_dir(), provider)
}

/// The ids the settings offer: the one the gate is tuned for, then the
/// provider's Anthropic models, then the rest, each group by id.
pub fn offered(provider: Provider, models: &[ModelInfo]) -> Vec<String> {
    let mut ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    ids.sort_by_key(|id| (!provider.is_recommended(id), !(id.starts_with("claude") || id.starts_with("anthropic/")), id.clone()));
    ids.dedup();
    ids
}

/// The error a settings load reports for a provider whose list is not cached.
pub fn missing(provider: Provider) -> String {
    format!("model list not fetched; run `ways agent models --provider {provider}`")
}

/// The list for `provider` in `dir`, as the model key offers it.
pub fn options_in(dir: &Path, provider: Provider) -> Result<Vec<String>, String> {
    match read_in(dir, provider) {
        Some(c) if !c.models.is_empty() => Ok(offered(provider, &c.models)),
        _ => Err(missing(provider)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str) -> ModelInfo {
        ModelInfo { id: id.into(), name: id.into(), input_per_mtok: None, output_per_mtok: None }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_written_list_reads_back_with_its_time() {
        let dir = scratch("rt");
        let list = vec![m("claude-haiku-4-5"), m("claude-opus-4-5")];
        let path = write_in(&dir, Provider::Anthropic, &list, 1_700_000_000).unwrap();
        assert_eq!(path, dir.join("models-anthropic.json"));
        let c = read_in(&dir, Provider::Anthropic).unwrap();
        assert_eq!((c.fetched_at, c.models), (1_700_000_000, list));
        assert!(read_in(&dir, Provider::Openrouter).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_absent_or_damaged_cache_says_how_to_fetch_it() {
        let dir = scratch("absent");
        let want = "model list not fetched; run `ways agent models --provider anthropic`";
        assert_eq!(options_in(&dir, Provider::Anthropic).unwrap_err(), want);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(cache_path(&dir, Provider::Anthropic), "{ not json").unwrap();
        assert_eq!(options_in(&dir, Provider::Anthropic).unwrap_err(), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_list_is_still_offered_with_the_tuned_model_first() {
        let dir = scratch("stale");
        write_in(&dir, Provider::Openrouter, &[m("openai/gpt-5"), m("anthropic/claude-opus-4.5"), m("anthropic/claude-haiku-4.5")], 1).unwrap();
        assert_eq!(
            options_in(&dir, Provider::Openrouter).unwrap(),
            ["anthropic/claude-haiku-4.5", "anthropic/claude-opus-4.5", "openai/gpt-5"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
