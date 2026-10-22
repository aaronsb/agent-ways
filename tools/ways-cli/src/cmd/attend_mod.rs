//! Installation of the mods agent-ways ships with Claude Code: the attend mod
//! (ADR-406 item 8) and the ways-judge mod (ADR-708 §9).
//!
//! Each mod is a plugin in the app source, `plugins/<name>/`, beside
//! `plugins/.claude-plugin/marketplace.json`, which declares the local
//! marketplace `agent-ways` they share. For each enabled target, after the
//! settings merge and the MCP registration, reconcile adds the marketplace and
//! installs `attend@agent-ways` at user scope through Claude Code's own
//! plugin CLI, and updates both when the installed version is older than the
//! shipped one. A disabled target, and every target once
//! `attend_mod.install` is false, has the plugin uninstalled and the
//! marketplace removed. `ways judge-setup` installs `ways-judge@agent-ways`
//! when the operator picks Claude Code internal and records the request per
//! target; reconcile then keeps it installed and current in each target that
//! holds the record, and withdraws it with attend from a disabled one.
//!
//! Each call runs under `CLAUDE_CONFIG_DIR` by the rule ADR-501 item 9 set for
//! the MCP registration (see [`super::mcp_register::config_dir`]), and is
//! killed past [`super::mcp_register::CLAUDE_TIMEOUT`].
//!
//! The `agent-ways` marketplace is ours when its source is this app's
//! `plugins/` folder; or the folder a repair recorded adding it from, which a
//! moved app leaves behind; or, with no record, a folder whose parent (the
//! app or checkout that held `plugins/`) is gone while the mod is installed
//! from it, since an app that is gone has no other owner. A folder that is
//! only missing its marketplace for now, in a checkout that is still there (a
//! branch without `plugins/`), is reported, not claimed. Any other
//! `agent-ways` marketplace, and a plugin of the mod's name from another
//! marketplace, are the operator's own: reported, with the command that hands
//! the marketplace to this app, and left alone. Removing a marketplace
//! uninstalls its plugins, so a moved app's marketplace is removed, added
//! again and every mod of ours that was installed from it installed again in
//! one pass; and a mod withdrawn while another of ours stays installed leaves
//! the marketplace in place. A missing `claude` binary or a failed call is
//! reported and never fails a reconcile.

use std::path::{Path, PathBuf};

use super::mcp_register::{claude_bin, claude_stdout, config_dir};

/// The marketplace `plugins/.claude-plugin/marketplace.json` declares.
pub const MARKETPLACE: &str = "agent-ways";

/// One mod the `agent-ways` marketplace ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mod {
    /// The plugin's name in its manifest, and its folder under `plugins/`.
    pub name: &'static str,
    /// The plugin as Claude Code knows it once installed.
    pub id: &'static str,
    /// How report lines name it.
    pub label: &'static str,
    /// The oldest Claude Code whose mod API carries every call the mod makes.
    pub min_claude_code: &'static str,
    /// For a mod installed on request rather than by every repair, the file
    /// beside a target's marketplace record that says the request was made.
    pub record: Option<&'static str>,
}

/// attend's agent-facing driver (ADR-406): function-hook modules, which
/// Claude Code 2.1.296 offers. Every repair installs it.
pub const ATTEND: Mod =
    Mod { name: "attend", id: "attend@agent-ways", label: "attend mod", min_claude_code: "2.1.296", record: None };
/// The judge host for the `claude-code` engine (ADR-708 §3). Beside the
/// modules it calls `$.model.complete` and `$.process.spawn`, which the mod
/// types of Claude Code 2.1.304 carry. `ways judge-setup` installs it.
pub const JUDGE: Mod = Mod {
    name: "ways-judge",
    id: "ways-judge@agent-ways",
    label: "ways-judge mod",
    min_claude_code: "2.1.304",
    record: Some("ways-judge-installed"),
};

/// The marketplace folder in the app source.
fn marketplace_dir(source_root: &Path) -> PathBuf {
    source_root.join("plugins")
}

/// The version of `m` the source ships, from the plugin's manifest; `None`
/// when the source has no marketplace (an app from before ADR-406) or does
/// not ship `m`, in which case reconcile leaves Claude Code's plugins alone.
pub fn shipped_version(m: &Mod, source_root: &Path) -> Option<String> {
    let dir = marketplace_dir(source_root);
    if !dir.join(".claude-plugin/marketplace.json").is_file() {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(m.name).join(".claude-plugin/plugin.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")?.as_str().map(str::to_string)
}

/// What Claude Code reports for one config directory, as one mod sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// The `agent-ways` marketplace, if one is configured: `Some(path)` for a
    /// directory source, `Some("")` for any other kind.
    pub marketplace: Option<String>,
    /// The installed version of the mod.
    pub ours: Option<String>,
    /// Ids of plugins of the mod's name from other marketplaces.
    pub foreign: Vec<String>,
    /// Ids of the other plugins installed from the `agent-ways` marketplace.
    pub siblings: Vec<String>,
}

