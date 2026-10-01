//! `ways uninstall`: the last step of the install lifecycle (ADR-198).
//!
//! Withdraws agent-ways from every target the way `ways config target remove`
//! does, stops the ways agent, removes the command links the installer put on
//! `PATH`, and deletes the app's own directories: the staged app and every
//! cache it has used. The operator's config (their ways, API keys, settings)
//! and state (events, probe data) are kept unless `--purge` is given. Without
//! `--yes` it prints the plan and changes nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;

use crate::config::Config;
use crate::paths;

/// What one uninstall would do.
#[derive(Debug, Default)]
struct Plan {
    targets: Vec<crate::config::Target>,
    links: Vec<PathBuf>,
    remove: Vec<PathBuf>,
    keep: Vec<PathBuf>,
}

fn user_bin_dir() -> PathBuf {
    std::env::var_os("XDG_BIN_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| crate::util::home_dir().join(".local/bin"))
}

/// A link the installer made: a symlink into the app's data root.
fn is_our_link(link: &Path, data_root: &Path) -> bool {
    std::fs::read_link(link).is_ok_and(|target| target.starts_with(data_root))
}

fn plan(purge: bool) -> Plan {
    let data_root = paths::data_root();
    let project = std::env::var("PWD").unwrap_or_else(|_| ".".to_string());
    // The recorded targets, plus ~/.claude: the installer projects into it
    // whatever the list says. Withdrawing where nothing of ours is is a no-op.
    let mut targets = Config::load(&project).targets();
    let home = crate::config::Target::new(paths::projection_root().to_string_lossy().to_string());
    if !targets.iter().any(|t| t.dir() == home.dir()) {
        targets.push(home);
    }
    let targets = targets.into_iter().filter(|t| t.dir().is_dir()).collect();
    // Every link into the app, whatever the installer named it.
    let links = std::fs::read_dir(user_bin_dir())
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|l| is_our_link(l, &data_root)).collect())
        .unwrap_or_default();
    let mut remove = vec![data_root];
    remove.extend(paths::cache_roots_all());
    let owned = [paths::config_root(), paths::state_root()];
    let (mut keep, mut purged) = (Vec::new(), Vec::new());
    for dir in owned {
        if purge { purged.push(dir) } else { keep.push(dir) }
    }
    remove.extend(purged);
    Plan {
        targets,
        links,
        remove: remove.into_iter().filter(|p| p.exists()).collect(),
        keep: keep.into_iter().filter(|p| p.exists()).collect(),
    }
}

fn print_plan(p: &Plan, purge: bool) {
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
    show("Remove command links:", &p.links);
    show("Delete:", &p.remove);
    if !purge {
        show("Keep (your ways, keys, settings, events; --purge deletes them):", &p.keep);
    }
}

pub fn run(yes: bool, purge: bool) -> Result<()> {
    let p = plan(purge);
    if p.targets.is_empty() && p.links.is_empty() && p.remove.is_empty() {
        println!("agent-ways is not installed: nothing to remove.");
        return Ok(());
    }
    print_plan(&p, purge);
    if !yes {
        println!("\nNothing changed. Run `ways uninstall --yes` to do this.");
        return Ok(());
    }

    // Withdraw as `target disable` does, without editing the config: a kept
    // config keeps its targets, so a later install activates them again.
    for target in &p.targets {
        let mut withdrawn = target.clone();
        withdrawn.enabled = false;
        if let Err(e) = crate::cmd::reconcile::run_for_targets(&[withdrawn], false, false, false) {
            eprintln!("withdrawing from {} failed: {e:#}; stopping before anything is deleted", target.dir().display());
            return Err(e);
        }
        // Directories withdrawal emptied and nothing else uses.
        for sub in ["hooks", "bin"] {
            let _ = std::fs::remove_dir(target.dir().join(sub));
        }
    }
    // The agent holds no files open that matter; stopping it keeps a running
    // process from outliving its binary.
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
            Err(e) => {
                failed += 1;
                eprintln!("could not delete {}: {e}", dir.display());
            }
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} path(s) could not be deleted; the rest of agent-ways is removed");
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

    #[test]
    fn only_symlinks_into_the_data_root_count_as_ours() {
        let base = std::env::temp_dir().join(format!("ways-uninstall-{}", std::process::id()));
        let data = base.join("data/agent-ways");
        let bin = base.join("bin");
        std::fs::create_dir_all(data.join("bin")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(data.join("bin/ways"), "").unwrap();
        std::fs::write(base.join("other"), "").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(data.join("bin/ways"), bin.join("ways")).unwrap();
            std::os::unix::fs::symlink(base.join("other"), bin.join("attend")).unwrap();
            assert!(is_our_link(&bin.join("ways"), &data));
            assert!(!is_our_link(&bin.join("attend"), &data), "a link elsewhere is the user's");
        }
        std::fs::write(bin.join("ways-mcp"), "").unwrap();
        assert!(!is_our_link(&bin.join("ways-mcp"), &data), "a real file is never ours to remove");
        let _ = std::fs::remove_dir_all(&base);
    }
}
