//! A judge profile as an item: shipped in `profiles.yaml`, compiled into
//! `ways-agent-core`, or the user's own under `gate.profiles.<name>` in
//! `agent.yaml`, which also extends a shipped one. Its kind is its provider
//! and its source its model.

use std::path::Path;

use agent_settings::item::{Entry, Family, How, Item, ItemKind, Origin, Override, ProfileState};
use serde::Serialize;
use ways_agent_core::profile::{self, Provider, UserLayer};

fn kind(p: Provider) -> ItemKind {
    match p {
        Provider::Anthropic => ItemKind::Anthropic,
        Provider::Openrouter => ItemKind::Openrouter,
        Provider::ClaudeCode => ItemKind::ClaudeCode,
    }
}

/// A profile has no fields of its own beyond the descriptor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileFields {}

/// Every profile, shipped ones first in name order, then the user's own,
/// from the agent file at `path`. The engine the gate resolves to is
/// `active`, judged with `has_key`; a profile of the user's that does not
/// build is `invalid`, and a shipped one whose change does not build runs
/// as shipped, saying so.
pub fn all(path: &Path, has_key: impl Fn(Provider) -> bool) -> Vec<Entry<ProfileFields>> {
    let user = UserLayer::load_with_findings(path).map(|(u, _)| u).unwrap_or_default();
    let shipped = profile::shipped();
    let built = profile::profiles(&user);
    let dropped = profile::dropped_profiles(&user);
    let engine = profile::resolve(&user, has_key).ok().flatten().map(|s| s.engine);
    let names = shipped.keys().chain(user.profiles.keys().filter(|n| !shipped.contains_key(*n)));
    names
        .map(|name| {
            let ours = user.profiles.contains_key(name);
            let why = dropped.iter().find(|(n, _)| n == name).map(|(_, e)| format!("{e:#}"));
            let mut item = match shipped.contains_key(name) {
                true => Item::new(Family::Profile, name, Origin::Builtin),
                false => Item { path: Some(path.to_path_buf()), ..Item::new(Family::Profile, name, Origin::User) },
            };
            if ours && shipped.contains_key(name) {
                item = item.with_overrides(vec![Override { origin: Origin::User, how: How::Extends, path: path.to_path_buf() }]);
            }
            let patch = user.profiles.get(name);
            let (provider, model) = match built.get(name) {
                Some(p) => (Some(p.provider), Some(p.model.clone())),
                None => (patch.and_then(|p| p.provider), patch.and_then(|p| p.model.clone())),
            };
            item.kind = provider.map(kind);
            item.source = model;
            let state = match (&why, built.contains_key(name), engine.as_deref() == Some(name.as_str())) {
                (Some(_), false, _) => ProfileState::Invalid,
                (_, _, true) => ProfileState::Active,
                _ => ProfileState::Inactive,
            };
            item.detail = match (why, state) {
                (Some(e), ProfileState::Invalid) => Some(e),
                (Some(e), _) => Some(format!("agent.yaml's change is dropped: {e}")),
                (None, _) => None,
            };
            Entry { item: item.with_state(state), fields: ProfileFields {} }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_reads_its_provider_model_origin_and_whether_it_is_the_engine() {
        let d = std::env::temp_dir().join(format!("ways-items-profiles-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("agent.yaml");
        let text = "engine: mine\nprofiles:\n  anthropic:\n    threshold: 0.4\n  mine:\n    provider: openrouter\n    model: vendor/model-1\n  bad:\n    provider: anthropic\n";
        std::fs::write(&path, text).unwrap();
        let all = all(&path, |_| false);
        let get = |n: &str| all.iter().find(|e| e.item.name == n).unwrap_or_else(|| panic!("no {n}")).item.clone();

        let anthropic = get("anthropic");
        assert_eq!((anthropic.origin, anthropic.kind, anthropic.path.as_deref()), (Origin::Builtin, Some(ItemKind::Anthropic), None));
        assert_eq!(anthropic.overrides, [Override { origin: Origin::User, how: How::Extends, path: path.clone() }]);
        assert_eq!(anthropic.state, ProfileState::Inactive.into());
        let cc = get("claude-code");
        assert_eq!((cc.kind, cc.overrides.len()), (Some(ItemKind::ClaudeCode), 0));

        let mine = get("mine");
        assert_eq!((mine.origin, mine.kind, mine.source.as_deref()), (Origin::User, Some(ItemKind::Openrouter), Some("vendor/model-1")));
        assert_eq!((mine.path.as_deref(), mine.state), (Some(path.as_path()), ProfileState::Active.into()));
        let bad = get("bad");
        assert_eq!((bad.origin, bad.state), (Origin::User, ProfileState::Invalid.into()));
        assert!(bad.detail.as_deref().is_some_and(|d| d.contains("must set model")), "{:?}", bad.detail);
        let _ = std::fs::remove_dir_all(&d);
    }
}
