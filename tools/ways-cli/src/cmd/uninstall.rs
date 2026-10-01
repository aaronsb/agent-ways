//! `ways uninstall`: the last step of the install lifecycle (ADR-198).
//!
//! Withdraws agent-ways from every target and from `~/.claude`, stops the ways
//! agent, removes the command links that point into the app, and deletes the
//! app and every cache it has used. The operator's config (their ways, API
//! keys, settings) and state (events, probe data) are kept unless `--purge` is
//! given. Without `--yes` it prints the plan and changes nothing. A plan that
//! would delete anything but the app's own directories is refused.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};

use crate::config::{Config, Target};
use crate::paths;

/// The names the app's directories carry. Nothing else is ever deleted.
const APP_DIR_NAMES: &[&str] = &["agent-ways", "claude-ways"];

/// Where things live, resolved from the environment once.
#[derive(Debug, Clone)]
struct Roots {
    home: PathBuf,
    projection: PathBuf,
    data: PathBuf,
    caches: Vec<PathBuf>,
    config: PathBuf,
    state: PathBuf,
}

impl Roots {
    fn from_env() -> Roots {
        Roots {
            home: crate::util::home_dir(),
            projection: paths::projection_root(),
            data: paths::data_root(),
            caches: paths::cache_roots_all().to_vec(),
            config: paths::config_root(),
            state: paths::state_root(),
        }
    }
}

/// What one uninstall would do.
#[derive(Debug, Default)]
struct Plan {
    /// Withdrawn first. Empty when the app is already gone: withdrawal
    /// identifies our links by the app they point into.
    targets: Vec<Target>,
    links: Vec<PathBuf>,
    /// Deleted in order; the app itself comes last, so a failure part way
    /// leaves `ways` in place to run again.
    remove: Vec<PathBuf>,
    keep: Vec<PathBuf>,
}

/// The bin dirs the installer may have linked into: `install.sh` uses
/// `~/.local/bin`, the Makefile `$XDG_BIN_HOME` when set.
fn bin_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![home.join(".local/bin")];
    if let Some(x) = std::env::var_os("XDG_BIN_HOME").map(PathBuf::from).filter(|p| p.is_absolute()) {
        if !dirs.contains(&x) {
            dirs.push(x);
        }
    }
    dirs
}

/// Lexical normalization: `..` and `.` folded without touching the disk, so a
/// dangling link still resolves.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// A link the installer made: a symlink whose target, resolved against the
/// link's own directory, lies in the app's data root.
fn is_our_link(link: &Path, data_root: &Path) -> bool {
    let Ok(target) = std::fs::read_link(link) else { return false };
    let target = if target.is_absolute() { target } else { link.parent().unwrap_or(Path::new("/")).join(target) };
    normalize(&target).starts_with(data_root)
}

