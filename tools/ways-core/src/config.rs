//! Declarative configuration for ways.
//!
//! Resolution order (later overrides earlier):
//!   1. Built-in defaults
//!   2. $XDG_CONFIG_HOME/agent-ways/config.yaml (user scope)
//!   3. $PROJECT/.claude/ways.yaml (project scope)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use agent_settings::LayerScope;

/// Global config, loaded once on first access.
/// Access via `config::global()` — grep-friendly for future context refactor.
static GLOBAL: LazyLock<Config> = LazyLock::new(|| {
    let project_dir = crate::util::project_dir();
    Config::load(&project_dir)
});

/// Access the process-wide config. Every call site is a future `ctx.config` migration point.
pub fn global() -> &'static Config {
    &GLOBAL
}

/// One Claude Code config directory agent-ways projects into (ADR-184).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Target {
    /// The config directory, as written. `~` is expanded on read.
    pub path: String,
    /// Projected and merged when true; withdrawn when false.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Included in transcript-derived readers. Defaults to `enabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe: Option<bool>,
    /// This target's own configuration file, layered over the user config for
    /// sessions under this target. Defaults to
    /// `$XDG_CONFIG/agent-ways/targets/<key>/config.yaml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Target {
    pub fn new(path: impl Into<String>) -> Self {
        Target { path: path.into(), enabled: true, observe: None, config: None }
    }

    /// Where this target's own config.yaml lives, explicit or default.
    pub fn config_path(&self) -> PathBuf {
        match &self.config {
            Some(p) => expand_tilde(p),
            None => crate::paths::target_config_root(&self.dir()).join("config.yaml"),
        }
    }

    /// True when `dir` is this target's directory.
    pub fn matches_dir(&self, dir: &Path) -> bool {
        let mine = self.dir();
        let mine = std::fs::canonicalize(&mine).unwrap_or(mine);
        let theirs = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        mine == theirs
    }

    /// The directory as a path, with a leading `~` expanded.
    pub fn dir(&self) -> PathBuf {
        expand_tilde(&self.path)
    }

    /// Effective observe flag: explicit value, else `enabled`.
    pub fn observes(&self) -> bool {
        self.observe.unwrap_or(self.enabled)
    }
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home_dir().join(rest)
    } else if p == "~" {
        home_dir()
    } else {
        PathBuf::from(p)
    }
}

/// Ways configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Projection targets (ADR-184). `None` means the key is absent and the
    /// implicit default applies; see [`Config::targets`].
    pub targets: Option<Vec<Target>>,
    /// The target whose own config layer applied to this load, if any.
    pub target_config: Option<PathBuf>,
    /// Project-scope master switch (ADR-184 item 6). `false` in a project's
    /// `ways.yaml` makes the scan inject nothing there.
    pub enabled: bool,
    /// Default scope for ways without explicit scope
    pub default_scope: String,
    /// Output language (e.g., "en", "ja", "auto")
    pub language: String,
    /// Disabled domains (e.g., ["ea", "itops"]) — user scope.
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
    /// Parent-boost multiplier: a child way's effective semantic fire
    /// probability is multiplied by this value when any ancestor way has fired
    /// in the session. Values <1.0 make children fire more easily once their
    /// parent domain is active (progressive disclosure). 1.0 disables the boost.
    /// (ADR-156: applies in calibrated probability space, not raw cosine.)
    pub parent_threshold_multiplier: f64,
    /// Minimum effective fire probability after parent-boost. Without a floor,
    /// cascading boosts can push children into the noise band where any
    /// generic-word collision fires. A calibrated probability (ADR-156).
    pub parent_boost_floor: f64,
    /// Semantic fire probability τ_s (ADR-156). A way fires on its own
    /// relatedness when the calibrated probability `g_m(cos) ≥ τ_s` on at least
    /// one model lane. Replaces the raw-cosine `default_embed_threshold`:
    /// calibration makes the boundary comparable across ways, so one global
    /// probability suffices where per-way cosine thresholds were needed before.
    /// Default 0.5 — the calibrated intent/noise decision boundary.
    pub semantic_fire_probability: f64,
    /// Keyword floor probability τ_k (ADR-156). A `pattern:` hit on the
    /// prompt/task surface fires only if the calibrated probability
    /// `g_m(cos) ≥ τ_k` on at least one model lane. Independent of `τ_s`
    /// (retires `keyword_gate_fraction` and its coupling to the semantic
    /// threshold): a leaky keyword is tightened by raising τ_k without moving
    /// the semantic bar. Ways opt out with `pattern_strict: true`. Default 0.15.
    pub keyword_floor_probability: f64,
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
            targets: None,
            target_config: None,
            enabled: true,
            default_scope: "agent".to_string(),
            language: "auto".to_string(),
            disabled_domains: Vec::new(),
            disabled_ways: Vec::new(),
            parent_threshold_multiplier: 0.8,
            parent_boost_floor: 0.30,
            semantic_fire_probability: 0.5,
            keyword_floor_probability: 0.15,
            near_miss_margin: 0.05,
            refire_presets,
            secret_path_deny: true,
        }
    }
}

