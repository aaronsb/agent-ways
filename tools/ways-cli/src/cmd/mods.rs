//! `ways manage mod` (ADR-512 item 5): the lifecycle of the Claude Code mods
//! agent-ways ships, over every enabled target.
//!
//! `list` and `check` read. `install`, `uninstall` and `repair` report what
//! they would change until `--commit` (ADR-511) and follow
//! `install.manage_mode`. `install` and `uninstall` write a mod's wanted
//! state, `mods.<name>.install`, and then converge it as repair does; for
//! ways-judge they are the engine choice (ADR-512 item 4).

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use super::attend_mod::{self, Mod, State, Target, View, JUDGE, MODS};
use super::lifecycle::{self, Mark, Mode, Report};

/// The exit code of `check` with any problem, as `ways settings lint`.
const PROBLEMS: i32 = 3;

/// One enabled target: its directory and its ways config.
struct Enabled {
    dir: PathBuf,
    config: PathBuf,
}

impl Enabled {
    fn target<'a>(&'a self, default_root: &'a Path) -> Target<'a> {
        Target { dest_root: &self.dir, default_root, config: &self.config }
    }
}

fn enabled() -> Vec<Enabled> {
    crate::config::global().targets().into_iter().filter(|t| t.enabled).map(|t| Enabled { dir: t.dir(), config: t.config_path() }).collect()
}

/// The enabled targets, or with `--target` the one it names.
fn chosen(target: Option<&str>) -> Result<Vec<Enabled>> {
    let all = enabled();
    let Some(dir) = target else { return Ok(all) };
    let dir = crate::config::Target::new(dir).dir();
    match crate::config::global().targets().into_iter().find(|t| t.matches_dir(&dir)) {
        Some(t) if t.enabled => Ok(vec![Enabled { dir: t.dir(), config: t.config_path() }]),
        Some(_) => bail!("{} is a disabled target; `ways manage target enable {} --commit` enables it", dir.display(), dir.display()),
        None => bail!("{} is not a target; `ways manage target list` lists them", dir.display()),
    }
}

/// The shipped mod named `name`.
fn named(name: &str) -> Result<Mod> {
    MODS.into_iter().find(|m| m.name == name).ok_or_else(|| {
        let names: Vec<&str> = MODS.iter().map(|m| m.name).collect();
        anyhow::anyhow!("unknown mod {name}; the shipped mods are {}", names.join(", "))
    })
}

fn source() -> PathBuf {
    crate::paths::data_root()
}

fn shown(dir: &Path) -> String {
    let home = claude_sessions::home_dir();
    match dir.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => dir.display().to_string(),
    }
}

/// Converge `only`, or every mod, in every enabled target, as repair's mod
/// stage does. Returns the lines to report, each naming its target where
/// there is more than one.
pub fn converge(only: Option<&Mod>, dry_run: bool) -> Vec<String> {
    converge_in(&enabled(), only, dry_run).into_iter().flat_map(|(_, lines)| lines).collect()
}

fn converge_in(targets: &[Enabled], only: Option<&Mod>, dry_run: bool) -> Vec<(PathBuf, Vec<String>)> {
    let default_root = crate::paths::projection_root();
    let src = source();
    let mut out = Vec::new();
    let align = if only.is_none_or(|m| *m == JUDGE) { attend_mod::align_judge(dry_run) } else { Vec::new() };
    for (i, t) in targets.iter().enumerate() {
        let mut lines = if i == 0 { align.clone() } else { Vec::new() };
        lines.extend(attend_mod::converge_target(&src, &t.target(&default_root), only, dry_run));
        let prefix = targets.len() > 1;
        out.push((t.dir.clone(), lines.into_iter().map(|l| if prefix { format!("{}: {l}", shown(&t.dir)) } else { l }).collect()));
    }
    out
}

/// A line from the convergence as a report item: a ⚠ line is a warning.
fn item(r: &mut Report, line: &str, done: Mark) {
    match line.split_once("⚠ ") {
        Some((head, tail)) => r.item(Mark::Warn, format!("{head}{tail}")),
        None => r.item(done, line),
    };
}

