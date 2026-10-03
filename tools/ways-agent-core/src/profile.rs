//! Engine profiles and the user layer over them (ADR-196 §5, ADR-502 §5).
//!
//! The shipped profiles live in `profiles.yaml`, compiled in. The user layer at
//! `$XDG_CONFIG_HOME/agent-ways/agent.yaml` names the engine, sets the mode, and
//! patches any profile field; it is the user's file and survives updates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const SHIPPED: &str = include_str!("../profiles.yaml");

/// Shipped profiles in the order the agent picks one when the user names none:
/// the first whose provider has a key.
pub const SHIPPED_ORDER: &[&str] = &["anthropic", "openrouter"];

/// A hosted model provider. Each is an adapter behind one judge contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Openrouter,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Anthropic, Provider::Openrouter];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Openrouter => "openrouter",
        }
    }

    /// The environment variable that overrides this provider's key file.
    pub fn key_env(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Openrouter => "OPENROUTER_API_KEY",
        }
    }

    /// The model the shipped profile is tuned for, and the one the picker recommends.
    pub fn recommended_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-haiku-4-5",
            Provider::Openrouter => "anthropic/claude-haiku-4.5",
        }
    }

    /// True for the recommended model's id, alias or dated snapshot
    /// (`claude-haiku-4-5` and `claude-haiku-4-5-20251001`).
    pub fn is_recommended(self, model: &str) -> bool {
        let rec = self.recommended_model();
        model == rec
            || model
                .strip_prefix(rec)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|date| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
    }

    pub fn parse(s: &str) -> Result<Provider> {
        match s.to_ascii_lowercase().as_str() {
            "anthropic" => Ok(Provider::Anthropic),
            "openrouter" => Ok(Provider::Openrouter),
            other => bail!("unknown provider '{other}' (expected anthropic or openrouter)"),
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the gate does with a verdict (ADR-196 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Block candidates judged below the threshold. The default: a working key
    /// is the operator's approval to gate.
    #[default]
    Enforce,
    /// Judge and log every candidate; the matcher still decides.
    Shadow,
    /// Do not judge.
    Off,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Mode> {
        match s.to_ascii_lowercase().as_str() {
            "enforce" => Ok(Mode::Enforce),
            "shadow" => Ok(Mode::Shadow),
            "off" => Ok(Mode::Off),
            other => bail!("unknown mode '{other}' (expected enforce, shadow or off)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Enforce => "enforce",
            Mode::Shadow => "shadow",
            Mode::Off => "off",
        }
    }
}

/// One engine: a provider, a model, and the settings tuned for that model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub provider: Provider,
    pub model: String,
    /// P(yes) below this blocks a candidate in enforce mode.
    pub threshold: f64,
    /// The judge call's deadline. Past it the gate fails open.
    pub timeout_ms: u64,
    /// Conversation turns sent as context, counted back from the last.
    pub turns: usize,
    /// Each turn is cut to its last this-many characters.
    pub max_turn_chars: usize,
    /// Provider calls the agent runs at once, across all sessions.
    pub concurrency: usize,
    /// Candidates judged per request, taken in the matcher's order. Judge
    /// latency grows with each candidate, so the rest pass unjudged.
    pub max_candidates: usize,
    /// USD per million input tokens, for pricing a call whose provider does
    /// not report its cost. Unset: the model's list price where agent-ways
    /// ships one ([`crate::cost::list_price`]), else the cost is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_in_per_mtok: Option<f64>,
    /// USD per million output tokens. Prices apply as a pair: one set alone is
    /// ignored, never an error, since a price must not turn the gate off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_out_per_mtok: Option<f64>,
}

impl Profile {
    fn validate(&self, name: &str) -> Result<()> {
        if !(0.0..=1.0).contains(&self.threshold) {
            bail!("profile '{name}': threshold {} is outside 0..1", self.threshold);
        }
        if self.timeout_ms == 0 || self.timeout_ms > 60_000 {
            bail!("profile '{name}': timeout_ms {} is outside 1..60000", self.timeout_ms);
        }
        if self.turns == 0 || self.max_turn_chars == 0 || self.concurrency == 0 || self.max_candidates == 0 {
            bail!("profile '{name}': turns, max_turn_chars, concurrency and max_candidates must be at least 1");
        }
        if [self.price_in_per_mtok, self.price_out_per_mtok].into_iter().flatten().any(|p| !p.is_finite() || p < 0.0) {
            bail!("profile '{name}': prices must be zero or more");
        }
        if !valid_model_id(&self.model) {
            bail!("profile '{name}': model '{}' is not a model id (letters, digits and . _ : / - only)", self.model);
        }
        Ok(())
    }
}

/// A model id as providers write them: `claude-haiku-4-5`,
/// `anthropic/claude-haiku-4.5`, `…:batch`. It goes into request paths, so
/// nothing else is allowed.
pub fn valid_model_id(model: &str) -> bool {
    model.starts_with(|c: char| c.is_ascii_alphanumeric())
        && model.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
        && model.chars().all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c))
}

