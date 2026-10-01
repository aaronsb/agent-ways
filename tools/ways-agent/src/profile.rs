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
}

impl Profile {
    fn validate(&self, name: &str) -> Result<()> {
        if !(0.0..=1.0).contains(&self.threshold) {
            bail!("profile '{name}': threshold {} is outside 0..1", self.threshold);
        }
        if self.timeout_ms == 0 || self.timeout_ms > 60_000 {
            bail!("profile '{name}': timeout_ms {} is outside 1..60000", self.timeout_ms);
        }
        if self.turns == 0 || self.max_turn_chars == 0 || self.concurrency == 0 {
            bail!("profile '{name}': turns, max_turn_chars and concurrency must be at least 1");
        }
        if self.model.trim().is_empty() {
            bail!("profile '{name}': model is empty");
        }
        Ok(())
    }
}

/// A user's change to one profile. Every field is optional; a profile name the
/// shipped set lacks must name its provider and model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
}

impl ProfilePatch {
    fn is_empty(&self) -> bool {
        *self == ProfilePatch::default()
    }

    fn apply(&self, base: &Profile) -> Profile {
        Profile {
            provider: self.provider.unwrap_or(base.provider),
            model: self.model.clone().unwrap_or_else(|| base.model.clone()),
            threshold: self.threshold.unwrap_or(base.threshold),
            timeout_ms: self.timeout_ms.unwrap_or(base.timeout_ms),
            turns: self.turns.unwrap_or(base.turns),
            max_turn_chars: self.max_turn_chars.unwrap_or(base.max_turn_chars),
            concurrency: self.concurrency.unwrap_or(base.concurrency),
        }
    }
}

/// The user layer: `$XDG_CONFIG_HOME/agent-ways/agent.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Reads the layer. A missing or empty file is an empty layer; a malformed
    /// one is an error naming the file.
    pub fn load(path: &Path) -> Result<UserLayer> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(UserLayer::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if text.trim().is_empty() {
            return Ok(UserLayer::default());
        }
        serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Writes the layer through a temporary file and a rename, so a reader
    /// never sees half a file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut layer = self.clone();
        layer.profiles.retain(|_, p| !p.is_empty());
        let body = format!(
            "# The ways agent's user layer (ADR-196 §5). Fields here override the\n\
             # shipped engine profiles and survive updates. `ways agent config`\n\
             # shows the resolved settings.\n{}",
            serde_yaml::to_string(&layer)?
        );
        let dir = path.parent().context("user layer path has no parent")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = dir.join(".agent.yaml.tmp");
        std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }
}

/// The shipped profiles, by name.
pub fn shipped() -> BTreeMap<String, Profile> {
    serde_yaml::from_str(SHIPPED).expect("profiles.yaml is valid; a test parses it")
}

/// Every profile after the user's patches: shipped ones patched, new ones built
/// from the shipped profile of the provider they name.
pub fn profiles(user: &UserLayer) -> Result<BTreeMap<String, Profile>> {
    let mut all = shipped();
    for (name, patch) in &user.profiles {
        let profile = match all.get(name) {
            Some(base) => patch.apply(base),
            None => {
                let (Some(provider), Some(_)) = (patch.provider, patch.model.as_ref()) else {
                    bail!("profile '{name}' is not shipped, so it must set provider and model");
                };
                let base = all
                    .get(provider.as_str())
                    .cloned()
                    .context("every provider has a shipped profile")?;
                patch.apply(&base)
            }
        };
        profile.validate(name)?;
        all.insert(name.clone(), profile);
    }
    Ok(all)
}

/// The settings the agent runs with.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub engine: String,
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
    Ok(Some(Settings { engine, profile, mode: user.mode.unwrap_or_default() }))
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
        let s = resolve(&UserLayer::default(), |_| true).unwrap().unwrap();
        assert_eq!(s.engine, "anthropic");
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
    fn bad_values_and_unknown_fields_are_refused() {
        let user: UserLayer = serde_yaml::from_str("profiles:\n  anthropic:\n    threshold: 1.5\n").unwrap();
        assert!(profiles(&user).is_err());
        assert!(serde_yaml::from_str::<UserLayer>("profiles:\n  anthropic:\n    treshold: 0.4\n").is_err());
        let user: UserLayer = serde_yaml::from_str("engine: nope\n").unwrap();
        assert!(resolve(&user, |_| true).is_err());
    }

    #[test]
    fn the_layer_round_trips_and_drops_empty_patches() {
        let dir = std::env::temp_dir().join(format!("ways-agent-profile-{}", std::process::id()));
        let path = dir.join("agent.yaml");
        let mut layer = UserLayer { mode: Some(Mode::Off), ..Default::default() };
        layer.profiles.insert("anthropic".into(), ProfilePatch::default());
        layer.save(&path).unwrap();
        let back = UserLayer::load(&path).unwrap();
        assert_eq!(back.mode, Some(Mode::Off));
        assert!(back.profiles.is_empty());
        assert_eq!(UserLayer::load(&dir.join("missing.yaml")).unwrap(), UserLayer::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