/// Decides what goes where. Pure over its inputs, and refuses any plan that
/// could delete what the operator owns.
fn plan_from(roots: &Roots, recorded: Vec<Target>, links: Vec<PathBuf>, purge: bool) -> Result<Plan> {
    for p in [&roots.home, &roots.projection, &roots.data, &roots.config, &roots.state] {
        if !p.is_absolute() {
            bail!("{} is not an absolute path (is HOME set?); refusing to uninstall", p.display());
        }
    }
    let mut targets = Vec::new();
    if roots.data.exists() {
        // The recorded targets, plus ~/.claude: the installer projects into it
        // whatever the list says.
        targets = recorded;
        if !targets.iter().any(|t| t.dir() == roots.projection) {
            targets.push(Target::new(roots.projection.to_string_lossy().to_string()));
        }
        targets.retain(|t| t.dir().is_dir());
    }

    let owned = vec![roots.config.clone(), roots.state.clone()];
    let (keep, mut remove) = if purge { (Vec::new(), owned.clone()) } else { (owned, Vec::new()) };
    for c in &roots.caches {
        if !remove.contains(c) {
            remove.push(c.clone());
        }
    }
    if !remove.contains(&roots.data) {
        remove.push(roots.data.clone());
    }

    // Every deletion is one of the app's own directories, and none is, holds
    // or sits inside anything kept, $HOME or ~/.claude.
    for r in &remove {
        let named = r.file_name().and_then(|n| n.to_str()).is_some_and(|n| APP_DIR_NAMES.contains(&n));
        if !r.is_absolute() || !named {
            bail!("refusing to delete {}: not one of the app's own directories", r.display());
        }
        for k in &keep {
            if r.starts_with(k) || k.starts_with(r) {
                bail!(
                    "refusing to delete {}: it overlaps {}, which is kept (XDG directories set to the same place?)",
                    r.display(),
                    k.display()
                );
            }
        }
        for k in [&roots.home, &roots.projection] {
            if k.starts_with(r) {
                bail!("refusing to delete {}: it holds {}", r.display(), k.display());
            }
        }
    }

    Ok(Plan {
        targets,
        links,
        remove: remove.into_iter().filter(|p| p.exists()).collect(),
        keep: keep.into_iter().filter(|p| p.exists()).collect(),
    })
}

fn print_plan(p: &Plan, app_present: bool, purge: bool) {
    let show = |label: &str, items: &[PathBuf]| {
        if !items.is_empty() {
            println!("{label}");
            for i in items {
                println!("  {}", i.display());
            }
        }
    };
    let targets: Vec<PathBuf> = p.targets.iter().map(|t| t.dir()).collect();
    show("Withdraw from (links, hooks, permissions and the MCP entry ways wrote):", &targets);
    if !app_present {
        println!("The app is already gone, so nothing can be withdrawn; reinstall and uninstall again to clear a projection left behind.");
    }
    show("Remove command links:", &p.links);
    show("Delete:", &p.remove);
    if !purge {
        show("Keep (your ways, keys, settings, events; --purge deletes them):", &p.keep);
    }
}