/// A user's change to one profile. Every field is optional; a profile name the
/// shipped set lacks must name its provider and model. Unknown fields are
/// caught by the settings schema, which falls the section back (ADR-503 §4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfilePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turn_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_candidates: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_in_per_mtok: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_out_per_mtok: Option<f64>,
}

impl ProfilePatch {
    fn apply(&self, base: &Profile) -> Profile {
        Profile {
            provider: self.provider.unwrap_or(base.provider),
            model: self.model.clone().unwrap_or_else(|| base.model.clone()),
            threshold: self.threshold.unwrap_or(base.threshold),
            timeout_ms: self.timeout_ms.unwrap_or(base.timeout_ms),
            turns: self.turns.unwrap_or(base.turns),
            max_turn_chars: self.max_turn_chars.unwrap_or(base.max_turn_chars),
            concurrency: self.concurrency.unwrap_or(base.concurrency),
            max_candidates: self.max_candidates.unwrap_or(base.max_candidates),
            price_in_per_mtok: self.price_in_per_mtok.or(base.price_in_per_mtok),
            price_out_per_mtok: self.price_out_per_mtok.or(base.price_out_per_mtok),
        }
    }
}

/// The user layer: `$XDG_CONFIG_HOME/agent-ways/agent.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserLayer {
    /// The profile the agent uses. Unset: the first shipped profile with a key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProfilePatch>,
}

pub fn user_layer_path() -> PathBuf {
    ways_core::paths::config_root().join("agent.yaml")
}

impl UserLayer {
    /// Reads the layer through the settings schema. A missing or empty file is
    /// an empty layer. A section that fails the schema, or a file that does
    /// not parse, loads as canonical with a diagnostic on stderr (ADR-503 §4);
    /// only an unreadable file is an error.
    pub fn load(path: &Path) -> Result<UserLayer> {
        let (layer, findings) = Self::load_with_findings(path)?;
        for f in findings {
            eprintln!("{}", f.diagnostic("ways"));
        }
        Ok(layer)
    }

    /// [`UserLayer::load`], returning the findings instead of printing them.
    pub fn load_with_findings(path: &Path) -> Result<(UserLayer, Vec<agent_settings::Finding>)> {
        // Read lossily, as the other settings loaders do: a stray byte in a
        // comment must not make the file unreadable.
        let text = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((UserLayer::default(), Vec::new())),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Self::parse(&text, Some(path))
    }

    /// Parse the layer's text, section by section.
    pub fn parse(text: &str, path: Option<&Path>) -> Result<(UserLayer, Vec<agent_settings::Finding>)> {
        use agent_settings::load;
        let doc = match load::parse_text(text, path) {
            Ok(d) => d,
            Err(f) => {
                // Whole file fails closed: nothing it says applies, and the
                // gate is off (`fails_closed` turns it off for the hook too).
                let closed = load::closed_file(
                    &crate::settings::SCHEMA,
                    crate::settings::FILE,
                    agent_settings::LayerScope::User,
                    Some(crate::settings::GATE_SECTIONS),
                );
                let layer = serde_yaml::from_value(serde_yaml::Value::Mapping(closed)).unwrap_or_default();
                return Ok((layer, vec![*f]));
            }
        };
        let checked = load::check(
            &crate::settings::SCHEMA,
            crate::settings::FILE,
            agent_settings::LayerScope::User,
            &doc,
            Some(crate::settings::GATE_SECTIONS),
        );
        let findings = if checked.is_clean() { Vec::new() } else { checked.findings(None, path, text) };
        let layer = serde_yaml::from_value(serde_yaml::Value::Mapping(checked.accepted))
            .with_context(|| format!("parsing {}", path.map(|p| p.display().to_string()).unwrap_or_default()))?;
        Ok((layer, findings))
    }
}

