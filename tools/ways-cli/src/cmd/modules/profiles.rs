//! A judge profile as an item: shipped in `profiles.yaml`, compiled into
//! `ways-agent-core`, or the user's own under `gate.profiles.<name>` in
//! `agent.yaml`, which also extends a shipped one. Its kind is its provider
//! and its source its model.

use std::collections::{BTreeMap, BTreeSet};
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
/// pass the schema or does not build is `invalid`, and a shipped one whose
/// change does not pass or build runs as shipped, saying so. When the file
/// turns the gate off (it does not parse, or its `mode` is refused) no
/// profile is `active` and each says why.
pub fn all(path: &Path, has_key: impl Fn(Provider) -> bool) -> Vec<Entry<ProfileFields>> {
    let (user, findings) = UserLayer::load_with_findings(path).unwrap_or_default();
    let closed = profile::fails_closed(&findings).map(|f| format!("{f}; the gate is off until it is fixed"));
    // The first finding of each `gate.profiles.<name>` unit that fell out.
    let mut refused: BTreeMap<&str, String> = BTreeMap::new();
    for f in &findings {
        if let Some(name) = f.unit.as_deref().and_then(|u| u.strip_prefix("gate.profiles.")) {
            refused.entry(name).or_insert_with(|| f.to_string());
        }
    }
    let shipped = profile::shipped();
    let built = profile::profiles(&user);
    let dropped = profile::dropped_profiles(&user);
    let engine = match closed {
        Some(_) => None,
        None => profile::resolve(&user, has_key).ok().flatten().map(|s| s.engine),
    };
    let extra: BTreeSet<String> = user.profiles.keys().cloned().chain(refused.keys().map(|n| n.to_string())).filter(|n| !shipped.contains_key(n)).collect();
    let names = shipped.keys().cloned().chain(extra);
    names
        .map(|name| {
            let ours = user.profiles.contains_key(&name);
            let why = dropped.iter().find(|(n, _)| *n == name).map(|(_, e)| format!("{e:#}"));
            let why = why.or_else(|| refused.get(name.as_str()).cloned());
            let mut item = match shipped.contains_key(&name) {
                true => Item::new(Family::Profile, name.clone(), Origin::Builtin),
                false => Item { path: Some(path.to_path_buf()), ..Item::new(Family::Profile, name.clone(), Origin::User) },
            };
            if ours && shipped.contains_key(&name) {
                item = item.with_overrides(vec![Override { origin: Origin::User, how: How::Extends, path: path.to_path_buf() }]);
            }
            let patch = user.profiles.get(&name);
            let (provider, model) = match built.get(&name) {
                Some(p) => (Some(p.provider), Some(p.model.clone())),
                None => (patch.and_then(|p| p.provider), patch.and_then(|p| p.model.clone())),
            };
            item.kind = provider.map(kind);
            item.source = model;
            let state = match (&why, built.contains_key(&name), engine.as_deref() == Some(name.as_str())) {
                (Some(_), false, _) => ProfileState::Invalid,
                (_, _, true) => ProfileState::Active,
                _ => ProfileState::Inactive,
            };
            let own = match (why, state) {
                (Some(e), ProfileState::Invalid) => Some(e),
                (Some(e), _) => Some(format!("agent.yaml's change is dropped: {e}")),
                (None, _) => None,
            };
            item.detail = match (own, &closed) {
                (Some(o), Some(c)) => Some(format!("{o}; {c}")),
                (o, c) => o.or_else(|| c.clone()),
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

    fn listed(tag: &str, text: &str) -> (Vec<Entry<ProfileFields>>, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("ways-items-profiles-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("agent.yaml");
        std::fs::write(&path, text).unwrap();
        (all(&path, |_| true), d)
    }

    fn get(all: &[Entry<ProfileFields>], n: &str) -> Item {
        all.iter().find(|e| e.item.name == n).unwrap_or_else(|| panic!("no {n}")).item.clone()
    }

    #[test]
    fn a_user_profile_that_fails_the_schema_is_listed_invalid_with_the_finding() {
        let (all, d) = listed("schema-user", "profiles:\n  typo:\n    provider: openrouter\n    model: vendor/m\n    threshold: high\n");
        let typo = get(&all, "typo");
        assert_eq!((typo.origin, typo.state), (Origin::User, ProfileState::Invalid.into()));
        assert!(typo.detail.as_deref().is_some_and(|d| d.contains("threshold")), "{:?}", typo.detail);
        assert_eq!(all.iter().filter(|e| e.item.name == "typo").count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_shipped_profile_whose_patch_fails_the_schema_keeps_its_state_and_names_the_drop() {
        let (all, d) = listed("schema-shipped", "engine: anthropic\nprofiles:\n  anthropic:\n    threshold: lots\n");
        let a = get(&all, "anthropic");
        assert_eq!((a.origin, a.state), (Origin::Builtin, ProfileState::Active.into()));
        assert!(a.detail.as_deref().is_some_and(|d| d.contains("dropped") && d.contains("threshold")), "{:?}", a.detail);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_turns_the_gate_off_leaves_no_profile_active_and_says_why() {
        for (tag, text) in [("closed-mode", "mode: bogus\nengine: anthropic\n"), ("closed-parse", "engine: anthropic\nprofiles: [\n")] {
            let (all, d) = listed(tag, text);
            assert!(all.iter().all(|e| e.item.state != ProfileState::Active.into()), "{tag}");
            assert!(all.iter().all(|e| e.item.detail.as_deref().is_some_and(|d| d.contains("the gate is off"))), "{tag}: {:?}", get(&all, "anthropic").detail);
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}