impl Config {
    /// Public read accessor for the project-scope disable list (ADR-131).
    pub fn disabled_ways(&self) -> &[String] {
        &self.disabled_ways
    }

    /// The effective projection targets (ADR-184). With no `targets` key the
    /// list is one implicit entry, the default projection root, enabled, so an
    /// install that predates the key behaves as before. Once the key is written
    /// the list is exactly what it says, including empty.
    pub fn targets(&self) -> Vec<Target> {
        match &self.targets {
            Some(list) => list.clone(),
            None => vec![Target::new(crate::paths::projection_root().to_string_lossy().to_string())],
        }
    }

    /// Whether the `targets` key is written, i.e. activation is explicit.
    pub fn targets_explicit(&self) -> bool {
        self.targets.is_some()
    }

    /// Read the targets as the user file has them right now, apply `edit`, and
    /// write the result, all under the settings writer's lock (ADR-503 §6).
    /// Every writer of the key goes through here, so a hook's migration write
    /// and an operator's `target add` cannot lose each other's change. `edit`
    /// returns `None` to leave the file alone. Returns the list written, or
    /// the list found when nothing was written.
    pub fn edit_user_targets<F>(edit: F) -> std::io::Result<(PathBuf, Vec<Target>)>
    where
        F: FnOnce(Option<Vec<Target>>) -> Option<Vec<Target>>,
    {
        let path = crate::paths::user_config();
        let (list, _) = agent_settings::writer::edit_file(&path, None, |doc| {
            let current = doc.get(&["targets".to_string()]).and_then(Self::read_targets_value);
            let found = current.clone().unwrap_or_default();
            match edit(current) {
                Some(list) => {
                    doc.set(&["targets".to_string()], &targets_value(&list))?;
                    Ok(list)
                }
                None => Ok(found),
            }
        })?;
        Ok((path, list))
    }

    /// The writer behind [`Config::edit_user_targets`]: rewrites only the
    /// `targets` key of the file at `path` through the settings writer,
    /// keeping every other key and comment as it was, and creates the file
    /// when absent. On an explicit path so tests never touch the real config.
    pub fn write_targets_to(path: &Path, list: &[Target]) -> std::io::Result<()> {
        agent_settings::writer::edit_file(path, None, |doc| doc.set(&["targets".to_string()], &targets_value(list)))?;
        Ok(())
    }

    /// Parse a `targets:` sequence from a YAML document, used by the user layer.
    #[cfg(test)]
    fn read_targets(doc: &serde_yaml::Value) -> Option<Vec<Target>> {
        doc.get("targets").and_then(Self::read_targets_value)
    }