/// The finding that turns the relevance gate off, if any: agent.yaml does
/// not parse, or its `mode` failed the schema. The gate fails closed on
/// either, since off sends nothing anywhere (ADR-503 addendum).
pub fn fails_closed(findings: &[agent_settings::Finding]) -> Option<&agent_settings::Finding> {
    findings.iter().find(|f| f.is_parse_failure() || (f.fallback && f.unit.as_deref() == Some("gate.mode")))
}

/// The settings the gate runs with, from the layer at `path`, as [`resolve`]
/// gives them. A finding that fails closed is returned as the error, so the
/// caller turns the gate off and can log why.
pub fn gate_settings(path: &Path, has_key: impl Fn(Provider) -> bool) -> Result<Option<Settings>> {
    let (user, findings) = UserLayer::load_with_findings(path)?;
    if let Some(f) = fails_closed(&findings) {
        bail!("{f}; the gate is off until it is fixed");
    }
    for f in &findings {
        eprintln!("{}", f.diagnostic("ways"));
    }
    resolve(&user, has_key)
}

/// The shipped profiles, by name.
pub fn shipped() -> BTreeMap<String, Profile> {
    serde_yaml::from_str(SHIPPED).expect("profiles.yaml is valid; a test parses it")
}

/// Every profile after the user's patches: shipped ones patched, new ones built
/// from the shipped profile of the provider they name.
pub fn profiles(user: &UserLayer) -> Result<BTreeMap<String, Profile>> {
    let shipped = shipped();
    let mut all = shipped.clone();
    for (name, patch) in &user.profiles {
        all.insert(name.clone(), patched(name, patch, &shipped)?);
    }
    Ok(all)
}

/// One profile after the user's patch `patch`, as [`profiles`] builds it.
pub fn patched(name: &str, patch: &ProfilePatch, shipped: &BTreeMap<String, Profile>) -> Result<Profile> {
    // A changed provider needs its own model id: the base's id belongs to
    // the base's provider.
    if patch.provider.is_some() && patch.model.is_none() {
        bail!("profile '{name}' sets provider, so it must set model too");
    }
    let profile = match shipped.get(name) {
        Some(base) => patch.apply(base),
        None => {
            let (Some(provider), Some(_)) = (patch.provider, patch.model.as_ref()) else {
                bail!("profile '{name}' is not shipped, so it must set provider and model");
            };
            // New profiles inherit the shipped values for their provider,
            // never another user patch, whatever the names sort to.
            let base = shipped
                .get(provider.as_str())
                .cloned()
                .context("every provider has a shipped profile")?;
            patch.apply(&base)
        }
    };
    profile.validate(name)?;
    Ok(profile)
}

/// The settings the agent runs with.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub engine: String,
    /// Whether agent.yaml names the engine; false when it was picked by
    /// key order.
    pub engine_set: bool,
    pub profile: Profile,
    pub mode: Mode,
}

