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

    /// The model the shipped profile uses, and the one the picker recommends:
    /// Claude Haiku 5.5. The threshold was measured on Haiku 4.5.
    pub fn recommended_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-haiku-5-5",
            Provider::Openrouter => "anthropic/claude-haiku-5.5",
        }
    }

    /// True for the recommended model's id, alias or dated snapshot
    /// (`claude-haiku-5-5`, and a dated form such as `claude-haiku-5-5-20260101` if one appears).
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

/// Whether a request to `model` may carry `temperature: 0`.
///
/// True only for ids known to accept it: Claude 3.x, 4.0 and 4.1 through 4.6
/// (including dated snapshots and the OpenRouter `anthropic/` forms). Opus 4.7
/// and later, and every Claude 5.x model, return HTTP 400 to any non-default
/// sampling parameter, so those, and any Claude id this function does not
/// recognise, get none. A non-Anthropic model reached through OpenRouter keeps
/// `temperature: 0`: those providers accept it, and the judge wants
/// deterministic verdicts from them.
pub fn accepts_sampling(model: &str) -> bool {
    let id = model.split(':').next().unwrap_or(model);
    let claude = match id.split_once('/') {
        Some(("anthropic", rest)) => rest,
        Some(_) => return true,
        None => id,
    };
    let Some(rest) = claude.strip_prefix("claude-") else { return false };
    // Version numbers are the short all-digit segments; the dotted OpenRouter
    // form (`4.5`) splits on '.' as well. An 8-digit date is not a minor.
    let mut nums = rest
        .split(['-', '.'])
        .filter(|seg| !seg.is_empty() && seg.len() <= 2 && seg.bytes().all(|b| b.is_ascii_digit()));
    match (nums.next(), nums.next()) {
        (Some("3"), _) => true,
        (Some("4"), None) => true,
        (Some("4"), Some(minor)) => minor.parse::<u8>().is_ok_and(|m| m <= 6),
        _ => false,
    }
}

/// Whether `model` accepts the forced `tool_choice` the judge sends.
///
/// Opus 5.5, Sonnet 5.5, Fable 5.1 and Mythos 5.1 return HTTP 400 to a forced
/// tool call; every other model accepts it. Matches both the dashed and the
/// dotted id forms, with or without a date or `:variant` suffix.
pub fn accepts_forced_tool(model: &str) -> bool {
    let id = model.split(':').next().unwrap_or(model).replace('.', "-");
    let rejecting = ["opus-5-5", "sonnet-5-5", "fable-5-1", "mythos-5-1"];
    !rejecting.iter().any(|needle| {
        id.match_indices(needle).any(|(i, _)| {
            let before = id[..i].chars().next_back();
            let after = id[i + needle.len()..].chars().next();
            before.is_none_or(|c| c == '-' || c == '/') && after.is_none_or(|c| c == '-')
        })
    })
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
    for (name, e) in dropped_profiles(&user) {
        match (shipped().contains_key(&name), user.engine.as_deref() == Some(name.as_str())) {
            (true, _) => eprintln!("ways: agent.yaml: {e:#}; the change is dropped and profile '{name}' runs as shipped"),
            (false, false) => eprintln!("ways: agent.yaml: {e:#}; profile '{name}' is left out"),
            // The engine's own: resolve fails with this reason.
            (false, true) => {}
        }
    }
    resolve(&user, has_key)
}

/// The shipped profiles, by name.
pub fn shipped() -> BTreeMap<String, Profile> {
    serde_yaml::from_str(SHIPPED).expect("profiles.yaml is valid; a test parses it")
}

/// Every profile after the user's patches: shipped ones patched, new ones built
/// from the shipped profile of the provider they name. A patch that does not
/// build is dropped and the others load (ADR-503 addendum: each profile is
/// its own unit), as a patch that fails the schema is: a shipped profile
/// falls back to its shipped definition, a profile of your own is left out.
/// The settings' list of profile names agrees. [`dropped_profiles`] says why.
pub fn profiles(user: &UserLayer) -> BTreeMap<String, Profile> {
    let shipped = shipped();
    let mut all = shipped.clone();
    for (name, patch) in &user.profiles {
        if let Ok(p) = patched(name, patch, &shipped) {
            all.insert(name.clone(), p);
        }
    }
    all
}