    fn read_targets_value(seq: &serde_yaml::Value) -> Option<Vec<Target>> {
        match serde_yaml::from_value::<Vec<Target>>(seq.clone()) {
            Ok(list) => Some(list),
            Err(e) => {
                eprintln!("[ways] config: targets: {e}; ignoring the key");
                None
            }
        }
    }

    /// Load config with full resolution chain. Each file is parsed once and
    /// checked section by section against the schema (ADR-503 §4-5): a
    /// section that fails drops out of that file's layer, with a diagnostic
    /// on stderr, and its keys resolve from the layers beneath.
    pub fn load(project_dir: &str) -> Self {
        Self::load_sections(project_dir, crate::settings::HOOK_SECTIONS)
    }

    /// [`Config::load`], reading only the named schema sections.
    pub fn load_sections(project_dir: &str, sections: &[&str]) -> Self {
        let mut cfg = Config::default();

        // User config ($XDG_CONFIG_HOME/agent-ways/config.yaml) — the
        // app-namespaced location, matching user_ways_root's parent. This is the
        // single source of truth for the path (paths::user_config).
        let user_config = crate::paths::user_config();
        if let Some(doc) = checked_file(&user_config, LayerScope::User, sections) {
            cfg.apply_values(&doc);
            // Targets are user scope only (ADR-184): a project cannot
            // redirect where the install lands.
            cfg.targets = doc.get("targets").and_then(Self::read_targets_value);
        }

        // Layer 3.5: the current target's own config (ADR-184). The session's
        // config directory names the target; its config.yaml, when present,
        // overrides the user layer for every key except `targets` itself.
        let current = crate::paths::current_config_dir();
        if let Some(t) = cfg.targets().iter().find(|t| t.matches_dir(&current)) {
            let path = t.config_path();
            if let Some(doc) = checked_file(&path, LayerScope::Target, sections) {
                cfg.apply_values(&doc);
                cfg.target_config = Some(path);
            }
        }

        // Layer 4: project overlay — only this layer may populate `disabled_ways`
        // (ADR-131: per-way disable is project-scope only).
        let project_config = crate::settings::project_file(Path::new(project_dir));
        if let Some(doc) = checked_file(&project_config, LayerScope::Project, sections) {
            cfg.apply_values(&doc);
            cfg.apply_project_ways_overlay_value(&doc);
        }

        cfg
    }

    /// Apply a YAML config file's text, checked as a user-scope layer.
    #[cfg(test)]
    fn apply_yaml(&mut self, content: &str) {
        if let Some(doc) = checked_text(content, None, LayerScope::User, crate::settings::HOOK_SECTIONS) {
            self.apply_values(&doc);
        }
    }

