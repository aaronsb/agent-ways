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

use super::attend_mod::{self, Degraded, Mod, State, Target, View, JUDGE, MODS};
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

/// The targets, read from the user config now: the settings screens read
/// the mods again while open, after a target may have been added or
/// disabled.
fn targets() -> Vec<crate::config::Target> {
    crate::config::Config::load(&crate::util::project_dir()).targets()
}

fn enabled() -> Vec<Enabled> {
    targets().into_iter().filter(|t| t.enabled).map(|t| Enabled { dir: t.dir(), config: t.config_path() }).collect()
}

/// The enabled targets, or with `--target` the one it names.
fn chosen(target: Option<&str>) -> Result<Vec<Enabled>> {
    let all = enabled();
    let Some(dir) = target else { return Ok(all) };
    let dir = crate::config::Target::new(dir).dir();
    match targets().into_iter().find(|t| t.matches_dir(&dir)) {
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
        // The ways-theme mod's other half is the settings key that selects its
        // theme, which the settings merge holds while the mod is installed.
        if only.is_none_or(|m| *m == attend_mod::THEME) && src.join("settings.json").exists() {
            lines.extend(theme_lines(&src.join("settings.json"), &t.dir, dry_run));
        }
        let prefix = targets.len() > 1;
        out.push((t.dir.clone(), lines.into_iter().map(|l| if prefix { format!("{}: {l}", shown(&t.dir)) } else { l }).collect()));
    }
    out
}

/// The settings merge's theme lines for one target after its mods have
/// converged: what it would change, or what it changed.
fn theme_lines(src_settings: &Path, dir: &Path, dry_run: bool) -> Vec<String> {
    let label = attend_mod::THEME.label;
    let base = super::reconcile::base_path_for(&crate::paths::state_root(), dir);
    let done = if dry_run {
        super::reconcile::theme_plan(src_settings, dir, &base, super::reconcile::theme_claim_ahead(dir))
    } else {
        super::reconcile::merge_settings(src_settings, dir, &base).map(|line| if line.contains("up to date") { Vec::new() } else { vec![line] })
    };
    match done {
        Ok(lines) => lines.into_iter().map(|l| format!("{label}: {l}")).collect(),
        Err(e) => vec![format!("{label}: ⚠ settings.json theme: {e:#}")],
    }
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
        None if *m == JUDGE && super::settings::engine::claude_code_chosen() => {
            format!("mods.{}.install: unset, wanted by gate.engine: claude-code; repair writes it", m.name)
        }
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
            Err(why) if v.claude_code.is_none() => out.push((
                Mark::Warn,
                format!("{why}. Fix: install Claude Code {} or later and put `claude` on PATH, then `ways manage mod repair {name} --commit`", m.min_claude_code),
            )),
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

/// The theme change converging the ways-theme mod to `want` would make in
/// one target, for the what-if of `install` and `uninstall`: the key's value
/// before and after, and the theme file written or removed. The merge holds
/// the key once the mod is installed, so a wanted mod counts as installed
/// where this host can load it.
fn would_theme(m: &Mod, v: &View, want: bool, src_settings: &Path, base: &Path) -> Vec<String> {
    if *m != attend_mod::THEME || !src_settings.exists() {
        return Vec::new();
    }
    let claim = want && (v.installed.is_some() || (v.error.is_none() && v.host.is_ok()));
    super::reconcile::theme_plan(src_settings, &v.dir, base, claim).unwrap_or_else(|e| vec![format!("⚠ settings.json theme: {e:#}")])
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
        // The engine choice, unless both halves already hold.
        if !super::settings::engine::claude_code_chosen() || ways_core::settings::mod_install_in_user(name) != Some(true) {
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
                let base = super::reconcile::base_path_for(&crate::paths::state_root(), &t.dir);
                for line in would_theme(m, &v, plan.want, &src.join("settings.json"), &base) {
                    item(&mut r, &line, Mark::Act);
                }
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
    // An explicit want: a removal by hand no repair has seen yet is not
    // written back as `install: false` (the engine choice does this itself).
    if plan.want && plan.engine.is_none() {
        for t in targets {
            attend_mod::forget(&t.dir, m).map_err(|e| anyhow::anyhow!("forgetting {} in {}: {e}", m.id, t.dir.display()))?;
        }
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
    let claude_code = super::settings::engine::claude_code_chosen();
    attend_mod::views(&src, &t, None)
        .into_iter()
        .map(|(m, v)| {
            let shipped = attend_mod::shipped_version(&m, &src);
            let flag = attention(&m, &v, claude_code, &default_root).map(|a| format!(" · ⚠ {} {}", m.name, a.why.word())).unwrap_or_default();
            format!("{} {}: {}{flag}", m.id, v.state(shipped.as_deref()).word(), v.describe(&m, shipped.as_deref()))
        })
        .collect()
}

/// `ways status --json`'s mods of one enabled target, by name.
pub fn status_json(dir: &Path, config: &Path) -> serde_json::Value {
    let src = source();
    let default_root = crate::paths::projection_root();
    let t = Target { dest_root: dir, default_root: &default_root, config };
    let claude_code = super::settings::engine::claude_code_chosen();
    let mut out = serde_json::Map::new();
    for (m, v) in attend_mod::views(&src, &t, None) {
        let shipped = attend_mod::shipped_version(&m, &src);
        let degraded = attention(&m, &v, claude_code, &default_root).map(|a| a.why.word());
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
                "degraded": degraded,
            }),
        );
    }
    serde_json::Value::Object(out)
}

