//! `ways target` (ADR-184, ADR-185, ADR-507): the projection targets, the
//! Claude Code config directories agent-ways is active in.
//!
//! Output follows ADR-185: tables for people, one JSON document under
//! `--json`, a debug rendering never.

use crate::cmd::reconcile::{self, Plan};
use crate::config::{Config, Target};
use crate::paths;
use agent_fmt::{Align, Table};
use anyhow::{bail, Result};
use std::path::PathBuf;

/// Exit code for a plan that would refuse or remove something the operator
/// owns (ADR-185 item 5): distinguishable from a failure.
pub const EXIT_BLOCKED: i32 = 3;

/// The converged state of one target, read from its directory.
pub fn state(t: &Target) -> String {
    let plan = match reconcile::plan_target(&t.dir()) {
        Ok(p) => p,
        Err(e) => return format!("unreadable ({e})"),
    };
    let linked = plan.roots.iter().filter(|r| r.state == "linked").count();
    let refused = plan.roots.iter().filter(|r| r.state == "refused").count();
    let total = plan.roots.len();
    if t.enabled {
        if refused > 0 {
            format!("refused ({refused} real paths)")
        } else if linked == total {
            "active".to_string()
        } else if linked == 0 {
            "pending".to_string()
        } else {
            format!("partial ({linked}/{total} linked)")
        }
    } else if linked > 0 {
        format!("stale ({linked} links remain)")
    } else {
        "withdrawn".to_string()
    }
}

pub fn list(json: bool) -> Result<()> {
    let cfg = Config::load(&crate::util::project_dir());
    let list = cfg.targets();
    if json {
        let rows: Vec<serde_json::Value> = list
            .iter()
            .map(|t| {
                serde_json::json!({
                    "path": t.path,
                    "dir": t.dir(),
                    "enabled": t.enabled,
                    "observe": t.observes(),
                    "config": t.config_path(),
                    "config_present": t.config_path().is_file(),
                    "state": state(t),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "explicit": cfg.targets_explicit(),
                "targets": rows,
            }))?
        );
        return Ok(());
    }
    if list.is_empty() {
        println!("no targets: agent-ways is installed and inactive.");
        println!("  ways target plan <dir>   # what activating a Claude Code config dir would do");
        println!("  ways target add <dir>    # activate it (default dir: ~/.claude)");
        return Ok(());
    }
    let mut t = Table::new(&["Target", "Enabled", "Observe", "State", "Config"]);
    t.align(0, Align::Left);
    t.no_auto_fit();
    for target in &list {
        let cfg_path = target.config_path();
        t.add_owned(vec![
            target.path.clone(),
            target.enabled.to_string(),
            target.observes().to_string(),
            state(target),
            if cfg_path.is_file() { cfg_path.display().to_string() } else { "(user config)".to_string() },
        ]);
    }
    t.print();
    if !cfg.targets_explicit() {
        eprintln!("implicit: no `targets` key in {}; the default config dir is the one target", paths::user_config().display());
    }
    Ok(())
}

/// Resolve an operator-supplied directory for activation: it must exist and
/// be a directory. The stored form keeps `~` when the operator typed it and is
/// the canonical absolute path otherwise, so a relative path is never recorded.
fn resolve_dir(dir: &str) -> Result<(String, PathBuf)> {
    let expanded = Target::new(dir).dir();
    if !expanded.is_dir() {
        bail!(
            "{} is not a directory. A target is a Claude Code config directory \
             (the default is ~/.claude; a relocated one is what CLAUDE_CONFIG_DIR points at).",
            expanded.display()
        );
    }
    let canonical = std::fs::canonicalize(&expanded).unwrap_or(expanded);
    let stored = if dir.starts_with('~') { dir.to_string() } else { canonical.to_string_lossy().to_string() };
    Ok((stored, canonical))
}

/// Match a recorded target to an operator-supplied directory, whether or not
/// the directory still exists: canonical paths when both resolve, the
/// expanded path otherwise. Disable and remove must work on a deleted dir.
fn find_target(list: &[Target], dir: &str) -> Option<usize> {
    let wanted = Target::new(dir).dir();
    let wanted_c = std::fs::canonicalize(&wanted).unwrap_or(wanted.clone());
    list.iter().position(|t| {
        let d = t.dir();
        let dc = std::fs::canonicalize(&d).unwrap_or(d.clone());
        dc == wanted_c || d == wanted || t.path == dir
    })
}

