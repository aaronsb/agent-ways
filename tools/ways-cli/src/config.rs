//! Declarative configuration for ways.
//!
//! Resolution order (later overrides earlier):
//!   1. Built-in defaults
//!   2. ~/.claude/ways.json (legacy, backward compat)
//!   3. $XDG_CONFIG_HOME/ways/config.yaml (user scope)
//!   4. $PROJECT/.claude/ways.yaml (project scope)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Global config, loaded once on first access.
/// Access via `config::global()` — grep-friendly for future context refactor.
static GLOBAL: LazyLock<Config> = LazyLock::new(|| {
    let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
    Config::load(&project_dir)
});

/// Access the process-wide config. Every call site is a future `ctx.config` migration point.
pub fn global() -> &'static Config {
    &GLOBAL
}

/// Ways configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default scope for ways without explicit scope
    pub default_scope: String,
    /// Output language (e.g., "en", "ja", "auto")
    pub language: String,
    /// Disabled domains (e.g., ["ea", "itops"])
    pub disabled_domains: Vec<String>,
    /// Parent-boost multiplier: a child way's effective embed_threshold is
    /// multiplied by this value when any ancestor way has fired in the
    /// session. Values <1.0 make children fire more easily once their parent
    /// domain is active (progressive disclosure). 1.0 disables the boost.
    pub parent_threshold_multiplier: f64,
    /// Minimum effective threshold after parent-boost. Without a floor,
    /// cascading boosts can push children into the noise band where any
    /// generic-word collision fires. Default 0.40 — just below the per-way
    /// default but well above the multilingual-corpus noise floor (~0.30).
    pub parent_boost_floor: f64,
    /// Default cosine threshold for the English model/corpus path.
    /// Used when a way's frontmatter doesn't specify embed_threshold.
    /// The EN model (384-dim, English-only) has sharper discrimination
    /// than the multilingual model, so this can sit lower.
    pub default_embed_threshold: f64,
    /// Default cosine threshold for the multilingual model/corpus path.
    /// The multilingual model (768-dim, 52 languages) has coarser
    /// discrimination and produces a wider noise band, so multi-corpus
    /// matches need a stricter bar. Kept separate from EN — the two
    /// models produce scores in different distributions, so comparing
    /// them directly (max, average) is apples-to-oranges.
    pub default_multi_embed_threshold: f64,
    /// Refire presets (ADR-126). Each value is a fraction of the session
    /// context window. At fire evaluation time, a way's `refire: <name>`
    /// resolves by looking up the preset here and multiplying by the
    /// operator's current context window.
    pub refire_presets: HashMap<String, f64>,
}

impl Default for Config {
    fn default() -> Self {
        let mut refire_presets = HashMap::new();
        refire_presets.insert("once".to_string(), 1.0);
        refire_presets.insert("rare".to_string(), 0.4);
        refire_presets.insert("normal".to_string(), 0.15);
        refire_presets.insert("frequent".to_string(), 0.05);

        Self {
            default_scope: "agent".to_string(),
            language: "auto".to_string(),
            disabled_domains: Vec::new(),
            parent_threshold_multiplier: 0.8,
            parent_boost_floor: 0.40,
            default_embed_threshold: 0.40,
            default_multi_embed_threshold: 0.55,
            refire_presets,
        }
    }
}

impl Config {
    /// Load config with full resolution chain.
    pub fn load(project_dir: &str) -> Self {
        let mut cfg = Config::default();

        // Layer 1: legacy ways.json
        let ways_json = home_dir().join(".claude/ways.json");
        if let Ok(content) = std::fs::read_to_string(&ways_json) {
            cfg.apply_ways_json(&content);
        }

        // Layer 2: XDG user config
        let xdg_config = xdg_config_dir().join("ways/config.yaml");
        if let Ok(content) = std::fs::read_to_string(&xdg_config) {
            cfg.apply_yaml(&content);
        }

        // Layer 3: project overlay
        let project_config = Path::new(project_dir).join(".claude/ways.yaml");
        if let Ok(content) = std::fs::read_to_string(&project_config) {
            cfg.apply_yaml(&content);
        }

        cfg
    }