/// One mod as the settings screens' mod tab shows it (ADR-512 item 6): its
/// shipped version, its word over every enabled target, and a row per
/// target.
#[derive(Debug, Clone)]
pub struct Overview {
    pub m: Mod,
    pub shipped: Option<String>,
    pub word: State,
    /// Each enabled target as the tab shows it: the directory, the state
    /// with its deciding word first, and the state in full.
    pub rows: Vec<(String, String, String)>,
    /// An installed copy is older than the shipped one: repair updates it.
    pub older: bool,
    /// `gate.engine` names a keyless profile, which the ways-judge mod hosts.
    pub claude_code: bool,
    /// Each enabled target where the mod is critical and does not run.
    pub attention: Vec<Attention>,
    /// The versions Claude Code lists as installed, once each, in target
    /// order.
    pub installed: Vec<String>,
    /// The marketplace folder the mod is read from.
    pub folder: PathBuf,
}

/// A critical mod that does not run in one enabled target
/// ([`View::degraded`]), as the settings screens flag it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    /// The target, as shown.
    pub dir: String,
    pub why: Degraded,
    /// What brings it back: a command, or the mod tab's action.
    pub fix: String,
}

/// `m` in the target `v` reads, where it is critical under the engine
/// (`claude_code`: `gate.engine` is `claude-code`) and does not run.
pub fn attention(m: &Mod, v: &View, claude_code: bool, default_root: &Path) -> Option<Attention> {
    if !m.critical_under(claude_code) {
        return None;
    }
    let why = v.degraded()?;
    let name = m.name;
    // Repair installs a wanted mod nobody removed; one kept out or removed
    // by hand takes the install action.
    let repairs = v.wanted && !v.recorded;
    let install = format!("`ways manage mod install {name} --commit`");
    let then = if repairs { format!("`ways manage mod repair {name} --commit`") } else { install.clone() };
    let floor = m.min_claude_code;
    let fix = match why {
        Degraded::Missing if repairs => "`ways manage repair --commit` installs it".to_string(),
        Degraded::Missing => format!("the install action, {install}, installs it"),
        Degraded::Disabled => format!("`{}` enables it", attend_mod::enable_command(m, &v.dir, default_root)),
        Degraded::Pending if v.claude_code.is_some() => {
            format!("upgrade Claude Code to {floor} or later, then {}", if repairs { "`ways manage update --commit`" } else { install.as_str() })
        }
        // Without `claude` nothing was read; with it, only its version was not.
        Degraded::Pending if !v.read => format!("install Claude Code {floor} or later and put `claude` on PATH, then {then}"),
        Degraded::Pending => {
            let why = v.host.as_ref().err().map(String::as_str).unwrap_or_default();
            format!("Claude Code's version could not be read ({why}); once `claude --version` names {floor} or later, {then}")
        }
    };
    Some(Attention { dir: shown(&v.dir), why, fix })
}