/// `ways manage mod list`: each shipped mod, its shipped version and wanted
/// state, and its state in each enabled target.
pub fn list() -> Result<()> {
    let src = source();
    let default_root = crate::paths::projection_root();
    let targets = enabled();
    let views: Vec<Vec<(Mod, View)>> = targets.iter().map(|t| attend_mod::views(&src, &t.target(&default_root), None)).collect();
    let mut r = Report::new("ways manage mod list");
    for m in MODS {
        let shipped = attend_mod::shipped_version(&m, &src);
        let here: Vec<&View> = views.iter().filter_map(|v| v.iter().find(|(n, _)| *n == m).map(|(_, v)| v)).collect();
        let word = State::over(&here.iter().map(|v| v.state(shipped.as_deref())).collect::<Vec<_>>()).word();
        r.section(format!("{}  {}  {word}", m.name, shipped.as_deref().unwrap_or("not shipped")));
        r.item(Mark::Info, wanted_line(&m));
        for v in here {
            r.item(Mark::Info, format!("{}: {}", shown(&v.dir), v.describe(&m, shipped.as_deref())));
        }
    }
    if targets.is_empty() {
        r.footer(Mark::Info, "no enabled target; `ways manage target list` lists them");
    }
    r.print();
    Ok(())
}

/// Where a mod's wanted state comes from in the user config.
fn wanted_line(m: &Mod) -> String {
    let user = ways_core::settings::mod_install_in_user(m.name);
    let default = m.name == attend_mod::ATTEND.name;
    let base = match user {
        Some(v) => format!("mods.{}.install: {v} (user config)", m.name),
        None => format!("mods.{}.install: {default} (default)", m.name),
    };
    if *m == JUDGE {
        format!("{base}; it follows gate.engine, which {}", engine_phrase())
    } else {
        base
    }
}

fn engine_phrase() -> String {
    use ways_agent_core::profile::{user_layer_path, UserLayer};
    match UserLayer::load(&user_layer_path()).ok().and_then(|u| u.engine) {
        Some(e) => format!("is {e}"),
        None => "is unset".to_string(),
    }
}

/// `ways manage mod check [<mod>]`: each check per mod and target, a ⚠ line
/// with its fix for each problem. Exits 3 with any problem.
pub fn check(name: Option<&str>) -> Result<()> {
    let only = name.map(named).transpose()?;
    let src = source();
    let default_root = crate::paths::projection_root();
    let targets = enabled();
    let mut r = Report::new("ways manage mod check");
    let mut problems = 0;
    let claude_code = super::settings::engine::claude_code_chosen();
    for t in &targets {
        for (m, v) in attend_mod::views(&src, &t.target(&default_root), only.as_ref()) {
            r.section(format!("{} — {}", m.name, shown(&t.dir)));
            for (mark, text) in checks(&m, &v, attend_mod::shipped_version(&m, &src).as_deref(), claude_code, &default_root) {
                if mark == Mark::Warn {
                    problems += 1;
                }
                r.item(mark, text);
            }
        }
    }
    if targets.is_empty() {
        r.footer(Mark::Info, "no enabled target; `ways manage target list` lists them");
    } else if problems == 0 {
        r.footer(Mark::Ok, "every check passes");
    } else {
        r.footer(Mark::Info, format!("{problems} problem{}; each warning names its fix", if problems == 1 { "" } else { "s" }));
    }
    r.print();
    if problems > 0 {
        std::process::exit(PROBLEMS);
    }
    Ok(())
}