    /// Apply the values of a checked layer. Every value here has passed the
    /// schema, so each key is read at its type.
    fn apply_values(&mut self, doc: &serde_yaml::Value) {
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
        if let Some(v) = doc.get("semantic_fire_probability").and_then(|v| v.as_f64()) {
            self.semantic_fire_probability = v;
        }
        if let Some(v) = doc.get("keyword_floor_probability").and_then(|v| v.as_f64()) {
            self.keyword_floor_probability = v;
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
        if let Some(v) = doc.get("secret_path_deny").and_then(|v| v.as_bool()) {
            self.secret_path_deny = v;
        }
        if let Some(v) = doc.get("enabled").and_then(|v| v.as_bool()) {
            self.enabled = v;
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
        if let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(content) {
            self.apply_project_ways_overlay_value(&doc);
        }
    }

    fn apply_project_ways_overlay_value(&mut self, doc: &serde_yaml::Value) {
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
        let content = "# ways configuration
# User scope: $XDG_CONFIG_HOME/agent-ways/config.yaml
# Project scope: {project}/.claude/ways.yaml (layered on top)

# language: en          # Output language (en, ja, auto)
# default_scope: agent  # Default scope for ways without explicit scope
# disabled_domains: []  # Domains to disable everywhere (e.g., [ea, itops])
# secret_path_deny: true # Project the secret-path permissions.deny baseline
#                        # (~/.ssh, ~/.aws, .env, …) into settings.json — ADR-152.
#                        # Set false to opt out entirely (secure by default).

# Projection targets (ADR-184): the Claude Code config directories agent-ways
# is active in. Absent: the default ~/.claude, enabled. Edit with
# `ways config target add|enable|disable|remove <dir>`.
# targets:
#   - path: ~/.claude
#     enabled: true
#     observe: true        # include in transcript-derived reports (default: enabled)
#     config: ~/.config/agent-ways/targets/<key>/config.yaml   # this target's own
#                          # settings, same keys as this file, layered over it for
#                          # sessions under that config directory (default location)

# enabled: false          # In {project}/.claude/ways.yaml: switch ways off for that
#                         # project; hooks inject nothing there (ADR-184).

# Per-way enable/disable (ADR-131) is project scope only — set it in
# {project}/.claude/ways.yaml using either form:
#   ways:
#     itops/incident: false          # shorthand
#     meta/introspection:            # long-form
#       enabled: false
# Or run `ways disable <name>` / `ways enable <name>` from the project root.
";
        // Creates the file only when none exists (ADR-503 §6).
        let _ = agent_settings::writer::create_new(&path, content);
        path
    }

    /// Show the config file paths.
    pub fn config_path() -> String {
        format!(
            "user:    {}\nproject: $PROJECT/.claude/ways.yaml",
            crate::paths::user_config().display(),
        )
    }
}

fn home_dir() -> PathBuf {
    crate::util::home_dir()
}

/// Read and check one settings file. `None` when it is absent or does not
/// parse; a section that fails is left out, with a diagnostic on stderr.
fn checked_file(path: &Path, scope: LayerScope, sections: &[&str]) -> Option<serde_yaml::Value> {
    // Read lossily: a stray byte must not hide the switches in the file.
    let text = String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned();
    checked_text(&text, Some(path), scope, sections)
}

fn checked_text(text: &str, path: Option<&Path>, scope: LayerScope, sections: &[&str]) -> Option<serde_yaml::Value> {
    use agent_settings::load;
    let doc = match load::parse_text(text, path) {
        Ok(d) => d,
        Err(f) => {
            report(&f.diagnostic("ways"));
            // Whole file fails closed (ADR-503 addendum): it sets nothing,
            // and every switch in its scope is off.
            let closed = load::closed_file(&crate::settings::SCHEMA, crate::settings::FILE, scope, Some(sections));
            return Some(serde_yaml::Value::Mapping(closed));
        }
    };
    // Every finding goes to stderr, one line each: a unit that fell back,
    // and a top-level key no section owns, such as a typo or a key ADR-156
    // retired, which the check reports by name with its replacement.
    let checked = load::check(&crate::settings::SCHEMA, crate::settings::FILE, scope, &doc, Some(sections));
    if !checked.is_clean() {
        for f in checked.findings(None, path, text) {
            report(&f.diagnostic("ways"));
        }
    }
    Some(serde_yaml::Value::Mapping(checked.accepted))
}

/// Print a diagnostic once per process: a hook that loads the config more
/// than once reports each finding one time.
fn report(line: &str) {
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !seen.iter().any(|l| l == line) {
        seen.push(line.to_string());
        eprintln!("{line}");
    }
}

fn targets_value(list: &[Target]) -> serde_yaml::Value {
    serde_yaml::to_value(list).unwrap_or(serde_yaml::Value::Sequence(Vec::new()))
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
        assert_eq!(cfg.parent_boost_floor, 0.30);
        assert_eq!(cfg.semantic_fire_probability, 0.5);
        assert_eq!(cfg.keyword_floor_probability, 0.15);
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
            "semantic_fire_probability: 0.6\n\
             keyword_floor_probability: 0.2\n\
             parent_boost_floor: 0.25",
        );
        assert_eq!(cfg.semantic_fire_probability, 0.6);
        assert_eq!(cfg.keyword_floor_probability, 0.2);
        assert_eq!(cfg.parent_boost_floor, 0.25);
    }