/// The patches [`profiles`] drops, each with why.
pub fn dropped_profiles(user: &UserLayer) -> Vec<(String, anyhow::Error)> {
    let shipped = shipped();
    user.profiles
        .iter()
        .filter_map(|(name, patch)| patched(name, patch, &shipped).err().map(|e| (name.clone(), e)))
        .collect()
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
    let all = profiles(user);
    if let Some(name) = user.engine.as_ref().filter(|n| !all.contains_key(*n)) {
        // The engine is a profile of your own that does not build: name
        // why, and the gate is off.
        if let Some((_, e)) = dropped_profiles(user).into_iter().find(|(n, _)| n == name) {
            return Err(e.context(format!("engine '{name}' names a profile that does not build")));
        }
    }
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
        assert!(Provider::Anthropic.is_recommended("claude-haiku-5-5"));
        assert!(Provider::Anthropic.is_recommended("claude-haiku-5-5-20260101"));
        assert!(!Provider::Anthropic.is_recommended("claude-haiku-4-5"));
        assert!(!Provider::Anthropic.is_recommended("claude-haiku-5-5-beta"));
        assert!(Provider::Openrouter.is_recommended("anthropic/claude-haiku-5.5"));
        assert!(!Provider::Openrouter.is_recommended("anthropic/claude-haiku-5.5:batch"));
    }

    #[test]
    fn only_legacy_models_accept_sampling() {
        for yes in [
            "claude-haiku-4-5",
            "claude-haiku-4-5-20251001",
            "claude-3-5-haiku-latest",
            "claude-3-haiku-20240307",
            "claude-sonnet-4-20250514",
            "anthropic/claude-haiku-4.5",
            "anthropic/claude-3.5-haiku",
            "anthropic/claude-haiku-4.5:batch",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-opus-4-1-20250805",
            "anthropic/claude-sonnet-4.6",
            "openai/gpt-5",
            "google/gemini-2.5-flash",
        ] {
            assert!(accepts_sampling(yes), "{yes}");
        }
        for no in [
            "claude-haiku-5-5",
            "claude-haiku-5-5-20260101",
            "claude-sonnet-5-5",
            "claude-opus-4-7",
            "claude-opus-4-8-20260301",
            "anthropic/claude-opus-4.7",
            "anthropic/claude-opus-4.8:batch",
            "anthropic/claude-haiku-5.5",
            "anthropic/claude-haiku-5.5:batch",
            "some-new-model",
            "claude-future",
        ] {
            assert!(!accepts_sampling(no), "{no}");
        }
    }

    #[test]
    fn forced_tool_is_rejected_only_by_four_models() {
        for no in [
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-sonnet-5-5-20260101",
            "claude-fable-5-1",
            "claude-mythos-5-1",
            "anthropic/claude-opus-5.5",
            "anthropic/claude-sonnet-5.5:batch",
            "anthropic/claude-fable-5.1",
            "anthropic/claude-mythos-5.1",
        ] {
            assert!(!accepts_forced_tool(no), "{no}");
        }
        for yes in [
            "claude-haiku-5-5",
            "anthropic/claude-haiku-5.5",
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-fable-5",
            "claude-opus-4-7",
            "claude-sonnet-4-20250514",
            "claude-haiku-4-5",
            "openai/gpt-5",
        ] {
            assert!(accepts_forced_tool(yes), "{yes}");
        }
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
        assert_eq!(s.profile.model, "claude-haiku-5-5");
    }

    #[test]
    fn a_new_profile_needs_provider_and_model() {
        let user: UserLayer = serde_yaml::from_str("profiles:\n  mine:\n    threshold: 0.4\n").unwrap();
        assert!(!profiles(&user).contains_key("mine"));
        assert_eq!(dropped_profiles(&user).len(), 1);
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
            assert_eq!(profiles(&user)[name].threshold, 0.3, "{name}");
        }
    }

    #[test]
    fn changing_provider_needs_a_model_and_ids_are_checked() {
        let user: UserLayer = serde_yaml::from_str("profiles:\n  anthropic:\n    provider: openrouter\n").unwrap();
        assert_eq!(profiles(&user)["anthropic"], shipped()["anthropic"], "the patch is dropped, the shipped profile kept");
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
        assert_eq!(profiles(&user)["anthropic"].threshold, 0.3, "the bad patch is dropped");
        let user: UserLayer = serde_yaml::from_str("engine: mine\nprofiles:\n  mine:\n    threshold: 0.4\n").unwrap();
        assert!(resolve(&user, |_| true).is_err(), "the engine is a custom profile that does not build: the gate is off");
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

    #[test]
    fn a_profile_that_does_not_build_is_left_out_and_the_gate_still_runs() {
        // ADR-503 addendum: each profile is its own unit. A custom profile
        // with no model used to turn the whole gate off.
        let user: UserLayer = serde_yaml::from_str(
            "engine: openrouter\nprofiles:\n  mine:\n    threshold: 0.4\n  openrouter:\n    threshold: 0.5\n",
        )
        .unwrap();
        let s = resolve(&user, |_| true).unwrap().unwrap();
        assert_eq!((s.engine.as_str(), s.profile.threshold), ("openrouter", 0.5));
        assert_eq!(dropped_profiles(&user).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["mine"]);
        // A shipped profile's bad patch is dropped and the shipped profile
        // runs, named engine or picked by key order: with only an Anthropic
        // key the gate still runs on anthropic, never off for want of a key.
        for text in [
            "profiles:\n  anthropic:\n    provider: openrouter\n",
            "engine: anthropic\nprofiles:\n  anthropic:\n    provider: openrouter\n",
        ] {
            let user: UserLayer = serde_yaml::from_str(text).unwrap();
            let s = resolve(&user, |p| p == Provider::Anthropic).unwrap().unwrap();
            assert_eq!((s.engine.as_str(), &s.profile), ("anthropic", &shipped()["anthropic"]), "{text:?}");
            assert_eq!(dropped_profiles(&user).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["anthropic"]);
        }
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