/// Reads `claude plugin marketplace list --json` and `claude plugin list --json`.
pub fn parse_installed(m: &Mod, marketplaces: &str, plugins: &str) -> Installed {
    let mut out = Installed::default();
    if let Ok(serde_json::Value::Array(list)) = serde_json::from_str(marketplaces) {
        if let Some(entry) = list.iter().find(|e| e.get("name").and_then(|n| n.as_str()) == Some(MARKETPLACE)) {
            let is_dir = entry.get("source").and_then(|s| s.as_str()) == Some("directory");
            let path = entry.get("path").and_then(|p| p.as_str()).unwrap_or_default();
            out.marketplace = Some(if is_dir { path.to_string() } else { String::new() });
        }
    }
    if let Ok(serde_json::Value::Array(list)) = serde_json::from_str(plugins) {
        for p in &list {
            let Some(id) = p.get("id").and_then(|i| i.as_str()) else { continue };
            let (name, market) = id.split_once('@').unwrap_or((id, ""));
            if id == m.id {
                out.ours = Some(p.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string());
            } else if name == m.name {
                out.foreign.push(id.to_string());
            } else if market == MARKETPLACE {
                out.siblings.push(id.to_string());
            }
        }
    }
    out
}

/// One call of the plugin CLI. The plugin calls act on the mod being
/// converged, except [`Call::InstallSibling`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    MarketplaceAdd(String),
    MarketplaceRemove,
    MarketplaceUpdate,
    Install,
    /// Install another of ours again, after a marketplace removal took it.
    InstallSibling(String),
    Update,
    Uninstall,
}

impl Call {
    fn args<'a>(&'a self, m: &Mod) -> Vec<&'a str> {
        match self {
            Call::MarketplaceAdd(dir) => vec!["plugin", "marketplace", "add", dir.as_str(), "--scope", "user"],
            Call::MarketplaceRemove => vec!["plugin", "marketplace", "remove", MARKETPLACE],
            Call::MarketplaceUpdate => vec!["plugin", "marketplace", "update", MARKETPLACE],
            Call::Install => vec!["plugin", "install", m.id, "--scope", "user"],
            Call::InstallSibling(id) => vec!["plugin", "install", id.as_str(), "--scope", "user"],
            Call::Update => vec!["plugin", "update", m.id, "--scope", "user"],
            Call::Uninstall => vec!["plugin", "uninstall", m.id, "--scope", "user"],
        }
    }

    fn describe(&self, m: &Mod) -> String {
        match self {
            Call::MarketplaceAdd(dir) => format!("added the {MARKETPLACE} marketplace ({dir})"),
            Call::MarketplaceRemove => format!("removed the {MARKETPLACE} marketplace"),
            Call::MarketplaceUpdate => format!("refreshed the {MARKETPLACE} marketplace"),
            Call::Install => format!("installed {}", m.id),
            Call::InstallSibling(id) => format!("installed {id} again, which removing the marketplace took"),
            Call::Update => format!(
                "updated {}'s recorded version; sessions run the plugin from the {MARKETPLACE} folder, and a running one takes a change with /reload-plugins",
                m.id
            ),
            Call::Uninstall => format!("uninstalled {}", m.id),
        }
    }
}

/// What converging one target takes: the calls in order, and lines to report
/// about what is left alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub calls: Vec<Call>,
    pub notes: Vec<String>,
}

/// Whose the configured `agent-ways` marketplace is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Market {
    Absent,
    /// From this app's `plugins/` folder.
    Current,
    /// From the folder a repair recorded, which the app has since left.
    Moved,
    /// Anyone else's: another folder, or a source that is not a folder.
    Foreign(String),
}

fn market(have: &Installed, dir: &str, recorded: Option<&str>) -> Market {
    let same = |a: &str, b: &str| super::reconcile::same_path(Path::new(a), Path::new(b));
    // The whole app or checkout is gone, not just its marketplace for now.
    let gone = |path: &str| {
        let folder = Path::new(path);
        !folder.join(".claude-plugin/marketplace.json").is_file() && !folder.parent().is_some_and(Path::exists)
    };
    match have.marketplace.as_deref() {
        None => Market::Absent,
        Some("") => Market::Foreign("a source that is not a local folder".to_string()),
        Some(path) if same(path, dir) => Market::Current,
        Some(path) if recorded.is_some_and(|r| same(path, r)) => Market::Moved,
        // Any plugin of ours from it marks it ours, not only this mod: the
        // other mod's pass would otherwise call it foreign.
        Some(path) if (have.ours.is_some() || !have.siblings.is_empty()) && gone(path) => Market::Moved,
        Some(path) => Market::Foreign(path.to_string()),
    }
}

/// The note for a marketplace this app will not touch. `recover` is the
/// command that removes it from this target, after which a repair adds ours.
fn foreign_note(from: &str, recover: &str) -> String {
    format!(
        "an {MARKETPLACE} marketplace from {from}, which this app did not add, is configured; it and its plugins are left as they are. \
         To hand it to this app, run `{recover}` and then `ways manage repair --commit`"
    )
}