/// Every shipped mod over the enabled targets, read now: Claude Code's
/// plugin lists and version per target, and repair's record.
pub fn overview() -> Vec<Overview> {
    let src = source();
    let default_root = crate::paths::projection_root();
    let targets = enabled();
    let views: Vec<Vec<(Mod, View)>> = targets.iter().map(|t| attend_mod::views(&src, &t.target(&default_root), None)).collect();
    let claude_code = super::settings::engine::claude_code_chosen();
    MODS.into_iter()
        .map(|m| {
            let shipped = attend_mod::shipped_version(&m, &src);
            let here: Vec<&View> = views.iter().filter_map(|v| v.iter().find(|(n, _)| *n == m).map(|(_, v)| v)).collect();
            let older = here
                .iter()
                .any(|v| v.installed.as_deref().zip(shipped.as_deref()).is_some_and(|(i, s)| super::update::version_older(i, s)));
            let mut installed: Vec<String> = Vec::new();
            for v in here.iter().filter_map(|v| v.installed.clone()) {
                if !installed.contains(&v) {
                    installed.push(v);
                }
            }
            Overview {
                m,
                installed,
                folder: attend_mod::marketplace_dir(&src),
                word: State::over(&here.iter().map(|v| v.state(shipped.as_deref())).collect::<Vec<_>>()),
                rows: here.iter().map(|v| (shown(&v.dir), v.row(&m, shipped.as_deref()), v.describe(&m, shipped.as_deref()))).collect(),
                attention: here.iter().filter_map(|v| attention(&m, v, claude_code, &default_root)).collect(),
                shipped,
                older,
                claude_code,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use attend_mod::{MarketState, ATTEND, THEME};

    fn view(dir: &Path, installed: Option<&str>, host: Result<String, String>) -> View {
        View {
            dir: dir.to_path_buf(),
            wanted: true,
            target_off: false,
            installed: installed.map(str::to_string),
            enabled: installed.map(|_| true),
            recorded: installed.is_some(),
            market: MarketState::Ours,
            foreign: vec![],
            claude_code: host.as_ref().ok().cloned(),
            host,
            read: true,
            error: None,
            runs: None,
        }
    }

    #[test]
    fn the_theme_mod_what_if_names_the_theme_change() {
        let dir = std::env::temp_dir().join(format!("ways-mod-what-if-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (src, base) = (dir.join("src.json"), dir.join("base.json"));
        std::fs::write(&src, "{}").unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"theme":"light"}"#).unwrap();
        let file = dir.join("themes").join("ways.json").display().to_string();
        let ok = || Ok("2.1.309".to_string());

        let install = would_theme(&THEME, &view(&dir, None, ok()), true, &src, &base);
        assert_eq!(install, vec![r#"would change settings.json theme: "light" → "custom:ways""#.to_string(), format!("would write {file}")]);
        // A host below the floor installs nothing, so nothing is claimed.
        assert!(would_theme(&THEME, &view(&dir, None, Err("Claude Code 2.1.300 is older".into())), true, &src, &base).is_empty());
        // Another mod moves no theme.
        assert!(would_theme(&ATTEND, &view(&dir, None, ok()), true, &src, &base).is_empty());

        // Claimed over light: uninstalling gives it back.
        std::fs::write(dir.join("settings.json"), r#"{"theme":"custom:ways"}"#).unwrap();
        std::fs::write(&base, r#"{"theme":"custom:ways","prior_theme":"light"}"#).unwrap();
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(&file, "{}").unwrap();
        let uninstall = would_theme(&THEME, &view(&dir, Some("1.45.0"), ok()), false, &src, &base);
        assert_eq!(uninstall, vec![r#"would change settings.json theme: "custom:ways" → "light""#.to_string(), format!("would remove {file}")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A critical mod that is missing, disabled or pending needs the
    /// operator, whatever its wanted state; ways-theme never does, and
    /// ways-judge only under gate.engine: claude-code.
    #[test]
    fn a_critical_mod_that_does_not_run_needs_attention() {
        let dir = Path::new("/home/u/.claude");
        let ok = || Ok("2.1.310".to_string());
        let why = |m: &Mod, v: &View, claude_code: bool| attention(m, v, claude_code, dir).map(|a| (a.why, a.fix));

        assert_eq!(why(&ATTEND, &view(dir, Some("0.29.0"), ok()), true), None, "installed and enabled");

        let missing = View { recorded: false, ..view(dir, None, ok()) };
        let (d, fix) = why(&ATTEND, &missing, true).unwrap();
        assert_eq!((d, fix.as_str()), (Degraded::Missing, "`ways manage repair --commit` installs it"));
        // Removed by hand, then kept out by the install false repair wrote.
        let kept_out = View { wanted: false, target_off: true, recorded: false, ..view(dir, None, ok()) };
        let (d, fix) = why(&ATTEND, &kept_out, true).unwrap();
        assert_eq!(d, Degraded::Missing);
        assert!(fix.contains("ways manage mod install attend --commit"), "{fix}");

        let disabled = View { enabled: Some(false), ..view(dir, Some("0.29.0"), ok()) };
        let (d, fix) = why(&ATTEND, &disabled, true).unwrap();
        assert_eq!(d, Degraded::Disabled);
        assert!(fix.contains("claude plugin enable attend@agent-ways"), "{fix}");

        let below = View { claude_code: Some("2.1.300".into()), ..view(dir, None, Err("Claude Code 2.1.300 is older than 2.1.304".into())) };
        let (d, fix) = why(&JUDGE, &below, true).unwrap();
        assert_eq!(d, Degraded::Pending);
        assert!(fix.contains("upgrade Claude Code to 2.1.304"), "{fix}");
        // A mod repair will not install takes the install action after the
        // upgrade.
        let below_kept_out = View { wanted: false, target_off: true, ..below.clone() };
        let (d, fix) = why(&JUDGE, &below_kept_out, true).unwrap();
        assert_eq!(d, Degraded::Pending);
        assert_eq!(fix, "upgrade Claude Code to 2.1.304 or later, then `ways manage mod install ways-judge --commit`");
        let below_removed = View { recorded: true, ..below };
        assert!(why(&JUDGE, &below_removed, true).unwrap().1.ends_with("then `ways manage mod install ways-judge --commit`"));

        let no_claude = View { read: false, ..view(dir, None, Err("`claude` is not on PATH".into())) };
        assert_eq!(
            why(&ATTEND, &no_claude, true).unwrap().1,
            "install Claude Code 2.1.296 or later and put `claude` on PATH, then `ways manage mod repair attend --commit`"
        );
        let no_claude_kept_out = View { wanted: false, target_off: true, ..no_claude };
        assert!(why(&ATTEND, &no_claude_kept_out, true).unwrap().1.ends_with("on PATH, then `ways manage mod install attend --commit`"));
        // `claude` is there and its plugin lists answer; only its version
        // could not be read.
        let unread = view(dir, None, Err("`claude --version` failed: exit 1".into()));
        let fix = why(&ATTEND, &unread, true).unwrap().1;
        assert!(fix.starts_with("Claude Code's version could not be read (`claude --version` failed: exit 1)") && !fix.contains("on PATH"), "{fix}");

        assert_eq!(why(&THEME, &missing, true), None, "ways-theme is not critical");
        assert_eq!(why(&JUDGE, &missing, false), None, "ways-judge under a key engine is absent by design");
        assert_eq!(why(&JUDGE, &missing, true).map(|w| w.0), Some(Degraded::Missing));

        // Nothing is claimed when Claude Code could not tell, or when a
        // plugin of the name or the marketplace is the operator's own,
        // which repair leaves alone.
        assert_eq!(why(&ATTEND, &View { error: Some("timed out".into()), ..missing.clone() }, true), None);
        assert_eq!(why(&ATTEND, &View { foreign: vec!["attend@mine".into()], ..missing.clone() }, true), None);
        assert_eq!(why(&ATTEND, &View { market: MarketState::Foreign("/elsewhere/plugins".into()), ..missing }, true), None);
    }
}
