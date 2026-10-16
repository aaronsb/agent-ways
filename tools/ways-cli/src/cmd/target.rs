//! `ways manage target` (ADR-184, ADR-185, ADR-507): the projection targets, the
//! Claude Code config directories agent-ways is active in.
//!
//! Output follows ADR-185: tables for people, one JSON document under
//! `--json`, a debug rendering never.

use crate::cmd::lifecycle::{self, Mark, Mode, Report};
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
        println!("  ways manage target plan <dir>   # what activating a Claude Code config dir would do");
        println!("  ways manage target add <dir> --commit   # activate it (default dir: ~/.claude)");
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

/// One target's plan in the lifecycle report shape (ADR-511): a section of
/// projection roots, each marked by what activation does to it, then the
/// settings.json merge, then what blocks it. `parse_plan` in the settings
/// screens reads this text back, so a change here changes it there too.
pub fn plan_sections(r: &mut Report, plan: &Plan) {
    r.section("Roots");
    let width = plan.roots.iter().map(|x| x.rel.chars().count()).max().unwrap_or(0).max(28);
    for root in &plan.roots {
        let mark = match root.state.as_str() {
            "linked" => Mark::Same,
            "link" => Mark::Add,
            "relink" => Mark::Up,
            "refused" => Mark::Warn,
            _ => Mark::Info,
        };
        r.item(mark, format!("{:<width$}  {:<8}{}", root.rel, root.state, root.detail));
    }
    settings_section(r, "settings.json", &plan.settings);
    if plan.blocked {
        r.section("Blocked");
        r.item(Mark::Warn, "blocked: a real path sits at a projection root, or an entry of yours would be removed.");
        r.muted(4, "--force renames each real path to <name>.ways-backup-<seconds> and proceeds;");
        r.muted(4, "move a hook out of .claude/hooks/ first if it is listed under REMOVE or replace.");
    }
}

/// The settings.json merge a plan computed, as a section named `heading`:
/// what of the operator's is kept, and each hook or permission entry the
/// merge adds, refreshes, replaces or removes. `ways manage repair`'s
/// what-if shows the same section for each target it converges.
pub fn settings_section(r: &mut Report, heading: &str, settings: &Option<reconcile::SettingsPlan>) {
    r.section(heading);
    match settings {
        None => {
            r.item(Mark::Info, "the source ships none; nothing merged");
        }
        Some(s) if s.unchanged => {
            r.item(Mark::Same, "already merged, nothing changes");
        }
        Some(s) => {
            r.item(Mark::Same, format!("kept      {} hook entr{} of yours", s.user_hooks_kept, if s.user_hooks_kept == 1 { "y" } else { "ies" }));
            for h in &s.hooks_added {
                r.item(Mark::Add, format!("add       {}: {}", h.event, h.command));
            }
            for h in &s.hooks_refreshed {
                r.item(Mark::Up, format!("refresh   {}: {}  (ours, from a prior version)", h.event, h.command));
            }
            for h in &s.hooks_replaced {
                r.item(Mark::Up, format!("replace   {}: {}  (reads as an agent-ways hook)", h.event, h.command));
            }
            for h in &s.hooks_removed {
                r.item(Mark::Remove, format!("REMOVE    {}: {}", h.event, h.command));
            }
            if !s.perms_added.is_empty() {
                r.item(Mark::Add, format!("allow     +{} entries", s.perms_added.len()));
            }
            if !s.deny_added.is_empty() {
                r.item(Mark::Add, format!("deny      +{} entries (secret paths, ADR-152)", s.deny_added.len()));
            }
        }
    }
}

/// Print a plan under `title`, or the plan document under `--json`.
pub fn print_plan(title: String, plan: &Plan, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(plan)?);
        return Ok(());
    }
    let mut r = Report::new(title);
    plan_sections(&mut r, plan);
    r.print();
    Ok(())
}