/// The exact command that removes the marketplace from one target.
fn recover_command(cfg: Option<&Path>) -> String {
    match cfg {
        Some(d) => format!("CLAUDE_CONFIG_DIR={} claude plugin marketplace remove {MARKETPLACE}", d.display()),
        None => format!("claude plugin marketplace remove {MARKETPLACE}"),
    }
}

/// Install `m`, or bring it up to `shipped`, from `dir`. `recorded` is the
/// folder a repair last added the marketplace from.
pub fn plan_converge(m: &Mod, have: &Installed, dir: &str, recorded: Option<&str>, shipped: &str, recover: &str) -> Plan {
    let mut plan = Plan::default();
    if !have.foreign.is_empty() {
        plan.notes.push(format!(
            "{} is installed from another marketplace; left as it is, and {} is not installed beside it",
            have.foreign.join(", "),
            m.id
        ));
        return plan;
    }
    match market(have, dir, recorded) {
        Market::Foreign(from) => plan.notes.push(foreign_note(&from, recover)),
        Market::Moved => {
            // Removing the marketplace uninstalls every plugin from it.
            plan.calls.extend([Call::MarketplaceRemove, Call::MarketplaceAdd(dir.to_string()), Call::Install]);
            plan.calls.extend(have.siblings.iter().cloned().map(Call::InstallSibling));
        }
        Market::Absent => {
            plan.calls.push(Call::MarketplaceAdd(dir.to_string()));
            plan.calls.extend(plugin_calls(have, shipped, false));
        }
        Market::Current => plan.calls.extend(plugin_calls(have, shipped, true)),
    }
    plan
}

/// The plugin's calls once the marketplace is in place: install it, or update
/// an older one, refreshing the marketplace first unless it was just added.
fn plugin_calls(have: &Installed, shipped: &str, refresh: bool) -> Vec<Call> {
    match have.ours.as_deref() {
        None => vec![Call::Install],
        Some(v) if super::update::version_older(v, shipped) && refresh => vec![Call::MarketplaceUpdate, Call::Update],
        Some(v) if super::update::version_older(v, shipped) => vec![Call::Update],
        Some(_) => Vec::new(),
    }
}

/// Remove `m`, and the marketplace where it is ours, no other of our plugins
/// is installed from it, and no other mod is `wanted_by_other` in this
/// target. Under a marketplace that is not ours, the mod is the operator's
/// too.
pub fn plan_withdraw(have: &Installed, dir: &str, recorded: Option<&str>, recover: &str, wanted_by_other: bool) -> Plan {
    let mut plan = Plan::default();
    match market(have, dir, recorded) {
        Market::Foreign(from) => plan.notes.push(foreign_note(&from, recover)),
        Market::Current | Market::Moved => {
            if have.ours.is_some() {
                plan.calls.push(Call::Uninstall);
            }
            // Another mod installed from it, or recorded as wanted and about
            // to be installed again, keeps it.
            if have.siblings.is_empty() && !wanted_by_other {
                plan.calls.push(Call::MarketplaceRemove);
            }
        }
        Market::Absent if have.ours.is_some() => plan.calls.push(Call::Uninstall),
        Market::Absent => {}
    }
    plan
}

/// Ask Claude Code what one config directory holds.
fn query(m: &Mod, claude: &Path, cfg: Option<&Path>) -> Result<Installed, String> {
    let markets = claude_stdout(claude, cfg, &["plugin", "marketplace", "list", "--json"])?;
    let plugins = claude_stdout(claude, cfg, &["plugin", "list", "--json"])?;
    Ok(parse_installed(m, &markets, &plugins))
}

/// A file of this target's beside its settings merge base.
fn state_file(dest_root: &Path, name: &str) -> PathBuf {
    super::reconcile::base_path_for(&crate::paths::state_root(), dest_root).with_file_name(name)
}

/// Where a target records the folder a repair added the marketplace from.
/// One record serves every mod, since they share the marketplace.
fn record_path(dest_root: &Path) -> PathBuf {
    state_file(dest_root, "attend-mod-marketplace")
}