    /// Apply values from legacy ways.json.
    fn apply_ways_json(&mut self, content: &str) {
        let v: serde_json::Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(lang) = v.get("output_language").and_then(|v| v.as_str()) {
            self.language = lang.to_string();
        }

        if let Some(disabled) = v.get("disabled").and_then(|v| v.as_array()) {
            self.disabled_domains = disabled
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    /// Apply values from a YAML config file.
    fn apply_yaml(&mut self, content: &str) {
        let doc: serde_yaml::Value = match serde_yaml::from_str(content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[ways] config: parse error: {}", e);
                return;
            }
        };

        if let Some(v) = doc.get("language").and_then(|v| v.as_str()) {
            self.language = v.to_string();
        }
        if let Some(v) = doc.get("default_scope").and_then(|v| v.as_str()) {
            self.default_scope = v.to_string();
        }
        if let Some(disabled) = doc.get("disabled_domains").and_then(|v| v.as_sequence()) {
            self.disabled_domains = disabled
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(v) = doc.get("parent_threshold_multiplier").and_then(|v| v.as_f64()) {
            self.parent_threshold_multiplier = v;
        }
        if let Some(v) = doc.get("parent_boost_floor").and_then(|v| v.as_f64()) {
            self.parent_boost_floor = v;
        }
        if let Some(v) = doc.get("default_embed_threshold").and_then(|v| v.as_f64()) {
            self.default_embed_threshold = v;
        }
        if let Some(v) = doc.get("default_multi_embed_threshold").and_then(|v| v.as_f64()) {
            self.default_multi_embed_threshold = v;
        }
        if let Some(m) = doc.get("refire_presets").and_then(|v| v.as_mapping()) {
            for (k, v) in m {
                if let (Some(name), Some(fraction)) = (k.as_str(), v.as_f64()) {
                    self.refire_presets.insert(name.to_string(), fraction);
                }
            }
        }
    }

    /// Initialize user config at XDG path.
    pub fn init_user_config() -> PathBuf {
        let path = xdg_config_dir().join("ways/config.yaml");
        if path.exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let content = "# ways configuration
# User scope: $XDG_CONFIG_HOME/ways/config.yaml
# Project scope: {project}/.claude/ways.yaml (layered on top)

# language: en          # Output language (en, ja, auto)
# default_scope: agent  # Default scope for ways without explicit scope
# disabled_domains: []  # Domains to disable (e.g., [ea, itops])
";
        std::fs::write(&path, content).ok();
        path
    }

    /// Show the config file path.
    pub fn config_path() -> String {
        let xdg = xdg_config_dir().join("ways/config.yaml");
        format!("user:    {}\nlegacy:  {}\nproject: $PROJECT/.claude/ways.yaml",
            xdg.display(),
            home_dir().join(".claude/ways.json").display())
    }
}

fn home_dir() -> PathBuf {
    crate::util::home_dir()
}

fn xdg_config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let cfg = Config::default();
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.default_scope, "agent");
        assert_eq!(cfg.parent_threshold_multiplier, 0.8);
        assert_eq!(cfg.parent_boost_floor, 0.40);
        assert_eq!(cfg.default_embed_threshold, 0.40);
        assert_eq!(cfg.default_multi_embed_threshold, 0.55);
        assert_eq!(cfg.refire_presets.get("once").copied(), Some(1.0));
        assert_eq!(cfg.refire_presets.get("rare").copied(), Some(0.4));
        assert_eq!(cfg.refire_presets.get("normal").copied(), Some(0.15));
        assert_eq!(cfg.refire_presets.get("frequent").copied(), Some(0.05));
    }

    #[test]
    fn apply_yaml_refire_presets_override() {
        let mut cfg = Config::default();
        cfg.apply_yaml(
            "refire_presets:\n  \
             normal: 0.20\n  \
             perpetual: 0.01\n",
        );
        // Override existing
        assert_eq!(cfg.refire_presets.get("normal").copied(), Some(0.20));
        // Add new custom preset
        assert_eq!(cfg.refire_presets.get("perpetual").copied(), Some(0.01));
        // Untouched preset still present
        assert_eq!(cfg.refire_presets.get("rare").copied(), Some(0.4));
    }

    #[test]
    fn apply_yaml_overrides() {
        let mut cfg = Config::default();
        cfg.apply_yaml("language: ja\nparent_threshold_multiplier: 0.7");
        assert_eq!(cfg.language, "ja");
        assert_eq!(cfg.parent_threshold_multiplier, 0.7);
    }

    #[test]
    fn apply_yaml_threshold_fields() {
        let mut cfg = Config::default();
        cfg.apply_yaml(
            "default_embed_threshold: 0.35\n\
             default_multi_embed_threshold: 0.60\n\
             parent_boost_floor: 0.30",
        );
        assert_eq!(cfg.default_embed_threshold, 0.35);
        assert_eq!(cfg.default_multi_embed_threshold, 0.60);
        assert_eq!(cfg.parent_boost_floor, 0.30);
    }

    #[test]
    fn apply_ways_json() {
        let mut cfg = Config::default();
        cfg.apply_ways_json(r#"{"output_language":"de","disabled":["ea"]}"#);
        assert_eq!(cfg.language, "de");
        assert_eq!(cfg.disabled_domains, vec!["ea"]);
    }

    #[test]
    fn yaml_overrides_json() {
        let mut cfg = Config::default();
        cfg.apply_ways_json(r#"{"output_language":"de"}"#);
        cfg.apply_yaml("language: ja");
        assert_eq!(cfg.language, "ja");
    }
}
