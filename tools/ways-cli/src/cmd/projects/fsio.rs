//! File primitives for `ways projects`' writers: atomic replacement that
//! keeps mtime, fsync before the swap, timestamped backups, and a trash
//! directory in place of deletion.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// A timestamped sibling copy, `<name>.bak-relocate-<UTC stamp>[.n]`.
pub(super) fn backup_file(path: &Path) -> std::io::Result<PathBuf> {
    let stamp = utc_stamp(agent_fmt::when::now_secs());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut dest = path.with_file_name(format!("{name}.bak-relocate-{stamp}"));
    let mut n = 0;
    // Same-second relocations must not clobber each other.
    while dest.exists() {
        n += 1;
        dest = path.with_file_name(format!("{name}.bak-relocate-{stamp}.{n}"));
    }
    fs::copy(path, &dest)?;
    if let Ok(meta) = fs::metadata(path) {
        if let Ok(mtime) = meta.modified() {
            let _ = fs::File::options().write(true).open(&dest).and_then(|f| f.set_modified(mtime));
        }
    }
    Ok(dest)
}

pub(super) fn tmp_sibling(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.relocate-tmp"))
}

/// Swap `tmp` into `path`, keeping `path`'s mtime and permissions: project
/// listings sort by mtime, and relocating is not activity.
pub(super) fn replace_atomic(tmp: &Path, path: &Path) -> std::io::Result<()> {
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(tmp, meta.permissions())?;
        if let Ok(mtime) = meta.modified() {
            fs::File::options().write(true).open(tmp)?.set_modified(mtime)?;
        }
    }
    fs::rename(tmp, path)
}

/// Serialize to a temp sibling, fsync, then swap. Never truncates in place.
pub(super) fn write_json_atomic(path: &Path, data: &Value, pretty: bool) -> std::io::Result<()> {
    let tmp = tmp_sibling(path);
    let result = (|| {
        let text = if pretty { serde_json::to_string_pretty(data) } else { serde_json::to_string(data) }
            .map_err(std::io::Error::other)?;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        replace_atomic(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub(super) fn stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

pub(super) fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// A fresh `projects/.trash-<UTC stamp>[.n]/` to move removed entries into.
/// Dot-named, so project listings skip it.
pub(super) fn trash_dir(projects: &Path) -> PathBuf {
    let stamp = utc_stamp(agent_fmt::when::now_secs());
    let mut dir = projects.join(format!(".trash-{stamp}"));
    let mut n = 0;
    while dir.exists() {
        n += 1;
        dir = projects.join(format!(".trash-{stamp}.{n}"));
    }
    dir
}

/// Move `path` into `trash` under `rel` (a relative path), creating parents.
pub(super) fn move_to_trash(path: &Path, trash: &Path, rel: &Path) -> std::io::Result<PathBuf> {
    let dest = trash.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(path, &dest)?;
    Ok(dest)
}

/// `YYYYMMDD-HHMMSS` in UTC.
pub(super) fn utc_stamp(secs: u64) -> String {
    let (y, m, d) = agent_fmt::when::civil_from_days((secs / 86_400) as i64);
    let t = secs % 86_400;
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", t / 3600, t / 60 % 60, t % 60)
}