fn read_record(dest_root: &Path) -> Option<String> {
    std::fs::read_to_string(record_path(dest_root)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// True when `m` is installed on request and the target records that the
/// request was made. Repair keeps such a mod installed from then on: the
/// record outlives a marketplace removal that took the mod with it.
pub fn recorded(m: &Mod, dest_root: &Path) -> bool {
    m.record.is_some_and(|name| state_file(dest_root, name).is_file())
}

/// True when a mod other than `m` is recorded as wanted in the target, so a
/// withdrawal of `m` from an enabled target leaves the marketplace for it.
fn wanted_by_other(m: &Mod, dest_root: &Path) -> bool {
    [ATTEND, JUDGE].iter().any(|o| o.id != m.id && recorded(o, dest_root))
}

/// Records that `m` was installed on request in the target at `dest_root`.
pub fn record_request(m: &Mod, dest_root: &Path) -> std::io::Result<()> {
    let Some(name) = m.record else { return Ok(()) };
    let path = state_file(dest_root, name);
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&path, format!("{}\n", m.id))
}

/// Run a plan's calls in order and keep the record of where the marketplace
/// was added from. A failed marketplace call stops the plan, since every
/// later call needs the marketplace; a failed plugin call is reported and
/// the rest still run, so one mod's failure never costs the other its
/// install. Returns one line per call made and per note.
fn execute(m: &Mod, claude: &Path, cfg: Option<&Path>, dest_root: &Path, plan: &Plan, dry_run: bool) -> Vec<String> {
    let label = m.label;
    let mut lines: Vec<String> = plan.notes.iter().map(|n| format!("{label}: {n}")).collect();
    for call in &plan.calls {
        if dry_run {
            lines.push(format!("{label}: would run `claude {}`", call.args(m).join(" ")));
            continue;
        }
        match claude_stdout(claude, cfg, &call.args(m)) {
            Ok(_) => {
                lines.push(format!("{label}: {}", call.describe(m)));
                let record = record_path(dest_root);
                let kept = match call {
                    Call::MarketplaceAdd(dir) => {
                        std::fs::create_dir_all(record.parent().unwrap_or(Path::new("."))).and_then(|_| std::fs::write(&record, dir))
                    }
                    Call::MarketplaceRemove => std::fs::remove_file(&record).or_else(|e| match e.kind() {
                        std::io::ErrorKind::NotFound => Ok(()),
                        _ => Err(e),
                    }),
                    _ => Ok(()),
                };
                if let Err(e) = kept {
                    lines.push(format!("{label}: ⚠ could not record the marketplace folder in {}: {e}", record.display()));
                }
            }
            Err(e) => {
                lines.push(format!("{label}: ⚠ `claude {}` failed: {e}", call.args(m).join(" ")));
                if matches!(call, Call::MarketplaceAdd(_) | Call::MarketplaceRemove | Call::MarketplaceUpdate) {
                    break;
                }
            }
        }
    }
    lines
}

/// What converging a mod in one enabled target is to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// Install it, or update an older one.
    Install,
    /// Remove it.
    Withdraw,
}

/// Converge `m` in one enabled target. Returns the lines to report; empty
/// when nothing changed.
pub fn converge(m: &Mod, source_root: &Path, dest_root: &Path, default_root: &Path, want: Want, dry_run: bool) -> Vec<String> {
    let label = m.label;
    let Some(shipped) = shipped_version(m, source_root) else { return Vec::new() };
    let Some(claude) = claude_bin() else {
        let verb = if want == Want::Install { "installed" } else { "checked" };
        return vec![format!("{label}: not {verb}: `claude` is not on PATH; re-run `ways manage repair --commit` once it is")];
    };
    let (cfg, _) = config_dir(dest_root, default_root);
    let have = match query(m, &claude, cfg.as_deref()) {
        Ok(h) => h,
        Err(e) => return vec![format!("{label}: ⚠ could not read Claude Code's plugins: {e}")],
    };
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    let recorded = read_record(dest_root);
    let recover = recover_command(cfg.as_deref());
    let plan = match want {
        Want::Install => plan_converge(m, &have, &dir, recorded.as_deref(), &shipped, &recover),
        Want::Withdraw => plan_withdraw(&have, &dir, recorded.as_deref(), &recover, wanted_by_other(m, dest_root)),
    };
    execute(m, &claude, cfg.as_deref(), dest_root, &plan, dry_run)
}

/// Remove `m` from a disabled target, before its links go. It runs where the
/// source ships the mod or its marketplace, or the target records that the
/// mod was installed on request, so a source that stopped shipping the mod
/// still takes it out.
pub fn withdraw(m: &Mod, source_root: &Path, dest_root: &Path, default_root: &Path, dry_run: bool) -> Vec<String> {
    let marketplace = marketplace_dir(source_root).join(".claude-plugin/marketplace.json").is_file();
    if shipped_version(m, source_root).is_none() && !marketplace && !recorded(m, dest_root) {
        return Vec::new();
    }
    let Some(claude) = claude_bin() else {
        return vec![format!("{}: ⚠ not checked in {}: `claude` is not on PATH", m.label, dest_root.display())];
    };
    let (cfg, _) = config_dir(dest_root, default_root);
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    match query(m, &claude, cfg.as_deref()) {
        Ok(have) => {
            let plan = plan_withdraw(&have, &dir, read_record(dest_root).as_deref(), &recover_command(cfg.as_deref()), false);
            execute(m, &claude, cfg.as_deref(), dest_root, &plan, dry_run)
        }
        Err(e) => vec![format!("{}: ⚠ could not read Claude Code's plugins: {e}", m.label)],
    }
}

/// The version `claude --version` prints first, as in `2.1.309 (Claude Code)`.
fn claude_code_version(out: &str) -> Option<&str> {
    out.split_whitespace().next().filter(|v| v.split('.').count() >= 2 && v.split('.').all(|p| p.parse::<u32>().is_ok()))
}

