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
}