/// The checks of one mod in one target, each passing or a ⚠ with its fix.
fn checks(m: &Mod, v: &View, shipped: Option<&str>, claude_code: bool, default_root: &Path) -> Vec<(Mark, String)> {
    let name = m.name;
    let dir = shown(&v.dir);
    let repair = format!("`ways manage mod repair {name} --commit`");
    let mut out = Vec::new();
    if let Some(e) = &v.error {
        out.push((Mark::Warn, format!("Claude Code's plugins could not be read: {e}. Fix: install Claude Code, or put `claude` on PATH")));
        return out;
    }
    if v.wanted {
        match &v.host {
            Ok(version) => out.push((Mark::Ok, format!("Claude Code {version} is at or above the floor, {}", m.min_claude_code))),
            Err(why) => out.push((
                Mark::Warn,
                format!("{why}. Fix: upgrade Claude Code to {} or later, then `ways manage update --commit`", m.min_claude_code),
            )),
        }
    }
    for f in &v.foreign {
        out.push((Mark::Warn, format!("{f} is installed from another marketplace and left as it is. Fix: uninstall it, then {repair}")));
    }
    if let attend_mod::MarketState::Foreign(from) = &v.market {
        out.push((
            Mark::Warn,
            format!("the {} marketplace comes from {from}, which this app did not add. Fix: remove it from this target, then {repair}", attend_mod::MARKETPLACE),
        ));
    } else if v.installed.is_some() {
        out.push((Mark::Ok, format!("the {} marketplace is this app's", attend_mod::MARKETPLACE)));
    }
    match (v.wanted, v.installed.as_deref()) {
        (true, Some(i)) if shipped.is_some_and(|s| super::update::version_older(i, s)) => {
            out.push((Mark::Warn, format!("installed {i}, older than the shipped {}. Fix: {repair}", shipped.unwrap_or("?"))))
        }
        (true, Some(i)) => out.push((Mark::Ok, format!("installed {i}, as wanted"))),
        (true, None) if v.recorded => out.push((
            Mark::Warn,
            format!("removed by hand while wanted. Fix: {repair} writes install false here, or `ways manage mod install {name} --target {dir} --commit` installs it again"),
        )),
        // A host below the floor is the one line that says so.
        (true, None) if v.host.is_err() => {}
        (true, None) => out.push((Mark::Warn, format!("wanted and not installed. Fix: {repair}"))),
        (false, Some(i)) => out.push((Mark::Warn, format!("installed {i}, not wanted. Fix: {repair} uninstalls it"))),
        (false, None) if v.target_off => {
            out.push((Mark::Ok, format!("kept out by this target's config (install false); `ways manage mod install {name} --target {dir} --commit` installs it")))
        }
        (false, None) => out.push((Mark::Ok, "not wanted, not installed".to_string())),
    }
    if v.installed.is_some() {
        match v.enabled {
            Some(false) => out.push((
                Mark::Warn,
                format!("disabled in Claude Code. Fix: `{}`", attend_mod::enable_command(m, &v.dir, default_root)),
            )),
            _ => out.push((Mark::Ok, "enabled in Claude Code".to_string())),
        }
    }
    if *m == JUDGE {
        match (claude_code, v.wanted) {
            (true, false) => out.push((
                Mark::Warn,
                "gate.engine is claude-code, and this target keeps the ways-judge mod out, so its sessions find no judge host. Fix: `ways manage mod install ways-judge --commit`, or `ways judge-setup` to pick a key engine".to_string(),
            )),
            (false, true) => out.push((
                Mark::Warn,
                format!("the ways-judge mod is wanted here while gate.engine is not claude-code. Fix: `ways manage mod uninstall ways-judge --target {dir} --commit`, or `ways settings set gate.engine claude-code`"),
            )),
            _ => out.push((Mark::Ok, "gate.engine and the mod's wanted state agree".to_string())),
        }
    }
    out
}

/// What converging `m` would do in one target once its wanted state is
/// `want`, read from its view; for the what-if of `install` and
/// `uninstall`, whose setting is not written yet.
fn would(m: &Mod, v: &View, want: bool, shipped: Option<&str>) -> (Mark, String) {
    let id = m.id;
    if let Some(e) = &v.error {
        return (Mark::Warn, format!("Claude Code's plugins could not be read: {e}"));
    }
    match (want, v.installed.as_deref()) {
        (true, Some(i)) if shipped.is_some_and(|s| super::update::version_older(i, s)) => {
            (Mark::Act, format!("would run `claude plugin update {id} --scope user`: {i} → {}", shipped.unwrap_or("?")))
        }
        (true, Some(i)) if v.enabled == Some(false) => (Mark::Same, format!("installed {i} and disabled in Claude Code; it stays disabled")),
        (true, Some(i)) => (Mark::Same, format!("installed {i}; nothing to do")),
        (true, None) => match &v.host {
            Err(why) => (Mark::Warn, format!("pending: {why}; upgrade Claude Code to {} or later", m.min_claude_code)),
            Ok(_) if v.market == attend_mod::MarketState::Absent => (
                Mark::Act,
                format!("would run `claude plugin marketplace add <app>/plugins --scope user` and `claude plugin install {id} --scope user`"),
            ),
            Ok(_) => (Mark::Act, format!("would run `claude plugin install {id} --scope user`")),
        },
        (false, Some(i)) => (Mark::Act, format!("would run `claude plugin uninstall {id} --scope user` (installed {i})")),
        (false, None) => (Mark::Same, "not installed; nothing to do".to_string()),
    }
}

