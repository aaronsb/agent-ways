//! `attend cleanup` — prune stale signal files and empty project dirs
//! from the signals base. The core `run_cleanup` routine is also used
//! by the in-loop auto-sweep in `cmd::run`, so `run_cleanup` and its
//! `CleanupStats` return value are `pub(crate)`.

use std::path::Path;
use std::time::Duration;

use crate::config;
use crate::sensors::Focus;
use crate::util::signals_base;

/// Parse a duration like "30s", "5m", "1h". Bare digits are treated as seconds.
fn parse_duration_arg(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = match s.chars().last()? {
        c if c.is_ascii_digit() => (s, "s"),
        _ => s.split_at(s.len() - 1),
    };
    let n: u64 = num.parse().ok()?;
    let mult: u64 = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    Some(Duration::from_secs(n * mult))
}

/// Statistics from a cleanup sweep.
#[derive(Default, Debug)]
pub(crate) struct CleanupStats {
    pub(crate) examined: u64,
    pub(crate) removed: u64,
    pub(crate) bytes: u64,
    pub(crate) dirs_removed: u64,
}

/// Core cleanup routine, shared by `attend cleanup` and the in-loop auto-sweep.
///
/// Two passes over the signals base:
///   1. Remove stale `*.signal` files older than `older_than` (or all if `nuke_all`).
///   2. Remove now-empty encoded-cwd project subdirs — the shells left behind
///      after projects go dormant. Never removes `_broadcast`, `@groups`, or
///      any dir containing non-signal files (e.g., `_groups.yaml`).
///
/// On `dry_run`, emits a line per candidate to stdout instead of deleting.
pub(crate) fn run_cleanup(
    base: &Path,
    older_than: Duration,
    dry_run: bool,
    nuke_all: bool,
) -> CleanupStats {
    let mut stats = CleanupStats::default();
    if !base.is_dir() {
        return stats;
    }

    let now = std::time::SystemTime::now();
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return stats,
    };

    // Pass 1: prune stale signal files.
    for sub in entries.flatten() {
        let subpath = sub.path();
        if !subpath.is_dir() {
            continue;
        }
        let files = match std::fs::read_dir(&subpath) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let path = f.path();
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.ends_with(".signal") {
                continue;
            }
            stats.examined += 1;

            let meta = match f.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let age = meta
                .modified()
                .ok()
                .and_then(|mt| now.duration_since(mt).ok())
                .unwrap_or(Duration::ZERO);

            if !nuke_all && age < older_than {
                continue;
            }

            let size = meta.len();
            if dry_run {
                println!(
                    "would remove {} ({}s old, {} bytes)",
                    path.display(),
                    age.as_secs(),
                    size
                );
            } else if std::fs::remove_file(&path).is_ok() {
                stats.removed += 1;
                stats.bytes += size;
            }
        }
    }

    // Pass 2: remove empty encoded-cwd project subdirs left as shells.
    // A project subdir is a non-reserved name (not _broadcast, not @group,
    // not _anything) that now contains nothing. Focus-group dirs self-clean
    // on leave/dissolve already; we don't touch those here.
    if let Ok(entries) = std::fs::read_dir(base) {
        for sub in entries.flatten() {
            let subpath = sub.path();
            if !subpath.is_dir() {
                continue;
            }
            let name = match subpath.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            // Reserved names we never touch.
            if name.starts_with('_') || name.starts_with('@') {
                continue;
            }
            // Dir is a candidate only if fully empty now.
            let empty = std::fs::read_dir(&subpath)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if !empty {
                continue;
            }
            if dry_run {
                println!("would remove empty project dir {}", subpath.display());
            } else if std::fs::remove_dir(&subpath).is_ok() {
                stats.dirs_removed += 1;
            }
        }
    }

    stats
}

pub(crate) fn cmd_cleanup(older_than_arg: Option<String>, dry_run: bool, nuke_all: bool) {
    // Default to the config's retention so the manual command's semantics
    // match the auto-sweep by default. `--older-than` overrides.
    let focus = Focus::default_focus();
    let cfg = config::Config::load(&focus.working_dir);
    let older_than = match older_than_arg {
        Some(s) => match parse_duration_arg(&s) {
            Some(d) => d,
            None => {
                eprintln!("attend cleanup: invalid duration '{s}' — try 5m, 1h, 30s");
                std::process::exit(2);
            }
        },
        None => cfg.cleanup.retention,
    };

    let base = signals_base();
    if !base.is_dir() {
        println!("no signals base at {} — nothing to clean", base.display());
        return;
    }

    let stats = run_cleanup(&base, older_than, dry_run, nuke_all);

    if dry_run {
        println!("\ndry run: examined {} signal file(s)", stats.examined);
    } else {
        println!(
            "cleaned up {} signal file(s), freed {} bytes (examined {}); removed {} empty project dir(s)",
            stats.removed, stats.bytes, stats.examined, stats.dirs_removed,
        );
    }
}