pub fn run(yes: bool, purge: bool) -> Result<()> {
    let roots = Roots::from_env();
    let project = std::env::var("PWD").unwrap_or_else(|_| ".".to_string());
    let links: Vec<PathBuf> = bin_dirs(&roots.home)
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()))
        .filter(|l| is_our_link(l, &roots.data))
        .collect();
    let p = plan_from(&roots, Config::load(&project).targets(), links, purge)?;
    if p.targets.is_empty() && p.links.is_empty() && p.remove.is_empty() {
        println!("agent-ways is not installed: nothing to remove.");
        return Ok(());
    }
    print_plan(&p, roots.data.exists(), purge);
    if !yes {
        println!("\nNothing changed. Run `ways uninstall --yes` to do this.");
        return Ok(());
    }

    // Withdraw as `target disable` does, without editing the config: a kept
    // config keeps its targets, so a later install activates them again. A
    // failure stops before anything is deleted; targets withdrawn before it
    // stay withdrawn, and a reinstall projects them again.
    for target in &p.targets {
        let mut withdrawn = target.clone();
        withdrawn.enabled = false;
        if let Err(e) = crate::cmd::reconcile::run_for_targets(&[withdrawn], false, false, false) {
            eprintln!("withdrawing from {} failed: {e:#}; nothing was deleted", target.dir().display());
            return Err(e);
        }
        // Directories withdrawal emptied and nothing else uses.
        for sub in ["hooks", "bin"] {
            let _ = std::fs::remove_dir(target.dir().join(sub));
        }
    }
    // Stopping the agent keeps a running process from outliving its binary.
    let _ = ways_agent_core::client::call(ways_agent_core::protocol::Request::Shutdown, Duration::from_secs(2), false);
    for link in &p.links {
        if std::fs::remove_file(link).is_ok() {
            println!("removed {}", link.display());
        }
    }
    let mut failed = 0;
    for dir in &p.remove {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => println!("deleted {}", dir.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                failed += 1;
                eprintln!("could not delete {}: {e}", dir.display());
            }
        }
    }
    if failed > 0 {
        bail!("{failed} path(s) could not be deleted; fix the cause and run `ways uninstall --yes` again");
    }
    println!("\nagent-ways is uninstalled. Restart Claude Code so running sessions drop the hooks.");
    if !purge && !p.keep.is_empty() {
        println!("Your config and state are kept; a later install picks them up.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("ways-uninstall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn roots(base: &Path, data: &str, config: &str, state: &str) -> Roots {
        let r = Roots {
            home: base.to_path_buf(),
            projection: base.join(".claude"),
            data: base.join(data).join("agent-ways"),
            caches: vec![base.join(".cache/agent-ways"), base.join(".cache/claude-ways")],
            config: base.join(config).join("agent-ways"),
            state: base.join(state).join("agent-ways"),
        };
        for d in [&r.projection, &r.data, &r.caches[0], &r.config, &r.state] {
            std::fs::create_dir_all(d).unwrap();
        }
        r
    }

    #[test]
    fn the_default_keeps_config_and_state_and_purge_deletes_them() {
        let base = sandbox("split");
        let r = roots(&base, ".local/share", ".config", ".local/state");
        let p = plan_from(&r, vec![], vec![], false).unwrap();
        assert_eq!(p.keep, vec![r.config.clone(), r.state.clone()]);
        assert_eq!(p.remove, vec![r.caches[0].clone(), r.data.clone()], "the app goes last");
        assert_eq!(p.targets.len(), 1, "~/.claude is always withdrawn");
        let p = plan_from(&r, vec![], vec![], true).unwrap();
        assert!(p.keep.is_empty());
        assert_eq!(p.remove, vec![r.config.clone(), r.state.clone(), r.caches[0].clone(), r.data.clone()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn overlapping_xdg_dirs_are_refused_rather_than_deleting_what_is_kept() {
        let base = sandbox("overlap");
        let r = roots(&base, "share", "share", "share");
        let err = plan_from(&r, vec![], vec![], false).unwrap_err().to_string();
        assert!(err.contains("overlaps"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_relative_root_is_refused() {
        let base = sandbox("relative");
        let mut r = roots(&base, ".local/share", ".config", ".local/state");
        r.data = PathBuf::from(".local/share/agent-ways");
        assert!(plan_from(&r, vec![], vec![], false).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn with_the_app_gone_nothing_is_withdrawn_and_the_caches_still_go() {
        let base = sandbox("gone");
        let r = roots(&base, ".local/share", ".config", ".local/state");
        std::fs::remove_dir_all(&r.data).unwrap();
        let p = plan_from(&r, vec![], vec![], false).unwrap();
        assert!(p.targets.is_empty());
        assert_eq!(p.remove, vec![r.caches[0].clone()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn only_links_into_the_data_root_count_as_ours_relative_or_not() {
        let base = sandbox("links");
        let data = base.join("share/agent-ways");
        let bin = base.join("bin");
        std::fs::create_dir_all(data.join("bin")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(base.join("other"), "").unwrap();
        std::os::unix::fs::symlink(data.join("bin/ways"), bin.join("ways")).unwrap();
        std::os::unix::fs::symlink("../share/agent-ways/bin/attend", bin.join("attend")).unwrap();
        std::os::unix::fs::symlink(base.join("other"), bin.join("mine")).unwrap();
        std::fs::write(bin.join("real"), "").unwrap();
        assert!(is_our_link(&bin.join("ways"), &data));
        assert!(is_our_link(&bin.join("attend"), &data), "a relative link into the app is ours");
        assert!(!is_our_link(&bin.join("mine"), &data), "a link elsewhere is the user's");
        assert!(!is_our_link(&bin.join("real"), &data), "a real file is never ours to remove");
        let _ = std::fs::remove_dir_all(&base);
    }
}