/// A settings write of `install` and the mod's convergence.
struct Plan {
    /// Each write: the file, `Some(value)` to set or `None` to remove, and
    /// how the report names it.
    writes: Vec<(PathBuf, Option<bool>, String)>,
    /// For ways-judge, the engine to write, through the engine choice.
    engine: Option<Option<String>>,
    /// Whether the mod is wanted in the converged targets afterwards.
    want: bool,
}

/// `ways manage mod install <mod>`.
pub fn install(name: &str, target: Option<&str>, mode: Mode) -> Result<()> {
    let m = named(name)?;
    let targets = chosen(target)?;
    let mut plan = Plan { writes: Vec::new(), engine: None, want: true };
    let key = format!("mods.{name}.install");
    // A target's own false is removed: in every enabled target, or the one
    // --target names.
    for t in &targets {
        if ways_core::settings::mod_install_in_file(name, &t.config) == Some(false) {
            plan.writes.push((t.config.clone(), None, format!("remove {key}: false from {}", t.config.display())));
        }
    }
    if m == JUDGE {
        if !super::settings::engine::claude_code_chosen() {
            plan.engine = Some(Some("claude-code".to_string()));
        }
    } else if target.is_none() && ways_core::settings::mod_install_in_user(name) != Some(true) {
        plan.writes.push((ways_core::paths::user_config(), Some(true), format!("write {key}: true in {}", ways_core::paths::user_config().display())));
    } else if let (Some(_), Some(t)) = (target, targets.first()) {
        // With the target's false gone, the user config may still say false.
        if ways_core::settings::mod_install_in_user(name) == Some(false) {
            plan.writes.push((t.config.clone(), Some(true), format!("write {key}: true in {}", t.config.display())));
        }
    }
    run(&m, &format!("ways manage mod install {name}{}", target.map(|t| format!(" --target {t}")).unwrap_or_default()), &targets, plan, mode)
}

/// `ways manage mod uninstall <mod>`.
pub fn uninstall(name: &str, target: Option<&str>, mode: Mode) -> Result<()> {
    let m = named(name)?;
    let targets = chosen(target)?;
    let key = format!("mods.{name}.install");
    if m == JUDGE && target.is_none() && super::settings::engine::claude_code_chosen() {
        // Removing the judge's host from every target would leave the gate
        // failing open or paying for a key: the engine choice decides.
        eprintln!(
            "ways manage mod uninstall ways-judge: gate.engine is claude-code, which the ways-judge mod hosts; nothing changed. \
             Run `ways judge-setup` to pick a key engine or no judge, which removes the mod, or pass --target <dir> to keep it out of one target"
        );
        std::process::exit(PROBLEMS);
    }
    let mut plan = Plan { writes: Vec::new(), engine: None, want: false };
    match (target, targets.first()) {
        (Some(_), Some(t)) => plan.writes.push((t.config.clone(), Some(false), format!("write {key}: false in {}", t.config.display()))),
        _ => {
            let user = ways_core::paths::user_config();
            if ways_core::settings::mod_install_in_user(name) != Some(false) {
                plan.writes.push((user.clone(), Some(false), format!("write {key}: false in {}", user.display())));
            }
        }
    }
    run(&m, &format!("ways manage mod uninstall {name}{}", target.map(|t| format!(" --target {t}")).unwrap_or_default()), &targets, plan, mode)
}

/// `ways manage mod repair [<mod>]`: converge with no settings write.
pub fn repair(name: Option<&str>, mode: Mode) -> Result<()> {
    let only = name.map(named).transpose()?;
    let command = format!("ways manage mod repair{}", name.map(|n| format!(" {n}")).unwrap_or_default());
    let dry_run = mode == Mode::WhatIf;
    let mut r = Report::new(if dry_run { format!("{command}: what-if") } else { command.clone() });
    let targets = enabled();
    for (dir, lines) in converge_in(&targets, only.as_ref(), dry_run) {
        r.section(shown(&dir));
        if lines.is_empty() {
            r.item(Mark::Same, "converged; nothing to do");
        }
        for l in &lines {
            item(&mut r, l, if dry_run { Mark::Act } else { Mark::Ok });
        }
    }
    if targets.is_empty() {
        r.footer(Mark::Info, "no enabled target; `ways manage target list` lists them");
    } else if dry_run {
        r.footer(Mark::Info, lifecycle::commit_hint(&command));
    }
    r.print();
    Ok(())
}