/// Resolves the engine. `None` means no engine is named and no provider has a
/// key: the gate is off and the hook keeps today's behaviour.
pub fn resolve(user: &UserLayer, has_key: impl Fn(Provider) -> bool) -> Result<Option<Settings>> {
    let all = profiles(user)?;
    let engine = match &user.engine {
        Some(name) => name.clone(),
        None => match SHIPPED_ORDER.iter().find(|n| all.get(**n).is_some_and(|p| has_key(p.provider))) {
            Some(name) => name.to_string(),
            None => return Ok(None),
        },
    };
    let profile = all
        .get(&engine)
        .cloned()
        .with_context(|| format!("engine '{engine}' names no profile"))?;
    Ok(Some(Settings { engine, engine_set: user.engine.is_some(), profile, mode: user.mode.unwrap_or_default() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_profiles_parse_validate_and_follow_the_order() {
        let all = shipped();
        for name in SHIPPED_ORDER {
            let p = &all[*name];
            p.validate(name).unwrap();
            assert_eq!(p.provider.as_str(), *name);
            assert_eq!(p.model, p.provider.recommended_model());
        }
        assert_eq!(all.len(), SHIPPED_ORDER.len());
    }

    #[test]
    fn the_recommended_model_matches_its_dated_snapshot_only() {
        assert!(Provider::Anthropic.is_recommended("claude-haiku-4-5"));
        assert!(Provider::Anthropic.is_recommended("claude-haiku-4-5-20251001"));
        assert!(!Provider::Anthropic.is_recommended("claude-haiku-4-5-beta"));
        assert!(!Provider::Openrouter.is_recommended("anthropic/claude-haiku-4.5:batch"));
    }

    #[test]
    fn no_key_and_no_engine_resolves_to_off() {
        assert_eq!(resolve(&UserLayer::default(), |_| false).unwrap(), None);
    }

    #[test]
    fn the_first_provider_with_a_key_is_picked_and_enforces() {
        let s = resolve(&UserLayer::default(), |p| p == Provider::Openrouter).unwrap().unwrap();
        assert_eq!(s.engine, "openrouter");
        assert_eq!(s.mode, Mode::Enforce);
        assert!(!s.engine_set, "picked by key order");
        let s = resolve(&UserLayer::default(), |_| true).unwrap().unwrap();
        assert_eq!(s.engine, "anthropic");
        let named: UserLayer = serde_yaml::from_str("engine: openrouter\n").unwrap();
        assert!(resolve(&named, |_| true).unwrap().unwrap().engine_set);
    }

    #[test]
    fn a_lone_price_never_turns_the_gate_off() {
        let lone: UserLayer = serde_yaml::from_str("profiles:\n  anthropic:\n    price_in_per_mtok: 3.0\n").unwrap();
        let s = resolve(&lone, |_| true).unwrap().unwrap();
        assert_eq!((s.profile.price_in_per_mtok, s.profile.price_out_per_mtok), (Some(3.0), None));
    }

    #[test]
    fn a_patch_overrides_only_its_fields() {
        let user: UserLayer =
            serde_yaml::from_str("mode: shadow\nprofiles:\n  anthropic:\n    threshold: 0.5\n").unwrap();
        let s = resolve(&user, |_| true).unwrap().unwrap();
        assert_eq!(s.mode, Mode::Shadow);
        assert_eq!(s.profile.threshold, 0.5);
        assert_eq!(s.profile.model, "claude-haiku-4-5");
    }

    #[test]
    fn a_new_profile_needs_provider_and_model() {
        let user: UserLayer = serde_yaml::from_str("profiles:\n  mine:\n    threshold: 0.4\n").unwrap();
        assert!(profiles(&user).is_err());
        let user: UserLayer = serde_yaml::from_str(
            "engine: mine\nprofiles:\n  mine:\n    provider: anthropic\n    model: claude-sonnet-5-5\n",
        )
        .unwrap();
        let s = resolve(&user, |_| false).unwrap().unwrap();
        assert_eq!(s.profile.model, "claude-sonnet-5-5");
        assert_eq!(s.profile.threshold, 0.3);
    }

    #[test]
    fn new_profiles_inherit_shipped_values_whatever_their_name() {
        for name in ["a-fast", "mine"] {
            let user: UserLayer = serde_yaml::from_str(&format!(
                "profiles:\n  anthropic:\n    threshold: 0.5\n  {name}:\n    provider: anthropic\n    model: claude-sonnet-5-5\n"
            ))
            .unwrap();
            assert_eq!(profiles(&user).unwrap()[name].threshold, 0.3, "{name}");
        }
    }

    #[test]
    fn changing_provider_needs_a_model_and_ids_are_checked() {
        let user: UserLayer = serde_yaml::from_str("profiles:\n  anthropic:\n    provider: openrouter\n").unwrap();
        assert!(profiles(&user).is_err());
        for bad in ["../messages", "a?b", "a#b", "/x", "", ".", "a/./b", "a//b", "-x"] {
            assert!(!valid_model_id(bad), "{bad}");
        }
        for good in ["claude-haiku-4-5", "anthropic/claude-haiku-4.5", "anthropic/claude-haiku-4.5:batch"] {
            assert!(valid_model_id(good), "{good}");
        }
    }

    #[test]
    fn bad_values_are_refused_by_resolve() {
        let user: UserLayer = serde_yaml::from_str("profiles:\n  anthropic:\n    threshold: 1.5\n").unwrap();
        assert!(profiles(&user).is_err());
        let user: UserLayer = serde_yaml::from_str("engine: nope\n").unwrap();
        assert!(resolve(&user, |_| true).is_err());
    }

    #[test]
    fn an_unknown_field_falls_its_section_back_and_the_other_loads() {
        // ADR-503 §4 replaces the deny_unknown_fields parse: one typo used to
        // reject the whole file and turn the gate off.
        let (user, findings) =
            UserLayer::parse("mode: shadow\nprofiles:\n  anthropic:\n    treshold: 0.4\n", Some(Path::new("/a.yaml"))).unwrap();
        assert_eq!(user.mode, Some(Mode::Shadow));
        assert!(user.profiles.is_empty());
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!((f.line, f.section.as_deref(), f.fallback), (Some(4), Some("gate.profiles"), true));
        assert!(f.to_string().contains("profiles.anthropic.treshold: unknown key"), "{f}");
        // A bad mode fails closed to off (ADR-503 addendum).
        let (user, findings) = UserLayer::parse("mode: loud\nprofiles:\n  anthropic:\n    threshold: 0.5\n", None).unwrap();
        assert_eq!(user.mode, Some(Mode::Off));
        assert_eq!(user.profiles["anthropic"].threshold, Some(0.5));
        assert_eq!(findings[0].section.as_deref(), Some("gate.mode"));
        // A bad engine never drops `mode: off`, and a typo in one profile
        // never drops another's tuning.
        let (user, _) = UserLayer::parse(
            "engine: 5\nmode: off\nprofiles:\n  anthropic:\n    threshold: 0.4\n  openrouter:\n    treshold: 0.3\n",
            None,
        )
        .unwrap();
        assert_eq!((user.engine, user.mode), (None, Some(Mode::Off)));
        assert_eq!(user.profiles["anthropic"].threshold, Some(0.4));
        assert!(!user.profiles.contains_key("openrouter"));
        // A top-level typo is reported, not silent.
        let (_, findings) = UserLayer::parse("mdoe: off\n", None).unwrap();
        assert_eq!(findings[0].key.as_deref(), Some("mdoe"));
        // An unreadable mode reads closed: off.
        let (user, findings) = UserLayer::parse("mode: [\n", None).unwrap();
        assert_eq!(user, UserLayer { mode: Some(Mode::Off), ..Default::default() });
        assert!(findings[0].message.contains("does not parse"));
    }

    fn gate_with(text: &str) -> Result<Option<Settings>> {
        let dir = std::env::temp_dir().join(format!("ways-gate-closed-{}-{}", std::process::id(), crate::keys::unique()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("agent.yaml");
        std::fs::write(&path, text).unwrap();
        let r = gate_settings(&path, |_| true);
        std::fs::remove_dir_all(&dir).ok();
        r
    }

    #[test]
    fn the_gate_fails_closed_on_a_broken_file_or_a_bad_mode() {
        // With a key present, each of these used to resolve to enforce.
        assert!(gate_with("mode: off\nengine: anthropic\nprofiles:\n  anthropic: [\n").is_err(), "parse failure is off");
        assert!(gate_with("engine: anthropic\nprofiles: [\n").is_err(), "parse failure with no mode is off too");
        assert!(gate_with("mode: of\n").is_err(), "a mistyped mode is off");
        assert!(matches!(gate_with("mode: off\nengine: 5\n"), Ok(Some(s)) if s.mode == Mode::Off), "a bad engine keeps mode: off");
        // The control: a sound file with a key runs the gate.
        assert!(matches!(gate_with("mode: shadow\n"), Ok(Some(s)) if s.mode == Mode::Shadow));
    }
}