pub fn print_plan(plan: &Plan, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(plan)?);
        return Ok(());
    }
    println!("plan for {}", plan.dest);
    let mut t = Table::new(&["Root", "Action", "Detail"]);
    t.align(0, Align::Left);
    t.no_auto_fit();
    for r in &plan.roots {
        t.add_owned(vec![r.rel.clone(), r.state.clone(), r.detail.clone()]);
    }
    t.print();
    match &plan.settings {
        None => println!("settings.json: the source ships none; nothing merged"),
        Some(s) if s.unchanged => println!("settings.json: already merged, nothing changes"),
        Some(s) => {
            println!("settings.json:");
            println!("  kept      {} hook entr{} of yours", s.user_hooks_kept, if s.user_hooks_kept == 1 { "y" } else { "ies" });
            for h in &s.hooks_added {
                println!("  add       {}: {}", h.event, h.command);
            }
            for h in &s.hooks_refreshed {
                println!("  refresh   {}: {}  (ours, from a prior version)", h.event, h.command);
            }
            for h in &s.hooks_replaced {
                println!("  replace   {}: {}  (reads as an agent-ways hook)", h.event, h.command);
            }
            for h in &s.hooks_removed {
                println!("  REMOVE    {}: {}", h.event, h.command);
            }
            if !s.perms_added.is_empty() {
                println!("  allow     +{} entries", s.perms_added.len());
            }
            if !s.deny_added.is_empty() {
                println!("  deny      +{} entries (secret paths, ADR-152)", s.deny_added.len());
            }
        }
    }
    if plan.blocked {
        println!();
        println!("blocked: a real path sits at a projection root, or an entry of yours would be removed.");
        println!("  --force renames each real path to <name>.ways-backup-<seconds> and proceeds;");
        println!("  move a hook out of .claude/hooks/ first if it is listed under REMOVE or replace.");
    }
    Ok(())
}

pub fn plan(dir: &str, json: bool) -> Result<()> {
    let (_, canonical) = resolve_dir(dir)?;
    let plan = reconcile::plan_target(&canonical)?;
    print_plan(&plan, json)
}

pub fn add(dir: &str, force: bool, dry_run: bool, json: bool) -> Result<()> {
    let (stored, canonical) = resolve_dir(dir)?;
    let plan = reconcile::plan_target(&canonical)?;
    print_plan(&plan, json)?;
    if dry_run {
        return Ok(());
    }
    if plan.blocked && !force {
        std::process::exit(EXIT_BLOCKED);
    }
    let _ = canonical;
    let implicit = Config::load(&crate::util::project_dir()).targets();
    let mut chosen: Option<Target> = None;
    let (path, _) = Config::edit_user_targets(|current| {
        // A file with no key starts from the implicit list, so the default
        // target is recorded alongside the new one.
        let mut list = current.unwrap_or(implicit);
        let t = match find_target(&list, dir) {
            Some(i) => {
                list[i].enabled = true;
                list[i].clone()
            }
            None => {
                let t = Target::new(stored.clone());
                list.push(t.clone());
                t
            }
        };
        chosen = Some(t);
        Some(list)
    })?;
    let target = chosen.expect("edit closure always runs");
    // Under --json the plan document is the whole of stdout; the apply logs
    // to stderr.
    if let Err(e) = reconcile::run_for_targets(&[target], false, json, force) {
        eprintln!(
            "target recorded in {} but the reconcile failed; `ways reconcile` retries it, \
             `ways target remove` forgets it",
            path.display()
        );
        return Err(e);
    }
    eprintln!("target recorded in {}", path.display());
    Ok(())
}

fn set_enabled(dir: &str, enabled: bool) -> Result<()> {
    let implicit = Config::load(&crate::util::project_dir()).targets();
    let mut chosen: Option<Target> = None;
    let mut missing = false;
    Config::edit_user_targets(|current| {
        let mut list = current.unwrap_or(implicit);
        let Some(i) = find_target(&list, dir) else {
            missing = true;
            return None;
        };
        list[i].enabled = enabled;
        chosen = Some(list[i].clone());
        Some(list)
    })?;
    if missing {
        bail!("{dir} is not a target; `ways target list` lists them, `ways target add` adds one");
    }
    let target = chosen.expect("set when found");
    if enabled && !target.dir().is_dir() {
        bail!("{} does not exist; a target must be a directory to enable", target.dir().display());
    }
    reconcile::run_for_targets(&[target], false, false, false)
}

pub fn enable(dir: &str) -> Result<()> {
    set_enabled(dir, true)
}

pub fn disable(dir: &str) -> Result<()> {
    set_enabled(dir, false)
}

pub fn remove(dir: &str) -> Result<()> {
    let implicit = Config::load(&crate::util::project_dir()).targets();
    let mut removed: Option<Target> = None;
    // Withdraw first, then forget: a removed target is a withdrawn one. The
    // record is dropped under the lock only after the withdrawal succeeded.
    let list = Config::edit_user_targets(|current| {
        let list = current.unwrap_or(implicit);
        removed = find_target(&list, dir).map(|i| list[i].clone());
        None
    })?
    .1;
    let Some(mut target) = removed else {
        bail!("{dir} is not a target");
    };
    let _ = list;
    target.enabled = false;
    if target.dir().is_dir() {
        reconcile::run_for_targets(&[target.clone()], false, false, false)?;
    }
    let (path, _) = Config::edit_user_targets(|current| {
        let mut list = current.unwrap_or_default();
        list.retain(|t| t.path != target.path);
        Some(list)
    })?;
    eprintln!("target removed from {}", path.display());
    Ok(())
}