/// Report or apply one plan: the writes, then the mod's convergence in each
/// of `targets`.
fn run(m: &Mod, command: &str, targets: &[Enabled], plan: Plan, mode: Mode) -> Result<()> {
    let src = source();
    let default_root = crate::paths::projection_root();
    if mode == Mode::WhatIf {
        let mut r = Report::new(format!("{command}: what-if"));
        r.section("settings");
        if let Some(engine) = &plan.engine {
            let to = engine.as_deref().unwrap_or("unset");
            r.item(Mark::Act, format!("write mods.{}.install: true in {}", m.name, ways_core::paths::user_config().display()));
            r.item(Mark::Act, format!("gate.engine: {} → {to}", engine_phrase().trim_start_matches("is ")));
        }
        for (_, _, what) in &plan.writes {
            r.item(Mark::Act, what);
        }
        if plan.engine.is_none() && plan.writes.is_empty() {
            r.item(Mark::Same, "nothing to write");
        }
        let shipped = attend_mod::shipped_version(m, &src);
        for t in targets {
            r.section(shown(&t.dir));
            for (_, v) in attend_mod::views(&src, &t.target(&default_root), Some(m)) {
                let (mark, text) = would(m, &v, plan.want, shipped.as_deref());
                r.item(mark, text);
            }
        }
        r.footer(Mark::Info, lifecycle::commit_hint(command));
        r.print();
        return Ok(());
    }
    let mut r = Report::new(command);
    r.section("settings");
    if let Some(engine) = &plan.engine {
        let written = super::settings::write_engine(engine.as_deref()).map_err(|f| anyhow::anyhow!("{}", f.message))?;
        for (_, path) in written {
            r.item(Mark::Ok, format!("wrote {}", path.display()));
        }
        r.item(Mark::Ok, format!("gate.engine is now {}", engine.as_deref().unwrap_or("unset")));
    }
    for (path, value, what) in &plan.writes {
        super::settings::write_mod_install(path, m.name, *value).map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        r.item(Mark::Ok, what.replacen("write", "wrote", 1).replacen("remove", "removed", 1));
    }
    if plan.engine.is_none() && plan.writes.is_empty() {
        r.item(Mark::Same, "nothing to write");
    }
    for (dir, lines) in converge_in(targets, Some(m), false) {
        r.section(shown(&dir));
        if lines.is_empty() {
            r.item(Mark::Same, "converged; nothing to do");
        }
        for l in &lines {
            item(&mut r, l, Mark::Ok);
        }
    }
    r.print();
    Ok(())
}

/// The status lines `ways status` prints for one enabled target: one per
/// shipped mod.
pub fn status_lines(dir: &Path, config: &Path) -> Vec<String> {
    let src = source();
    let default_root = crate::paths::projection_root();
    let t = Target { dest_root: dir, default_root: &default_root, config };
    attend_mod::views(&src, &t, None)
        .into_iter()
        .map(|(m, v)| {
            let shipped = attend_mod::shipped_version(&m, &src);
            format!("{} {}: {}", m.id, v.state(shipped.as_deref()).word(), v.describe(&m, shipped.as_deref()))
        })
        .collect()
}

/// `ways status --json`'s mods of one enabled target, by name.
pub fn status_json(dir: &Path, config: &Path) -> serde_json::Value {
    let src = source();
    let default_root = crate::paths::projection_root();
    let t = Target { dest_root: dir, default_root: &default_root, config };
    let mut out = serde_json::Map::new();
    for (m, v) in attend_mod::views(&src, &t, None) {
        let shipped = attend_mod::shipped_version(&m, &src);
        out.insert(
            m.name.to_string(),
            serde_json::json!({
                "state": v.state(shipped.as_deref()).word(),
                "wanted": v.wanted,
                "installed": v.installed,
                "runs": v.runs,
                "shipped": shipped,
                "enabled": v.enabled,
                "pending": if v.wanted && v.installed.is_none() { v.host.as_ref().err().cloned() } else { None },
                "removed": v.target_off || (v.wanted && v.installed.is_none() && v.recorded),
                "foreign": v.foreign,
                "error": v.error,
            }),
        );
    }
    serde_json::Value::Object(out)
}
