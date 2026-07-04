//! Declarative configuration for ways.
//!
//! Resolution order (later overrides earlier):
//!   1. Built-in defaults
//!   2. ~/.claude/ways.json (legacy JSON, pre-1.0 backward compat)
//!   3. $XDG_CONFIG_HOME/ways/config.yaml (legacy XDG path)
//!   4. $XDG_CONFIG_HOME/agent-ways/config.yaml (canonical user scope)
//!   5. $PROJECT/.claude/ways.yaml (project scope)

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
    /// Disabled domains (e.g., ["ea", "itops"]) — user scope (legacy ways.json).
    pub disabled_domains: Vec<String>,
    /// Disabled ways (e.g., ["itops/incident", "meta/introspection"]) — project scope only.
    /// Populated exclusively from `{project}/.claude/ways.yaml`. Default-enabled
    /// (absence means the way fires normally). See ADR-131.
    ///
    /// Field is `pub(crate)` (not `pub`) to keep the only legitimate writer
    /// — `apply_project_ways_overlay` — inside this module. Readers access
    /// via `disabled_ways()`. This makes the "project scope only" invariant
    /// structural rather than conventional: a future contributor who adds
    /// per-way knobs to user-scope `apply_yaml` would have to also touch
    /// this field, which sits right next to a load-bearing doc comment.
    pub(crate) disabled_ways: Vec<String>,
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
    /// Keyword gate fraction (ADR-155). A `pattern:` hit on the prompt/task
    /// surface fires only if the way's embedding score also clears
    /// `keyword_gate_fraction × effective_threshold` on at least one model
    /// lane. The gate vetoes lexical coincidence (a pasted URL containing
    /// "github", "I remember the tektronix…") using the score the batch
    /// embedding pass already computed — it adds no model work. 0.0 disables
    /// the gate (every pattern hit fires, the pre-ADR-155 behavior); ways can
    /// opt out individually with `pattern_strict: true`. Default 0.5: on the
    /// default EN threshold (0.40) the floor is 0.20, which sits above every
    /// observed false fire (0.09–0.15) and below every observed true fire
    /// (0.28+) at decision time.
    pub keyword_gate_fraction: f64,
    /// Near-miss margin (ADR-134). A way that did NOT fire is logged as a
    /// `way_nearmiss` telemetry event when at least one model's score landed
    /// within this much *below* its effective threshold (`thr - margin <=
    /// score < thr`). Purely a logging knob — it never changes firing. The
    /// tuning passes (`ways tune --cadence/--precision`) consume the stream.
    /// Default 0.05: a narrow band that captures genuine near-fires without
    /// flooding the log with deep misses.
    pub near_miss_margin: f64,
    /// Refire presets (ADR-126). Each value is a fraction of the session
    /// context window. At fire evaluation time, a way's `refire: <name>`
    /// resolves by looking up the preset here and multiplying by the
    /// operator's current context window.
    pub refire_presets: HashMap<String, f64>,
    /// Override for where `ways settings` refreshes its Claude Code settings
    /// schema from (ADR-147). `None` = the built-in default (community
    /// SchemaStore). Resolution precedence is env > this > default; the resolver
    /// lives in the `ways` binary's `cmd::settings::source`. A config surface so
    /// an org can point at an internal mirror or a pinned version instead of the
    /// public schema.
    pub settings_schema_url: Option<String>,
    /// Whether `ways reconcile` projects the framework's secret-path
    /// `permissions.deny` baseline into `settings.json` (ADR-152). Default
    /// `true` — secure by default. Set `secret_path_deny: false` to suppress the
    /// baseline entirely (the one explicit opt-out; a Claude Code deny cannot be
    /// re-opened by a user `allow`, so the escape hatch lives here, not in
    /// settings).
    pub secret_path_deny: bool,
}

impl Config {
    /// The active non-English localization language, or `None` in English mode.
    ///
    /// English mode (`en` / `auto` / unset — the default) keeps the intl pipeline
    /// dormant: no multilingual corpus, no multilingual matching, no locale tuning
    /// (ADR-139). A specific non-English code (set by the `ways-localize` skill)
    /// switches the build, matcher, and tuner into localized mode.
    pub fn localized_language(&self) -> Option<&str> {
        match self.language.as_str() {
            "en" | "auto" | "" => None,
            other => Some(other),
        }
    }
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
            disabled_ways: Vec::new(),
            parent_threshold_multiplier: 0.8,
            parent_boost_floor: 0.40,
            default_embed_threshold: 0.40,
            default_multi_embed_threshold: 0.55,
            keyword_gate_fraction: 0.5,
            near_miss_margin: 0.05,
            refire_presets,
            settings_schema_url: None,
            secret_path_deny: true,
        }
    }
}

impl Config {
    /// Public read accessor for the project-scope disable list (ADR-131).
    pub fn disabled_ways(&self) -> &[String] {
        &self.disabled_ways
    }