    #[test]
    fn an_out_of_range_value_falls_its_section_back_and_the_rest_load() {
        // ADR-503 §4: the matching section fails the schema, so all of it
        // loads as canonical; the ways section of the same file loads as
        // written. Before, each key was clamped or skipped on its own.
        let mut cfg = Config::default();
        cfg.apply_yaml("language: es\nsemantic_fire_probability: 1.5\nkeyword_floor_probability: 0.2\n");
        assert_eq!(cfg.semantic_fire_probability, 0.5);
        assert_eq!(cfg.keyword_floor_probability, 0.15, "the whole section falls back");
        assert_eq!(cfg.language, "es");
    }

    #[test]
    fn a_wrong_type_or_unknown_key_falls_back_only_its_section() {
        let mut cfg = Config::default();
        cfg.apply_yaml("disabled_domains: ea\nparent_boost_floor: 0.25\n");
        assert_eq!(cfg.disabled_domains, vec!["ea".to_string()], "a bad domain list keeps the names it can read disabled");
        assert_eq!(cfg.parent_boost_floor, 0.25);
        let mut cfg = Config::default();
        cfg.apply_yaml("refire_presets:\n  normal: 0.2\n  bad: lots\nlanguage: ja\n");
        assert_eq!(cfg.refire_presets.get("normal").copied(), Some(0.15));
        assert_eq!(cfg.language, "ja");
        // A key no section owns is reported and ignored; nothing falls back.
        let mut cfg = Config::default();
        cfg.apply_yaml("mystery: 1\nsemantic_fire_probability: 0.6\n");
        assert_eq!(cfg.semantic_fire_probability, 0.6);
    }