/// Whether a `claude --version` output names a Claude Code that can run `m`.
fn new_enough(m: &Mod, out: &str) -> Result<(), String> {
    match claude_code_version(out) {
        Some(v) if !super::update::version_older(v, m.min_claude_code) => Ok(()),
        Some(v) => Err(format!("Claude Code {v} is older than {}, the first whose mods can run the {}", m.min_claude_code, m.label)),
        None => Err(format!("`claude --version` printed no version ({})", out.trim())),
    }
}

/// Whether this host can run `m`: `claude` is on PATH, it is at least the
/// Claude Code whose mod API carries the mod's calls (`m.min_claude_code`),
/// and its plugin CLI answers for the target at `dest_root`. A mod's hooks
/// module also loads behind a server-side rollout flag (ADR-406, addendum of
/// 2026-10-12), which no local probe can read. The error names the host's
/// limit.
pub fn host_can_load(m: &Mod, dest_root: &Path, default_root: &Path) -> Result<(), String> {
    let claude = claude_bin().ok_or_else(|| "`claude` is not on PATH".to_string())?;
    let cfg = config_dir(dest_root, default_root).0;
    let out = claude_stdout(&claude, cfg.as_deref(), &["--version"]).map_err(|e| format!("`claude --version` failed: {e}"))?;
    new_enough(m, &out)?;
    query(m, &claude, cfg.as_deref()).map(|_| ()).map_err(|e| format!("Claude Code's plugin CLI did not answer: {e}"))
}