    /// Load config with full resolution chain.
    pub fn load(project_dir: &str) -> Self {
        let mut cfg = Config::default();

        // Layer 1: legacy ways.json
        let ways_json = home_dir().join(".claude/ways.json");
        if let Ok(content) = std::fs::read_to_string(&ways_json) {
            cfg.apply_ways_json(&content);
        }

        // Layer 2: legacy XDG user config ($XDG_CONFIG_HOME/ways/config.yaml).
        // Kept as a fallback for installs written before the app-namespaced path.
        let legacy_xdg = xdg_config_dir().join("ways/config.yaml");
        if let Ok(content) = std::fs::read_to_string(&legacy_xdg) {
            cfg.apply_yaml(&content);
        }

        // Layer 3: canonical user config ($XDG_CONFIG_HOME/agent-ways/config.yaml) —
        // the app-namespaced location, matching user_ways_root's parent. This is the
        // single source of truth for the path (paths::user_config); it overrides
        // the legacy `ways/config.yaml` above when both exist.
        let user_config = crate::paths::user_config();
        if user_config != legacy_xdg {
            if let Ok(content) = std::fs::read_to_string(&user_config) {
                cfg.apply_yaml(&content);
            }
        }

        // Layer 4: project overlay — only this layer may populate `disabled_ways`
        // (ADR-131: per-way disable is project-scope only).
        let project_config = Path::new(project_dir).join(".claude/ways.yaml");
        if let Ok(content) = std::fs::read_to_string(&project_config) {
            cfg.apply_yaml(&content);
            cfg.apply_project_ways_overlay(&content);
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
        if let Some(v) = doc.get("keyword_gate_fraction").and_then(|v| v.as_f64()) {
            self.keyword_gate_fraction = v;
        }
        if let Some(v) = doc.get("near_miss_margin").and_then(|v| v.as_f64()) {
            self.near_miss_margin = v;
        }
        if let Some(m) = doc.get("refire_presets").and_then(|v| v.as_mapping()) {
            for (k, v) in m {
                if let (Some(name), Some(fraction)) = (k.as_str(), v.as_f64()) {
                    self.refire_presets.insert(name.to_string(), fraction);
                }
            }
        }
        if let Some(v) = doc.get("settings_schema_url").and_then(|v| v.as_str()) {
            let v = v.trim();
            if !v.is_empty() {
                self.settings_schema_url = Some(v.to_string());
            }
        }
        if let Some(v) = doc.get("secret_path_deny").and_then(|v| v.as_bool()) {
            self.secret_path_deny = v;
        }
    }

    /// Parse the project-scope `ways:` mapping for per-way toggles (ADR-131).
    ///
    /// Schema accepts two equivalent forms:
    ///   ways:
    ///     itops/incident: false              # shorthand
    ///     meta/introspection:                # long-form
    ///       enabled: false
    ///
    /// Anything that evaluates to enabled=false is collected into `disabled_ways`.
    /// Unknown sub-keys on the long-form (threshold overrides, etc.) are ignored
    /// — reserved for future use per ADR-131.
    /// Public alias used by `cmd::disable` to verify writer/reader round-trips
    /// without exposing the parser as a stable API surface. Keeping the
    /// internal name pinned makes future refactors trivial.
    pub fn apply_project_ways_overlay_public(&mut self, content: &str) {
        self.apply_project_ways_overlay(content);
    }

    fn apply_project_ways_overlay(&mut self, content: &str) {
        let doc: serde_yaml::Value = match serde_yaml::from_str(content) {
            Ok(v) => v,
            Err(_) => return, // already reported by apply_yaml
        };
        let Some(ways) = doc.get("ways").and_then(|v| v.as_mapping()) else {
            return;
        };
        for (k, v) in ways {
            let Some(name) = k.as_str() else { continue };
            let disabled = match v {
                serde_yaml::Value::Bool(b) => !*b, // shorthand: `way: false` means disabled
                serde_yaml::Value::Mapping(m) => m
                    .get(serde_yaml::Value::String("enabled".to_string()))
                    .and_then(|v| v.as_bool())
                    .map(|b| !b)
                    .unwrap_or(false),
                _ => false,
            };
            if disabled && !self.disabled_ways.iter().any(|w| w == name) {
                self.disabled_ways.push(name.to_string());
            }
        }
    }

    /// Initialize user config at the canonical XDG path.
    pub fn init_user_config() -> PathBuf {
        let path = crate::paths::user_config();
        if path.exists() {
            return path;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let content = "# ways configuration
# User scope: $XDG_CONFIG_HOME/agent-ways/config.yaml
# Project scope: {project}/.claude/ways.yaml (layered on top)

# language: en          # Output language (en, ja, auto)
# default_scope: agent  # Default scope for ways without explicit scope
# disabled_domains: []  # Domains to disable everywhere (e.g., [ea, itops])
# secret_path_deny: true # Project the secret-path permissions.deny baseline
#                        # (~/.ssh, ~/.aws, .env, …) into settings.json — ADR-152.
#                        # Set false to opt out entirely (secure by default).

# Per-way enable/disable (ADR-131) is project scope only — set it in
# {project}/.claude/ways.yaml using either form:
#   ways:
#     itops/incident: false          # shorthand
#     meta/introspection:            # long-form
#       enabled: false
# Or run `ways disable <name>` / `ways enable <name>` from the project root.
";
        std::fs::write(&path, content).ok();
        path
    }

    /// Show the config file paths (canonical first, then honored legacy fallbacks).
    pub fn config_path() -> String {
        format!(
            "user:    {}\nlegacy:  {}\nlegacy:  {}\nproject: $PROJECT/.claude/ways.yaml",
            crate::paths::user_config().display(),
            xdg_config_dir().join("ways/config.yaml").display(),
            home_dir().join(".claude/ways.json").display()
        )
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
    fn settings_schema_url_defaults_none_and_parses() {
        let mut cfg = Config::default();
        assert_eq!(cfg.settings_schema_url, None);
        cfg.apply_yaml("settings_schema_url: https://mirror.example/cc.json");
        assert_eq!(
            cfg.settings_schema_url.as_deref(),
            Some("https://mirror.example/cc.json")
        );
        // Blank is ignored (stays whatever it was).
        cfg.apply_yaml("settings_schema_url: '   '");
        assert_eq!(
            cfg.settings_schema_url.as_deref(),
            Some("https://mirror.example/cc.json")
        );
    }

    #[test]
    fn secret_path_deny_defaults_true_and_opts_out() {
        let mut cfg = Config::default();
        assert!(cfg.secret_path_deny, "secure by default (ADR-152)");
        cfg.apply_yaml("secret_path_deny: false");
        assert!(!cfg.secret_path_deny, "explicit opt-out honored");
        cfg.apply_yaml("secret_path_deny: true");
        assert!(cfg.secret_path_deny);
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

    // ── ADR-131: project-scope per-way disable ─────────────────────

    #[test]
    fn project_overlay_shorthand_disable() {
        let mut cfg = Config::default();
        cfg.apply_project_ways_overlay(
            "ways:\n  \
             itops/incident: false\n  \
             meta/introspection: false\n",
        );
        assert!(cfg.disabled_ways.iter().any(|w| w == "itops/incident"));
        assert!(cfg.disabled_ways.iter().any(|w| w == "meta/introspection"));
        assert_eq!(cfg.disabled_ways.len(), 2);
    }

    #[test]
    fn project_overlay_longform_disable() {
        let mut cfg = Config::default();
        cfg.apply_project_ways_overlay(
            "ways:\n  \
             itops/incident:\n    \
             enabled: false\n",
        );
        assert_eq!(cfg.disabled_ways, vec!["itops/incident".to_string()]);
    }

    #[test]
    fn project_overlay_enabled_true_is_noop() {
        // Explicit `enabled: true` (or shorthand `true`) must NOT add to disabled_ways.
        let mut cfg = Config::default();
        cfg.apply_project_ways_overlay(
            "ways:\n  \
             itops/incident: true\n  \
             meta/introspection:\n    \
             enabled: true\n",
        );
        assert!(cfg.disabled_ways.is_empty());
    }

    #[test]
    fn project_overlay_missing_ways_key_is_noop() {
        let mut cfg = Config::default();
        cfg.apply_project_ways_overlay("language: en\nparent_boost_floor: 0.40\n");
        assert!(cfg.disabled_ways.is_empty());
    }

    #[test]
    fn project_overlay_dedupes_repeated_entries() {
        let mut cfg = Config::default();
        cfg.disabled_ways.push("itops/incident".to_string());
        cfg.apply_project_ways_overlay("ways:\n  itops/incident: false\n");
        assert_eq!(cfg.disabled_ways.len(), 1);
    }

    #[test]
    fn project_overlay_ignores_unknown_subkeys() {
        // Long-form with future-reserved keys should still parse and not panic.
        let mut cfg = Config::default();
        cfg.apply_project_ways_overlay(
            "ways:\n  \
             itops/incident:\n    \
             enabled: false\n    \
             embed_threshold: 0.50\n",
        );
        assert_eq!(cfg.disabled_ways, vec!["itops/incident".to_string()]);
    }

    #[test]
    fn apply_yaml_does_not_populate_disabled_ways() {
        // Only apply_project_ways_overlay should touch disabled_ways.
        // apply_yaml is shared between user-scope and project-scope; if it ever
        // started reading `ways:`, user-scope YAML would gain a per-way disable
        // surface — explicitly forbidden by ADR-131.
        let mut cfg = Config::default();
        cfg.apply_yaml("ways:\n  itops/incident: false\n");
        assert!(cfg.disabled_ways.is_empty());
    }

    #[test]
    fn localized_language_gates_on_mode() {
        // English mode — en / auto / empty are all dormant (ADR-139).
        let mut cfg = Config { language: "auto".to_string(), ..Default::default() };
        assert_eq!(cfg.localized_language(), None);
        cfg.language = "en".to_string();
        assert_eq!(cfg.localized_language(), None);
        cfg.language = String::new();
        assert_eq!(cfg.localized_language(), None);
        // Localized mode — a specific non-English code.
        cfg.language = "es".to_string();
        assert_eq!(cfg.localized_language(), Some("es"));
    }
}
