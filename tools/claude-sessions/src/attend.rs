//! The names attend gives a project's signal tray and instance registry.
//!
//! A tray is a delivery address: two projects must never share one, or a
//! directed message reaches a session it was not meant for. [`project_slug`]
//! alone is lossy (`/srv/my proj` and `/srv/my-proj` both give
//! `-srv-my-proj`; two all-CJK names of one length collide), so attend adds
//! the hash of the full path. The slug stays the readable prefix, and the
//! part before the last `-` is the slug, which is how cleanup finds the
//! project a tray belongs to.

use crate::slug::{base36, path_hash, project_slug};

/// The tray and registry name of the project at `path`:
/// `project_slug(path)` + `-` + base 36 of Claude Code's path hash over the
/// full path. Empty for an empty path.
pub fn attend_key(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    format!("{}-{}", project_slug(path), base36(path_hash(path).unsigned_abs()))
}

/// The project-slug part of an [`attend_key`]: everything before its last
/// `-`. `None` when the name holds no `-` or holds a character an attend
/// key never does.
pub fn attend_key_slug(name: &str) -> Option<&str> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let (slug, hash) = name.rsplit_once('-')?;
    (!hash.is_empty()).then_some(slug)
}

// transition read: removed by #701 (ADR-506)
/// `path` with `/ _ .` and the `extra` characters mapped to `-` and every
/// other character kept: attend's naming before [`attend_key`].
fn old_rule(path: &str, extra: &[char]) -> String {
    path.chars()
        .map(|c| if matches!(c, '/' | '_' | '.') || extra.contains(&c) { '-' } else { c })
        .collect()
}

// transition read: removed by #701 (ADR-506)
/// The names a project's signal tray had before [`attend_key`]. Trays were
/// written under two rules: `attend` and `sensor-peers` mapped
/// `/ _ . \ :` to `-`; `attend-chat` mapped only `/ _ .`. Both forms,
/// without duplicates, none equal to the current key.
pub fn legacy_tray_names(path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    let current = attend_key(path);
    let mut out: Vec<String> = Vec::new();
    for name in [old_rule(path, &['\\', ':']), old_rule(path, &[])] {
        if name != current && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

// transition read: removed by #701 (ADR-506)
/// The name `attend-instances` gave a project's registry before
/// [`attend_key`]: it mapped only `/ _ .`. The wider tray rule is not
/// probed: under it `/x/a:b` reads as `-x-a-b`, the registry of `/x/a/b`.
pub fn legacy_registry_name(path: &str) -> Option<String> {
    (!path.is_empty()).then(|| old_rule(path, &[]))
}

/// The tray names to read for the project at `path`: [`attend_key`] first,
/// then, during the transition, [`legacy_tray_names`]. Empty for an empty
/// path. Sends go to the first name only.
pub fn attend_tray_names(path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut names = vec![attend_key(path)];
    // transition read: removed by #701 (ADR-506)
    names.extend(legacy_tray_names(path));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_of_colliding_slugs_differ() {
        // Both pairs share a project slug.
        assert_eq!(project_slug("/srv/my proj"), project_slug("/srv/my-proj"));
        assert_ne!(attend_key("/srv/my proj"), attend_key("/srv/my-proj"));
        assert_eq!(project_slug("/home/u/项目"), project_slug("/home/u/工作"));
        assert_ne!(attend_key("/home/u/项目"), attend_key("/home/u/工作"));
    }

    #[test]
    fn key_is_slug_dash_hash() {
        // Hash suffixes computed with Claude Code's qZ under node.
        let key = attend_key("/srv/my proj");
        assert_eq!(key, "-srv-my-proj-bte5w6");
        assert_eq!(attend_key("/srv/my-proj"), "-srv-my-proj-bm8u6h");
        assert_eq!(attend_key_slug(&key), Some("-srv-my-proj"));
        assert_eq!(attend_key(""), "");
    }

    #[test]
    fn key_slug_rejects_legacy_names() {
        assert_eq!(attend_key_slug("-srv-my proj"), None);
        assert_eq!(attend_key_slug("nodash"), None);
    }

    #[test]
    fn legacy_names_cover_both_old_rules() {
        assert_eq!(legacy_tray_names("/home/a/.claude"), vec!["-home-a--claude".to_string()]);
        assert_eq!(legacy_tray_names("/x/a b:c"), vec!["-x-a b-c".to_string(), "-x-a b:c".to_string()]);
        assert!(legacy_tray_names("").is_empty());
        // Registries were written under the narrow rule only.
        assert_eq!(legacy_registry_name("/x/a:b").as_deref(), Some("-x-a:b"));
        assert_eq!(legacy_registry_name(""), None);
    }

    #[test]
    fn tray_names_put_the_key_first() {
        let names = attend_tray_names("/x/a_b");
        assert_eq!(names, vec![attend_key("/x/a_b"), "-x-a-b".to_string()]);
        assert!(attend_tray_names("").is_empty());
    }
}
