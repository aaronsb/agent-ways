//! `ways config` (ADR-184, ADR-185): the resolved configuration, and the
//! projection targets that decide where agent-ways is active.
//!
//! Output follows ADR-185: tables for people, one JSON document under
//! `--json`, a debug rendering never. `show --json` is the stored file;
//! `show --json --effective` is the resolved state with defaults applied.

use crate::cmd::reconcile::{self, Plan};
use crate::config::{Config, Target};
use crate::paths;
use agent_fmt::{Align, Table};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

/// Exit code for a plan that would refuse or remove something the operator
/// owns (ADR-185 item 5): distinguishable from a failure.
pub const EXIT_BLOCKED: i32 = 3;

fn project_dir() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()))
}

/// The stored user config as JSON, or an empty object when the file is absent.
fn stored_json() -> Result<serde_json::Value> {
    let path = paths::user_config();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    serde_yaml::from_str::<serde_json::Value>(&text).with_context(|| format!("parsing {}", path.display()))
}

/// The resolved config as JSON. Built by hand because `Config` is assembled
/// from layers and carries no serde derive.
fn effective_json(cfg: &Config) -> serde_json::Value {
    serde_json::json!({
        "language": cfg.language,
        "default_scope": cfg.default_scope,
        "enabled": cfg.enabled,
        "disabled_domains": cfg.disabled_domains,
        "disabled_ways": cfg.disabled_ways(),
        "parent_threshold_multiplier": cfg.parent_threshold_multiplier,
        "parent_boost_floor": cfg.parent_boost_floor,
        "semantic_fire_probability": cfg.semantic_fire_probability,
        "keyword_floor_probability": cfg.keyword_floor_probability,
        "near_miss_margin": cfg.near_miss_margin,
        "refire_presets": cfg.refire_presets,
        "secret_path_deny": cfg.secret_path_deny,
        "targets": cfg.targets(),
        "targets_explicit": cfg.targets_explicit(),
        "target_config": cfg.target_config,
        "current_config_dir": paths::current_config_dir(),
    })
}

pub fn show(json: bool, effective: bool) -> Result<()> {
    // Loads fresh from disk rather than config::global(): a diagnostic verb
    // reflects the files as they are now.
    let cfg = Config::load(&project_dir());
    if json {
        let doc = if effective { effective_json(&cfg) } else { stored_json()? };
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(());
    }
    let mut t = Table::new(&["Setting", "Value"]);
    t.align(0, Align::Left);
    t.no_auto_fit();
    let mut presets: Vec<(&String, &f64)> = cfg.refire_presets.iter().collect();
    presets.sort_by(|a, b| a.0.cmp(b.0));
    let rows: Vec<(String, String)> = vec![
        ("language".into(), cfg.language.clone()),
        ("default_scope".into(), cfg.default_scope.clone()),
        ("enabled".into(), cfg.enabled.to_string()),
        ("disabled_domains".into(), list_or_none(&cfg.disabled_domains)),
        ("disabled_ways".into(), list_or_none(cfg.disabled_ways())),
        ("parent_threshold_multiplier".into(), cfg.parent_threshold_multiplier.to_string()),
        ("parent_boost_floor".into(), cfg.parent_boost_floor.to_string()),
        ("semantic_fire_probability".into(), cfg.semantic_fire_probability.to_string()),
        ("keyword_floor_probability".into(), cfg.keyword_floor_probability.to_string()),
        ("near_miss_margin".into(), cfg.near_miss_margin.to_string()),
        (
            "refire_presets".into(),
            presets.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "),
        ),
        ("secret_path_deny".into(), cfg.secret_path_deny.to_string()),
        (
            "targets".into(),
            format!(
                "{}{}",
                cfg.targets().iter().map(|t| t.path.clone()).collect::<Vec<_>>().join(", "),
                if cfg.targets_explicit() { "" } else { " (implicit)" }
            ),
        ),
    ];
    for (k, v) in rows {
        t.add_owned(vec![k, v]);
    }
    t.print();
    eprintln!("stored: {}", paths::user_config().display());
    match &cfg.target_config {
        Some(p) => eprintln!("target:  {} (layered for {})", p.display(), paths::current_config_dir().display()),
        None => eprintln!("target:  no target config for {}", paths::current_config_dir().display()),
    }
    Ok(())
}

fn list_or_none(v: &[String]) -> String {
    if v.is_empty() {
        "(none)".to_string()
    } else {
        v.join(", ")
    }
}

// ── Targets ─────────────────────────────────────────────────────

/// The converged state of one target, read from its directory.
pub fn target_state(t: &Target) -> String {
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

pub fn targets(json: bool) -> Result<()> {
    let cfg = Config::load(&project_dir());
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
                    "state": target_state(t),
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
        println!("  ways config target plan <dir>   # what activating a Claude Code config dir would do");
        println!("  ways config target add <dir>    # activate it (default dir: ~/.claude)");
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
            target_state(target),
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

pub fn target_plan(dir: &str, json: bool) -> Result<()> {
    let (_, canonical) = resolve_dir(dir)?;
    let plan = reconcile::plan_target(&canonical)?;
    print_plan(&plan, json)
}

pub fn target_add(dir: &str, force: bool, dry_run: bool, json: bool) -> Result<()> {
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
    let cfg = Config::load(&project_dir());
    let mut list = cfg.targets();
    let target = match find_target(&list, dir) {
        Some(i) => {
            list[i].enabled = true;
            list[i].clone()
        }
        None => {
            let t = Target::new(stored);
            list.push(t.clone());
            t
        }
    };
    let path = Config::write_user_targets(&list)?;
    // Under --json the plan document is the whole of stdout; the apply logs
    // to stderr.
    if let Err(e) = reconcile::run_for_targets(&[target], false, json, force) {
        eprintln!(
            "target recorded in {} but the reconcile failed; `ways reconcile` retries it, \
             `ways config target remove` forgets it",
            path.display()
        );
        return Err(e);
    }
    eprintln!("target recorded in {}", path.display());
    Ok(())
}

fn set_enabled(dir: &str, enabled: bool) -> Result<()> {
    let cfg = Config::load(&project_dir());
    let mut list = cfg.targets();
    let Some(i) = find_target(&list, dir) else {
        bail!("{dir} is not a target; `ways config targets` lists them, `ways config target add` adds one");
    };
    list[i].enabled = enabled;
    let target = list[i].clone();
    if enabled && !target.dir().is_dir() {
        bail!("{} does not exist; a target must be a directory to enable", target.dir().display());
    }
    Config::write_user_targets(&list)?;
    reconcile::run_for_targets(&[target], false, false, false)
}

pub fn target_enable(dir: &str) -> Result<()> {
    set_enabled(dir, true)
}

pub fn target_disable(dir: &str) -> Result<()> {
    set_enabled(dir, false)
}

pub fn target_remove(dir: &str) -> Result<()> {
    let cfg = Config::load(&project_dir());
    let mut list = cfg.targets();
    let Some(i) = find_target(&list, dir) else {
        bail!("{dir} is not a target");
    };
    let mut target = list.remove(i);
    target.enabled = false;
    // Withdraw first, then forget: a removed target is a withdrawn one. A
    // directory that is gone has nothing to withdraw from.
    if target.dir().is_dir() {
        reconcile::run_for_targets(&[target], false, false, false)?;
    }
    let path = Config::write_user_targets(&list)?;
    eprintln!("target removed from {}", path.display());
    Ok(())
}