    #[test]
    fn a_file_that_does_not_parse_sets_nothing_and_switches_its_scope_off() {
        let mut cfg = Config::default();
        cfg.apply_yaml("language: es\nsemantic_fire_probability: [\n");
        assert_eq!(cfg.language, "auto");
        assert_eq!(cfg.semantic_fire_probability, 0.5);
        assert!(!cfg.enabled, "whole file fails closed");
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
             future_reserved_key: 0.50\n",
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

    #[test]
    fn targets_absent_means_the_implicit_default_root() {
        let cfg = Config::default();
        assert!(!cfg.targets_explicit());
        let t = cfg.targets();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].dir(), crate::paths::projection_root());
        assert!(t[0].enabled);
        assert!(t[0].observes());
    }

    #[test]
    fn targets_key_parses_and_an_empty_list_is_empty() {
        let doc: serde_yaml::Value = serde_yaml::from_str(
            "targets:\n  - path: ~/.claude\n  - path: /srv/work/.claude-work\n    enabled: false\n    observe: true\n",
        )
        .unwrap();
        let list = Config::read_targets(&doc).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].dir(), crate::util::home_dir().join(".claude"));
        assert!(list[0].enabled && list[0].observes());
        assert!(!list[1].enabled);
        assert!(list[1].observes());
        let empty: serde_yaml::Value = serde_yaml::from_str("targets: []\n").unwrap();
        assert_eq!(Config::read_targets(&empty), Some(Vec::new()));
        let cfg = Config { targets: Some(Vec::new()), ..Default::default() };
        assert!(cfg.targets().is_empty());
    }

    #[test]
    fn write_targets_keeps_other_keys_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("ways-targets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, "language: es\ndisabled_domains: [ea]\n").unwrap();
        let list = vec![Target::new("~/.claude"), Target { path: "/x/.claude".into(), enabled: false, observe: Some(true), config: None }];
        Config::write_targets_to(&path, &list).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&body).unwrap();
        assert_eq!(doc.get("language").and_then(|v| v.as_str()), Some("es"));
        assert_eq!(Config::read_targets(&doc), Some(list.clone()));
        // Rewriting replaces the key rather than appending a second one.
        Config::write_targets_to(&path, &list[..1]).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(Config::read_targets(&doc).unwrap().len(), 1);
        assert_eq!(doc.get("disabled_domains").and_then(|v| v.as_sequence()).map(|s| s.len()), Some(1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_targets_keeps_comments_and_accepts_the_init_template() {
        let dir = std::env::temp_dir().join(format!("ways-targets-comments-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, "# ways configuration\n# language: en\n\nlanguage: es\n# tail comment\n").unwrap();
        Config::write_targets_to(&path, &[Target::new("~/.claude")]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.starts_with("# ways configuration\n# language: en\n"), "{body}");
        assert!(body.contains("# tail comment\n"), "{body}");
        assert!(body.contains("targets:\n  - path: \"~/.claude\"\n    enabled: true\n"), "{body}");
        let doc: serde_yaml::Value = serde_yaml::from_str(&body).unwrap();
        assert_eq!(doc.get("language").and_then(|v| v.as_str()), Some("es"));
        assert_eq!(Config::read_targets(&doc).unwrap()[0].path, "~/.claude");
        // Second write replaces the block in place, comments still intact.
        Config::write_targets_to(&path, &[Target { path: "/x".into(), enabled: false, observe: Some(true), config: None }]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.matches("targets:").count(), 1, "{body}");
        assert!(body.contains("# tail comment\n"), "{body}");
        let doc: serde_yaml::Value = serde_yaml::from_str(&body).unwrap();
        assert_eq!(Config::read_targets(&doc).unwrap(), vec![Target { path: "/x".into(), enabled: false, observe: Some(true), config: None }]);
        // A comment between the block and the next key survives a rewrite.
        std::fs::write(&path, "targets:\n  - path: /a\n    enabled: true\n# language setting\n\nlanguage: es\n").unwrap();
        Config::write_targets_to(&path, &[Target::new("/b")]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("# language setting\n\nlanguage: es\n"), "{body}");
        assert!(!body.contains("/a\n"), "{body}");
        // CRLF in, CRLF out.
        std::fs::write(&path, "language: es\r\n# note\r\n").unwrap();
        Config::write_targets_to(&path, &[Target::new("/c")]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("language: es\r\n# note\r\n"), "{body:?}");
        assert!(body.contains("targets:\r\n  - path: /c\r\n"), "{body:?}");
        assert!(!body.contains("\n\n") || body.contains("\r\n\r\n"), "no bare LF: {body:?}");
        // The comments-only template `ways config init` writes is accepted.
        std::fs::write(&path, "# only comments\n# targets:\n#   - path: ~/.claude\n").unwrap();
        Config::write_targets_to(&path, &[]).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("# only comments\n"));
        let doc: serde_yaml::Value = serde_yaml::from_str(&body).unwrap();
        assert_eq!(Config::read_targets(&doc), Some(Vec::new()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enabled_false_in_yaml_switches_the_layer_off() {
        let mut cfg = Config::default();
        assert!(cfg.enabled);
        cfg.apply_yaml("enabled: false\n");
        assert!(!cfg.enabled);
        cfg.apply_yaml("language: en\n");
        assert!(!cfg.enabled);
    }

    #[test]
    fn a_target_config_layers_over_the_user_config_for_its_sessions() {
        let base = std::env::temp_dir().join(format!("ways-target-cfg-{}", std::process::id()));
        let dir = base.join("claude-work");
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_file = base.join("work.yaml");
        std::fs::write(&cfg_file, "language: es\ndisabled_domains: [ea]\ntargets: [{path: /elsewhere}]\n").unwrap();
        let t = Target { path: dir.to_string_lossy().to_string(), enabled: true, observe: None, config: Some(cfg_file.to_string_lossy().to_string()) };
        assert!(t.matches_dir(&dir));
        assert!(!t.matches_dir(&base));
        assert_eq!(t.config_path(), cfg_file);
        // Apply the layer the way `load` does: same keys, `targets` never read.
        let mut cfg = Config { targets: Some(vec![t.clone()]), ..Default::default() };
        cfg.apply_yaml(&std::fs::read_to_string(t.config_path()).unwrap());
        assert_eq!(cfg.language, "es");
        assert_eq!(cfg.disabled_domains, vec!["ea".to_string()]);
        assert_eq!(cfg.targets().len(), 1, "a target layer cannot redirect the targets list");
        assert_eq!(cfg.targets()[0].path, t.path);
        // The default location is keyed by the directory.
        let d = Target::new(dir.to_string_lossy().to_string());
        assert!(d.config_path().starts_with(crate::paths::target_config_root(&dir)));
        assert!(d.config_path().ends_with("config.yaml"));
        std::fs::remove_dir_all(&base).ok();
    }

    fn apply_project(cfg: &mut Config, text: &str) {
        let doc = checked_text(text, None, LayerScope::Project, crate::settings::HOOK_SECTIONS).unwrap();
        cfg.apply_values(&doc);
        cfg.apply_project_ways_overlay_value(&doc);
    }

    #[test]
    fn a_bad_domain_list_never_switches_a_project_back_on() {
        let mut cfg = Config::default();
        apply_project(&mut cfg, "disabled_domains: ea,itops\nenabled: false\n");
        assert!(!cfg.enabled, "enabled: false is its own section");
    }

    #[test]
    fn one_bad_toggle_never_re_enables_the_other_disabled_ways() {
        let mut cfg = Config::default();
        apply_project(&mut cfg, "ways:\n  itops/incident: false\n  meta/introspection: no\n  ea/x:\n    enabled: false\n");
        assert_eq!(cfg.disabled_ways(), &["itops/incident".to_string(), "meta/introspection".to_string(), "ea/x".to_string()], "a bad toggle reads as disabled");
    }

    #[test]
    fn a_bad_secret_path_deny_keeps_the_targets() {
        let doc = checked_text(
            "secret_path_deny: \"false\"\ntargets:\n  - path: /a\n    enabled: false\n",
            None,
            LayerScope::User,
            crate::settings::HOOK_SECTIONS,
        )
        .unwrap();
        let t = doc.get("targets").and_then(Config::read_targets_value).unwrap();
        assert_eq!((t.len(), t[0].enabled), (1, false));
        assert_eq!(doc.get("secret_path_deny"), Some(&serde_yaml::Value::Bool(true)), "a bad value keeps the deny baseline");
    }

    #[test]
    fn a_refire_preset_above_one_is_valid() {
        let mut cfg = Config::default();
        cfg.apply_yaml("semantic_fire_probability: 0.35\nrefire_presets:\n  never: 5\n");
        assert_eq!(cfg.semantic_fire_probability, 0.35);
        assert_eq!(cfg.refire_presets.get("never").copied(), Some(5.0));
    }

    #[test]
    fn an_unparseable_file_fails_closed_for_its_scope() {
        // "Whole file fails closed" (#713): nothing the file says applies,
        // readable or not, and every switch its scope holds is off.
        let closed = |text: &str, scope| checked_text(text, None, scope, crate::settings::HOOK_SECTIONS).unwrap();
        let broken = "enabled: true\nlanguage: es\nways:\n  itops/incident: false\nx: [\n";
        // Project: ways are off for the project.
        assert_eq!(closed(broken, LayerScope::Project), serde_yaml::from_str::<serde_yaml::Value>("{enabled: false, secret_path_deny: true}").unwrap());
        // User: off, no projection target, the deny baseline merged.
        assert_eq!(
            closed(broken, LayerScope::User),
            serde_yaml::from_str::<serde_yaml::Value>("{enabled: false, targets: [], secret_path_deny: true}").unwrap()
        );
        // A target's file: its keys, which do not include targets.
        assert_eq!(closed(broken, LayerScope::Target), serde_yaml::from_str::<serde_yaml::Value>("{enabled: false, secret_path_deny: true}").unwrap());
        let mut cfg = Config::default();
        apply_project(&mut cfg, broken);
        assert!(!cfg.enabled);
        assert!(cfg.disabled_ways().is_empty(), "nothing the file says is read");
    }

    #[test]
    fn the_review_inputs_that_do_not_parse_fail_closed() {
        // N1-N3 shapes: each fails to parse, so the whole file is closed.
        for text in [
            "  language: en\n#ña\nenabled: false\n",
            "  language: en\nenabled: false\n",
            "enabled: \"false\n",
            "enabled: false\nenabled: true\n",
            "{enabled: false, x: [}\n",
            "\u{feff}enabled: false\nx: [\n",
            "  enabled: false\nlanguage: [\n",
            "\tenabled: false\n",
            "ways:\n\ta/b: false\n  c/d: false\n",
            "{note: \"{\", enabled: false, x: [}\n",
        ] {
            assert!(serde_yaml::from_str::<serde_yaml::Value>(text).is_err(), "{text:?} parses; test it as parsed");
            let mut cfg = Config::default();
            apply_project(&mut cfg, text);
            assert!(!cfg.enabled, "{text:?}");
        }
        // A BOM on a file that parses is read as written.
        let mut cfg = Config::default();
        apply_project(&mut cfg, "\u{feff}enabled: false\n");
        assert!(!cfg.enabled);
    }

    #[test]
    fn a_bad_switch_value_fails_closed() {
        let mut cfg = Config::default();
        apply_project(&mut cfg, "enabled: nope\ndisabled_domains: ea,itops\nways:\n  a/b: maybe\n");
        assert!(!cfg.enabled);
        assert_eq!(cfg.disabled_domains, vec!["ea".to_string(), "itops".to_string()]);
        assert_eq!(cfg.disabled_ways(), &["a/b".to_string()]);
        let mut cfg = Config { secret_path_deny: false, ..Default::default() };
        cfg.apply_yaml("secret_path_deny: \"false\"\n");
        assert!(cfg.secret_path_deny, "the deny baseline is the closed side");
    }

    #[test]
    fn one_bad_target_keeps_the_others_and_is_kept_disabled() {
        let doc = checked_text(
            "targets:\n  - path: ~/.claude\n    enabled: false\n  - path: ~/.claude-work\n    enabled: maybe\n",
            None,
            LayerScope::User,
            crate::settings::HOOK_SECTIONS,
        )
        .unwrap();
        let t = doc.get("targets").and_then(Config::read_targets_value).unwrap();
        assert_eq!(t.iter().map(|t| (t.path.as_str(), t.enabled)).collect::<Vec<_>>(), vec![("~/.claude", false), ("~/.claude-work", false)]);
    }

    #[test]
    fn loading_never_panics_on_arbitrary_bytes() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        const BYTES: &[u8] = b"enabled:targets-ways []{}\"'#\t \n\r\n,|&*!?false~\xef\xbb\xbf\xc3\xb1\xff";
        for _ in 0..5_000 {
            let len = (next() % 64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| BYTES[(next() % BYTES.len() as u64) as usize]).collect();
            let text = String::from_utf8_lossy(&bytes);
            for scope in [LayerScope::User, LayerScope::Target, LayerScope::Project] {
                if let Some(doc) = checked_text(&text, None, scope, crate::settings::HOOK_SECTIONS) {
                    let mut cfg = Config::default();
                    cfg.apply_values(&doc);
                    cfg.apply_project_ways_overlay_value(&doc);
                }
            }
        }
    }
}
