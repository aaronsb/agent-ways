//! Named group management for attend (ADR-118).
//!
//! Groups are named signal namespaces that agents focus on and release.
//! Storage: `@group-name/` directories under the signals base, with membership
//! tracked in `_groups.yaml`.
//!
//! Every agent is always in its implicit project group (from cwd).
//! Named groups are explicit and opt-in via `attend focus on <name>`.
//!
//! The state manager and `_groups.yaml` I/O live in the shared
//! `attend-groups` crate (ADR-170) — attend-chat's `/join` write path
//! uses the same implementation, so the wire format has a single
//! owner. What remains here is attend-specific: the one-shot ADR-124
//! `@open/` migration.

use std::fs;
use std::path::Path;

pub use attend_groups::Groups;

/// One-shot migration for the legacy `@open/` focus group.
///
/// Pre-ADR-124 the `open` scene would create an `@open/` dir alongside
/// `_broadcast/`; post-ADR the base channel is `_broadcast/` and the
/// display name is `#open` (no `@open/` on disk). This helper makes
/// `attend run` idempotently clean up lingering state on the next
/// startup after an upgrade:
///
/// - move any `*.signal` files from `@open/` into `_broadcast/`
/// - remove the `@open/` dir and — if present — strip the `open:`
///   entry from `_groups.yaml`
///
/// **Non-signal files are destroyed.** The directory is removed
/// wholesale after the signal files are migrated; any `.tmp`, stray
/// lockfiles, or hand-placed notes under `@open/` go with it.
/// That's acceptable under attend's "only attend writes to its own
/// signal base" contract — no legitimate caller should have put
/// anything else there — but noted explicitly so nobody is surprised
/// later.
///
/// Returns the number of signal files moved, or `None` if there was
/// nothing to migrate (the common case after the first post-upgrade
/// run). `Some(0)` means `@open/` existed but had no signal files
/// to move — we still removed the dir, which is worth logging.
pub fn migrate_legacy_open_group(signals_base: &Path, groups: &Groups) -> Option<usize> {
    let open_dir = signals_base.join("@open");
    if !open_dir.is_dir() {
        return None;
    }
    let broadcast_dir = signals_base.join("_broadcast");
    fs::create_dir_all(&broadcast_dir).ok();

    let mut moved = 0;
    if let Ok(entries) = fs::read_dir(&open_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let src = entry.path();
            if src.extension().and_then(|s| s.to_str()) != Some("signal") {
                continue;
            }
            let Some(name) = src.file_name() else { continue };
            let dst = broadcast_dir.join(name);
            // Prefer rename (atomic, intra-fs); fall back to copy+remove
            // when rename fails (e.g. cross-device — unlikely here but
            // defensive). Ignore errors per-file: best-effort migration.
            if fs::rename(&src, &dst).is_ok() {
                moved += 1;
            } else if fs::copy(&src, &dst).is_ok() {
                fs::remove_file(&src).ok();
                moved += 1;
            }
        }
    }
    // Only drive `dissolve` (full `_groups.yaml` rewrite) when the
    // yaml actually has an `open:` entry to remove — otherwise the
    // migration pays a read-modify-write on every startup for no
    // reason, widening a race window against peer-session edits on
    // shared signal bases.
    if groups.has_group("open") {
        groups.dissolve("open");
    } else {
        fs::remove_dir_all(&open_dir).ok();
    }
    Some(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_legacy_open_noop_when_absent() {
        // No `@open/` → returns None. Idempotent fast path on
        // subsequent startups.
        let base = std::env::temp_dir().join(format!(
            "attend-migrate-noop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&base).unwrap();
        let mgr = Groups::new(&base, "sess-test");
        assert!(migrate_legacy_open_group(&base, &mgr).is_none());
    }

    #[test]
    fn migrate_legacy_open_skips_yaml_rewrite_when_no_entry() {
        // PR #66 review S3: the migration must not rewrite
        // _groups.yaml when `open:` isn't present. A user who
        // hand-made `@open/` without an attend scene shouldn't
        // trigger a yaml churn that races peer sessions.
        let base = std::env::temp_dir().join(format!(
            "attend-migrate-noyaml-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(base.join("@open")).unwrap();
        fs::write(
            base.join("@open").join("a.signal"),
            "from|proj|/x|hi\n",
        )
        .unwrap();
        // Seed _groups.yaml with a *different* group so we can
        // observe whether the migration rewrites it. If it rewrites
        // without real work, the mtime will bump.
        let yaml_path = base.join("_groups.yaml");
        fs::write(
            &yaml_path,
            "infra:\n  pinned: false\n  members:\n    - sess-x\n",
        )
        .unwrap();
        let before = fs::metadata(&yaml_path).unwrap().modified().unwrap();

        let mgr = Groups::new(&base, "sess-test");
        let moved = migrate_legacy_open_group(&base, &mgr).unwrap();
        assert_eq!(moved, 1);
        assert!(base.join("_broadcast").join("a.signal").exists());
        assert!(!base.join("@open").exists());

        // The canary yaml was not rewritten — the mtime is
        // unchanged. (On fast filesystems the resolution may match,
        // but the actual file-content invariant is the reliable
        // signal, so check both.)
        let after = fs::metadata(&yaml_path).unwrap().modified().unwrap();
        let contents = fs::read_to_string(&yaml_path).unwrap();
        assert_eq!(before, after, "yaml must not be rewritten");
        assert!(contents.contains("infra:"));
        assert!(!contents.contains("open:"));
    }

    #[test]
    fn migrate_legacy_open_moves_signals_and_drops_dir() {
        let base = std::env::temp_dir().join(format!(
            "attend-migrate-move-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let open_dir = base.join("@open");
        fs::create_dir_all(&open_dir).unwrap();
        fs::write(open_dir.join("a.signal"), "from|proj|/x|hi\n").unwrap();
        fs::write(open_dir.join("b.signal"), "from|proj|/x|bye\n").unwrap();
        // A non-signal file — must be left alone (still in
        // @open/, until dissolve drops the dir).
        fs::write(open_dir.join("notes.txt"), "scratch").unwrap();

        // Pre-seed the `open` group in _groups.yaml so dissolve has
        // something to remove. Written directly because `join` now
        // rejects `"open"` as reserved — this simulates state left
        // by a pre-ADR-124 binary.
        fs::write(
            base.join("_groups.yaml"),
            "open:\n  pinned: false\n  members:\n    - sess-test\n",
        )
        .unwrap();
        let mgr = Groups::new(&base, "sess-test");

        let moved = migrate_legacy_open_group(&base, &mgr).unwrap();
        assert_eq!(moved, 2);

        let broadcast_dir = base.join("_broadcast");
        assert!(broadcast_dir.join("a.signal").exists());
        assert!(broadcast_dir.join("b.signal").exists());
        assert!(!open_dir.exists(), "@open/ should be gone after dissolve");
        // _groups.yaml should no longer mention `open`.
        let yaml = fs::read_to_string(base.join("_groups.yaml")).unwrap_or_default();
        assert!(!yaml.contains("open:"));
    }
}