pub fn plan(dir: &str, json: bool) -> Result<()> {
    let (_, canonical) = resolve_dir(dir)?;
    let plan = reconcile::plan_target(&canonical)?;
    print_plan(format!("ways manage target plan: {}", plan.dest), &plan, json)
}

/// The line that ends a target verb's what-if (ADR-511). Under `--json` the
/// plan document is the whole of stdout, so it goes to stderr.
fn whatif_footer(command: &str, json: bool) {
    let line = lifecycle::commit_hint(command);
    if json {
        eprintln!("{line}");
    } else {
        let mut r = Report::new("");
        r.footer(Mark::Info, line);
        r.print();
    }
}

pub fn add(dir: &str, force: bool, mode: Mode, json: bool) -> Result<()> {
    let (stored, canonical) = resolve_dir(dir)?;
    let plan = reconcile::plan_target(&canonical)?;
    let title = match mode {
        Mode::WhatIf => format!("ways manage target add: what-if for {}", plan.dest),
        Mode::Commit => format!("ways manage target add: {}", plan.dest),
    };
    print_plan(title, &plan, json)?;
    if mode == Mode::WhatIf {
        let force = if force { " --force" } else { "" };
        whatif_footer(&format!("ways manage target add {dir}{force}"), json);
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
            "target recorded in {} but the reconcile failed; `ways manage repair --commit` retries it, \
             `ways manage target remove <dir> --commit` forgets it",
            path.display()
        );
        return Err(e);
    }
    eprintln!("target recorded in {}", path.display());
    Ok(())
}

/// The recorded target `dir` names, read without a write.
fn recorded(dir: &str) -> Result<Target> {
    let list = Config::load(&crate::util::project_dir()).targets();
    match find_target(&list, dir) {
        Some(i) => Ok(list[i].clone()),
        None => bail!("{dir} is not a target; `ways manage target list` lists them, `ways manage target add <dir> --commit` adds one"),
    }
}

/// What enable, disable or remove would do, from a dry run of the
/// reconcile they run (ADR-511).
fn whatif(verb: &str, dir: &str, mut target: Target, enabled: bool, record: &str) -> Result<()> {
    println!("ways manage target {verb}: what-if");
    println!("{record}");
    target.enabled = enabled;
    if target.dir().is_dir() {
        reconcile::run_for_targets(&[target], true, false, false)?;
    }
    whatif_footer(&format!("ways manage target {verb} {dir}"), false);
    Ok(())
}

fn set_enabled(dir: &str, enabled: bool, mode: Mode) -> Result<()> {
    if mode == Mode::WhatIf {
        let target = recorded(dir)?;
        if enabled && !target.dir().is_dir() {
            bail!("{} does not exist; a target must be a directory to enable", target.dir().display());
        }
        let (verb, record) = if enabled {
            ("enable", format!("would record {} as enabled and reconcile into it:", target.dir().display()))
        } else {
            ("disable", format!("would record {} as disabled and withdraw from it:", target.dir().display()))
        };
        return whatif(verb, dir, target, enabled, &record);
    }
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
        bail!("{dir} is not a target; `ways manage target list` lists them, `ways manage target add <dir> --commit` adds one");
    }
    let target = chosen.expect("set when found");
    if enabled && !target.dir().is_dir() {
        bail!("{} does not exist; a target must be a directory to enable", target.dir().display());
    }
    reconcile::run_for_targets(&[target], false, false, false)
}

pub fn enable(dir: &str, mode: Mode) -> Result<()> {
    set_enabled(dir, true, mode)
}

pub fn disable(dir: &str, mode: Mode) -> Result<()> {
    set_enabled(dir, false, mode)
}

pub fn remove(dir: &str, mode: Mode) -> Result<()> {
    if mode == Mode::WhatIf {
        let target = recorded(dir)?;
        let record = format!("would withdraw from {} and drop it from the targets list:", target.dir().display());
        return whatif("remove", dir, target, false, &record);
    }
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