/// The `enabledPlugins` value Claude Code keeps for `m` in a target's
/// `settings.json`: `claude plugin install` sets it true at user scope,
/// `claude plugin disable` sets it false, and uninstalling removes it.
fn enabled_value(m: &Mod, dest_root: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(dest_root.join("settings.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("enabledPlugins")?.get(m.id)?.as_bool()
}

/// True when `m` is installed and enabled in the target at `dest_root`, read
/// from its settings with no call to Claude Code.
pub fn enabled_in(m: &Mod, dest_root: &Path) -> bool {
    enabled_value(m, dest_root) == Some(true)
}

/// True when the target's settings name `m` at all, enabled or not.
pub fn listed_in(m: &Mod, dest_root: &Path) -> bool {
    enabled_value(m, dest_root).is_some()
}

/// For `ways status`: in one target, the version sessions run, the version
/// Claude Code records as installed, and the shipped one.
///
/// A plugin from a folder marketplace runs from that folder, never from the
/// copy `claude plugin install` makes in the plugin cache (ADR-406, addendum
/// of 2026-10-12). So where the `agent-ways` marketplace is this app's
/// `plugins/` folder, sessions run the shipped version whatever the record
/// says; `runs` is `null` where that cannot be told.
pub fn status(m: &Mod, source_root: &Path, dest_root: &Path, default_root: &Path) -> serde_json::Value {
    let shipped = shipped_version(m, source_root);
    let installed = claude_bin()
        .ok_or_else(|| "`claude` is not on PATH".to_string())
        .and_then(|c| query(m, &c, config_dir(dest_root, default_root).0.as_deref()));
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    match installed {
        Ok(h) => serde_json::json!({
            "runs": runs_version(&h, &dir, shipped.as_deref()),
            "installed": h.ours,
            "shipped": shipped,
            "foreign": h.foreign,
        }),
        Err(e) => serde_json::json!({ "runs": null, "installed": null, "shipped": shipped, "error": e }),
    }
}

/// The version sessions run: the shipped one when the mod is installed from
/// this app's folder `dir`, which sessions read in place.
fn runs_version(have: &Installed, dir: &str, shipped: Option<&str>) -> Option<String> {
    (have.ours.is_some() && have.marketplace.as_deref() == Some(dir)).then(|| shipped.map(str::to_string)).flatten()
}

/// `status` as one line.
pub fn status_line(m: &Mod, v: &serde_json::Value) -> String {
    let id = m.id;
    let shipped = v["shipped"].as_str().unwrap_or("none");
    if let Some(e) = v["error"].as_str() {
        return format!("{id} unknown ({e}); shipped {shipped}");
    }
    let foreign: Vec<&str> = v["foreign"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let foreign = if foreign.is_empty() { String::new() } else { format!("; {} from another marketplace", foreign.join(", ")) };
    match (v["runs"].as_str(), v["installed"].as_str()) {
        (Some(runs), Some(i)) if runs == i => format!("{id} {runs} runs from the app's folder{foreign}"),
        (Some(runs), Some(i)) => format!("{id} {runs} runs from the app's folder (Claude Code records {i}){foreign}"),
        (_, Some(i)) => format!("{id} {i} installed, shipped {shipped}{foreign}"),
        (_, None) => format!("{id} not installed, shipped {shipped}{foreign}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/app/plugins";

    fn have(marketplace: Option<&str>, ours: Option<&str>) -> Installed {
        Installed { marketplace: marketplace.map(str::to_string), ours: ours.map(str::to_string), ..Installed::default() }
    }

    #[test]
    fn parses_what_claude_lists() {
        let markets = r#"[{"name":"claude-plugins-official","source":"github","repo":"x/y"},{"name":"agent-ways","source":"directory","path":"/app/plugins","installLocation":"/app/plugins"}]"#;
        let plugins = r#"[{"id":"attend@agent-ways","version":"0.17.0","scope":"user"},{"id":"attend@elsewhere","version":"9.0.0"},{"id":"attendant@agent-ways","version":"1.0.0"}]"#;
        let h = parse_installed(&ATTEND, markets, plugins);
        assert_eq!(h.marketplace.as_deref(), Some("/app/plugins"));
        assert_eq!(h.ours.as_deref(), Some("0.17.0"));
        assert_eq!(h.foreign, vec!["attend@elsewhere".to_string()]);
        let gh = parse_installed(&ATTEND, r#"[{"name":"agent-ways","source":"github","repo":"aaronsb/agent-ways"}]"#, "[]");
        assert_eq!(gh.marketplace.as_deref(), Some(""));
        assert_eq!(parse_installed(&ATTEND, "not json", "[]"), Installed::default());
    }

    #[test]
    fn each_mod_sees_the_other_as_a_sibling() {
        let plugins = r#"[{"id":"attend@agent-ways","version":"0.28.4"},{"id":"ways-judge@agent-ways","version":"1.50.0"},{"id":"ways-judge@mine","version":"0.1.0"},{"id":"other@elsewhere"}]"#;
        let a = parse_installed(&ATTEND, "[]", plugins);
        assert_eq!((a.ours.as_deref(), a.foreign.clone(), a.siblings.clone()), (Some("0.28.4"), vec![], vec!["ways-judge@agent-ways".to_string()]));
        let j = parse_installed(&JUDGE, "[]", plugins);
        assert_eq!(j.ours.as_deref(), Some("1.50.0"));
        assert_eq!(j.foreign, vec!["ways-judge@mine".to_string()]);
        assert_eq!(j.siblings, vec!["attend@agent-ways".to_string()]);
    }

    #[test]
    fn a_fresh_target_gets_the_marketplace_then_the_plugin() {
        let p = plan_converge(&ATTEND, &have(None, None), DIR, None, "0.17.0", "CMD");
        assert_eq!(p.calls, vec![Call::MarketplaceAdd(DIR.into()), Call::Install]);
        assert_eq!(Call::MarketplaceAdd(DIR.into()).args(&ATTEND), ["plugin", "marketplace", "add", DIR, "--scope", "user"]);
        assert_eq!(Call::Install.args(&ATTEND), ["plugin", "install", "attend@agent-ways", "--scope", "user"]);
    }

    #[test]
    fn the_judge_mod_installs_by_its_own_id_from_the_same_marketplace() {
        let p = plan_converge(&JUDGE, &have(Some(DIR), None), DIR, None, "1.50.0", "CMD");
        assert_eq!(p.calls, vec![Call::Install]);
        assert_eq!(Call::Install.args(&JUDGE), ["plugin", "install", "ways-judge@agent-ways", "--scope", "user"]);
        assert_eq!(Call::Update.args(&JUDGE), ["plugin", "update", "ways-judge@agent-ways", "--scope", "user"]);
        assert_eq!(Call::Uninstall.args(&JUDGE), ["plugin", "uninstall", "ways-judge@agent-ways", "--scope", "user"]);
        assert_eq!(Call::Install.describe(&JUDGE), "installed ways-judge@agent-ways");
    }

    #[test]
    fn current_is_nothing_older_is_an_update_newer_is_left() {
        assert_eq!(plan_converge(&ATTEND, &have(Some(DIR), Some("0.17.0")), DIR, None, "0.17.0", "CMD"), Plan::default());
        assert_eq!(plan_converge(&ATTEND, &have(Some(DIR), Some("0.16.2")), DIR, None, "0.17.0", "CMD").calls, vec![Call::MarketplaceUpdate, Call::Update]);
        assert_eq!(plan_converge(&ATTEND, &have(Some(DIR), Some("0.18.0")), DIR, None, "0.17.0", "CMD"), Plan::default());
        assert_eq!(plan_converge(&ATTEND, &have(Some(DIR), None), DIR, None, "0.17.0", "CMD").calls, vec![Call::Install]);
    }

    #[test]
    fn a_moved_app_re_adds_the_marketplace_and_reinstalls() {
        // Removing the marketplace uninstalls the plugin, whatever version.
        for v in [Some("0.17.0"), Some("0.16.0"), None] {
            let p = plan_converge(&ATTEND, &have(Some("/old/plugins"), v), DIR, Some("/old/plugins"), "0.17.0", "CMD");
            assert_eq!(p.calls, vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into()), Call::Install], "{v:?}");
        }
    }

    #[test]
    fn a_moved_app_installs_a_sibling_again_and_a_withdrawal_spares_its_marketplace() {
        let mut h = have(Some("/old/plugins"), Some("0.17.0"));
        h.siblings.push("ways-judge@agent-ways".into());
        let p = plan_converge(&ATTEND, &h, DIR, Some("/old/plugins"), "0.17.0", "CMD");
        assert_eq!(
            p.calls,
            vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into()), Call::Install, Call::InstallSibling("ways-judge@agent-ways".into())]
        );
        // attend_mod.install false while ways-judge stays: attend goes, the
        // marketplace ways-judge runs from stays.
        let mut h = have(Some(DIR), Some("0.17.0"));
        h.siblings.push("ways-judge@agent-ways".into());
        assert_eq!(plan_withdraw(&h, DIR, None, "CMD", false).calls, vec![Call::Uninstall]);
        // ways-judge is recorded as wanted but not installed now (a failed
        // install lost it): the marketplace stays for its reinstall.
        let alone = have(Some(DIR), Some("0.17.0"));
        assert_eq!(plan_withdraw(&alone, DIR, None, "CMD", true).calls, vec![Call::Uninstall]);
    }

    /// A folder that still holds a marketplace, which this app never used.
    fn checkout(tag: &str) -> String {
        let dir = std::env::temp_dir().join(format!("ways-attend-mod-checkout-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        std::fs::write(dir.join(".claude-plugin/marketplace.json"), "{}").unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn a_marketplace_from_a_folder_this_app_never_used_is_foreign() {
        let dev = checkout("foreign");
        let p = plan_converge(&ATTEND, &have(Some(&dev), Some("0.17.0")), DIR, Some("/old/plugins"), "0.18.0", "CMD");
        assert!(p.calls.is_empty(), "{p:?}");
        assert!(p.notes[0].contains(&dev), "{p:?}");
        assert!(p.notes[0].contains("run `CMD` and then `ways manage repair --commit`"), "the way out: {p:?}");
        let w = plan_withdraw(&have(Some(&dev), Some("0.17.0")), DIR, None, "CMD", false);
        assert!(w.calls.is_empty(), "its plugin is the operator's too: {w:?}");
        assert_eq!(w.notes.len(), 1);
        std::fs::remove_dir_all(&dev).ok();
    }

    #[test]
    fn a_gone_folder_with_attend_installed_is_ours_moved_without_a_record() {
        let gone = "/nowhere/agent-ways/plugins";
        let p = plan_converge(&ATTEND, &have(Some(gone), Some("0.17.0")), DIR, None, "0.17.0", "CMD");
        assert_eq!(p.calls, vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into()), Call::Install]);
        let w = plan_withdraw(&have(Some(gone), Some("0.17.0")), DIR, None, "CMD", false);
        assert_eq!(w.calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        // A checkout that is still there, only without plugins/ for now (a
        // branch switch), is not claimed: reported, with the way out.
        let dev = checkout("branch-switch");
        std::fs::remove_dir_all(&dev).unwrap();
        let p = plan_converge(&ATTEND, &have(Some(&dev), Some("0.17.0")), DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty(), "{p:?}");
        assert!(p.notes[0].contains("run `CMD`"), "{p:?}");
        let w = plan_withdraw(&have(Some(&dev), Some("0.17.0")), DIR, None, "CMD", false);
        assert!(w.calls.is_empty(), "{w:?}");
        // With no attend plugin from it, a gone folder proves nothing.
        let p = plan_converge(&ATTEND, &have(Some(gone), None), DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty() && p.notes.len() == 1, "{p:?}");
    }

    #[test]
    fn the_recovery_command_names_a_non_default_target() {
        assert_eq!(recover_command(None), "claude plugin marketplace remove agent-ways");
        assert_eq!(recover_command(Some(Path::new("/t/claude-b"))), "CLAUDE_CONFIG_DIR=/t/claude-b claude plugin marketplace remove agent-ways");
    }

    #[test]
    fn foreign_plugins_and_marketplaces_are_reported_and_left_alone() {
        let mut h = have(None, None);
        h.foreign.push("attend@mine".into());
        let p = plan_converge(&ATTEND, &h, DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty());
        assert!(p.notes[0].contains("attend@mine"), "{p:?}");
        let p = plan_converge(&ATTEND, &have(Some(""), None), DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty());
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn withdrawal_uninstalls_then_removes_the_marketplace() {
        assert_eq!(plan_withdraw(&have(Some(DIR), Some("0.17.0")), DIR, None, "CMD", false).calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        assert_eq!(plan_withdraw(&have(Some("/old/plugins"), Some("0.17.0")), DIR, Some("/old/plugins"), "CMD", false).calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        assert_eq!(plan_withdraw(&have(None, None), DIR, None, "CMD", false), Plan::default());
        let mut h = have(None, None);
        h.foreign.push("attend@mine".into());
        assert_eq!(plan_withdraw(&h, DIR, None, "CMD", false), Plan::default(), "a foreign attend stays");
    }

    #[test]
    fn enabled_plugins_in_a_targets_settings_say_whether_a_mod_is_on() {
        let dir = std::env::temp_dir().join(format!("ways-attend-mod-enabled-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!enabled_in(&JUDGE, &dir) && !listed_in(&JUDGE, &dir), "no settings.json");
        std::fs::write(dir.join("settings.json"), r#"{"enabledPlugins":{"attend@agent-ways":true,"ways-judge@agent-ways":false}}"#).unwrap();
        assert!(enabled_in(&ATTEND, &dir));
        assert!(!enabled_in(&JUDGE, &dir) && listed_in(&JUDGE, &dir), "disabled, still listed");
        std::fs::write(dir.join("settings.json"), r#"{"enabledPlugins":{"ways-judge@agent-ways":true}}"#).unwrap();
        assert!(enabled_in(&JUDGE, &dir) && !listed_in(&ATTEND, &dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_source_without_the_mod_ships_no_version() {
        let app = std::env::temp_dir().join(format!("ways-attend-mod-ships-{}", std::process::id()));
        std::fs::create_dir_all(app.join("plugins/.claude-plugin")).unwrap();
        std::fs::create_dir_all(app.join("plugins/attend/.claude-plugin")).unwrap();
        std::fs::write(app.join("plugins/.claude-plugin/marketplace.json"), "{}").unwrap();
        std::fs::write(app.join("plugins/attend/.claude-plugin/plugin.json"), r#"{"version":"0.28.4"}"#).unwrap();
        assert_eq!(shipped_version(&ATTEND, &app).as_deref(), Some("0.28.4"));
        assert_eq!(shipped_version(&JUDGE, &app), None);
        std::fs::remove_dir_all(&app).unwrap();
    }

    #[test]
    fn the_judge_mod_needs_a_claude_code_whose_mods_can_complete_and_spawn() {
        assert_eq!(claude_code_version("2.1.309 (Claude Code)\n"), Some("2.1.309"));
        assert_eq!(claude_code_version("Claude Code\n"), None);
        assert!(new_enough(&JUDGE, "2.1.304 (Claude Code)").is_ok());
        assert!(new_enough(&JUDGE, "2.1.309 (Claude Code)").is_ok());
        assert!(new_enough(&JUDGE, "2.2.0 (Claude Code)").is_ok());
        let old = new_enough(&JUDGE, "2.1.296 (Claude Code)").unwrap_err();
        assert!(old.contains("Claude Code 2.1.296 is older than 2.1.304"), "{old}");
        assert!(new_enough(&ATTEND, "2.1.296 (Claude Code)").is_ok(), "attend's floor is the modules release");
        assert!(new_enough(&JUDGE, "garbage").unwrap_err().contains("printed no version"));
    }

    #[test]
    fn a_gone_folder_with_only_the_other_mod_installed_is_ours_not_foreign() {
        // attend is not installed; ways-judge is, from a folder that is gone.
        let gone = "/nowhere/agent-ways/plugins";
        let mut h = have(Some(gone), None);
        h.siblings.push("ways-judge@agent-ways".into());
        let p = plan_converge(&ATTEND, &h, DIR, None, "0.17.0", "CMD");
        assert!(p.notes.is_empty(), "no foreign note: {p:?}");
        assert_eq!(
            p.calls,
            vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into()), Call::Install, Call::InstallSibling("ways-judge@agent-ways".into())]
        );
    }

    /// Installed from this app's folder, sessions run the folder's version
    /// whatever Claude Code's record says (review of #927, finding 1).
    #[test]
    fn the_status_line_says_what_sessions_run() {
        let ours = have(Some(DIR), Some("0.18.0"));
        assert_eq!(runs_version(&ours, DIR, Some("0.19.0")).as_deref(), Some("0.19.0"));
        assert_eq!(runs_version(&have(Some("/elsewhere"), Some("0.18.0")), DIR, Some("0.19.0")), None);
        assert_eq!(runs_version(&have(Some(DIR), None), DIR, Some("0.19.0")), None);
        let v = serde_json::json!({ "runs": "0.19.0", "installed": "0.18.0", "shipped": "0.19.0", "foreign": [] });
        assert_eq!(status_line(&ATTEND, &v), "attend@agent-ways 0.19.0 runs from the app's folder (Claude Code records 0.18.0)");
        let v = serde_json::json!({ "runs": "0.19.0", "installed": "0.19.0", "shipped": "0.19.0", "foreign": [] });
        assert_eq!(status_line(&ATTEND, &v), "attend@agent-ways 0.19.0 runs from the app's folder");
    }

    #[test]
    fn the_status_line_names_both_versions() {
        let v = serde_json::json!({ "installed": "0.16.0", "shipped": "0.17.0", "foreign": [] });
        assert_eq!(status_line(&ATTEND, &v), "attend@agent-ways 0.16.0 installed, shipped 0.17.0");
        let v = serde_json::json!({ "installed": null, "shipped": "0.17.0", "error": "`claude` is not on PATH" });
        assert!(status_line(&ATTEND, &v).contains("not on PATH"));
    }
}
