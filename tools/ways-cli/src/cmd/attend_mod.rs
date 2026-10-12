//! Installation of the attend mod with Claude Code (ADR-406 item 8).
//!
//! The mod is a plugin in the app source, `plugins/attend/`, beside
//! `plugins/.claude-plugin/marketplace.json`, which declares the local
//! marketplace `agent-ways`. For each enabled target, after the settings
//! merge and the MCP registration, reconcile adds the marketplace and
//! installs `attend@agent-ways` at user scope through Claude Code's own
//! plugin CLI, and updates both when the installed version is older than the
//! shipped one. A disabled target, and every target once
//! `attend_mod.install` is false, has the plugin uninstalled and the
//! marketplace removed.
//!
//! Each call runs under `CLAUDE_CONFIG_DIR` by the rule ADR-501 item 9 set for
//! the MCP registration (see [`super::mcp_register::config_dir`]), and is
//! killed past [`super::mcp_register::CLAUDE_TIMEOUT`].
//!
//! The `agent-ways` marketplace is ours when its source is this app's
//! `plugins/` folder; or the folder a repair recorded adding it from, which a
//! moved app leaves behind; or, with no record, a folder whose parent (the
//! app or checkout that held `plugins/`) is gone while `attend@agent-ways` is
//! installed from it, since an app that is gone has no other owner. A folder
//! that is only missing its marketplace for now, in a checkout that is still
//! there (a branch without `plugins/`), is reported, not claimed. Any other `agent-ways` marketplace, and
//! an `attend` plugin from another marketplace, are the operator's own:
//! reported, with the command that hands the marketplace to this app, and
//! left alone. Removing a marketplace uninstalls its plugins, so a moved
//! app's marketplace is removed, added again and the plugin installed again in
//! one pass. A missing `claude` binary or a failed call is reported and never
//! fails a reconcile.

use std::path::{Path, PathBuf};

use super::mcp_register::{claude_bin, claude_stdout, config_dir};

/// The marketplace `plugins/.claude-plugin/marketplace.json` declares.
pub const MARKETPLACE: &str = "agent-ways";
/// The plugin's name in its manifest.
pub const PLUGIN: &str = "attend";
/// The plugin as Claude Code knows it once installed.
pub const PLUGIN_ID: &str = "attend@agent-ways";

/// The marketplace folder in the app source.
fn marketplace_dir(source_root: &Path) -> PathBuf {
    source_root.join("plugins")
}

/// The version the source ships, from the plugin's manifest; `None` when the
/// source has no marketplace (an app from before ADR-406), in which case
/// reconcile leaves Claude Code's plugins alone.
pub fn shipped_version(source_root: &Path) -> Option<String> {
    let dir = marketplace_dir(source_root);
    if !dir.join(".claude-plugin/marketplace.json").is_file() {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(PLUGIN).join(".claude-plugin/plugin.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")?.as_str().map(str::to_string)
}

/// What Claude Code reports for one config directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// The `agent-ways` marketplace, if one is configured: `Some(path)` for a
    /// directory source, `Some("")` for any other kind.
    pub marketplace: Option<String>,
    /// The installed version of `attend@agent-ways`.
    pub ours: Option<String>,
    /// Ids of `attend` plugins from other marketplaces.
    pub foreign: Vec<String>,
}

/// Reads `claude plugin marketplace list --json` and `claude plugin list --json`.
pub fn parse_installed(marketplaces: &str, plugins: &str) -> Installed {
    let mut out = Installed::default();
    if let Ok(serde_json::Value::Array(list)) = serde_json::from_str(marketplaces) {
        if let Some(m) = list.iter().find(|m| m.get("name").and_then(|n| n.as_str()) == Some(MARKETPLACE)) {
            let is_dir = m.get("source").and_then(|s| s.as_str()) == Some("directory");
            let path = m.get("path").and_then(|p| p.as_str()).unwrap_or_default();
            out.marketplace = Some(if is_dir { path.to_string() } else { String::new() });
        }
    }
    if let Ok(serde_json::Value::Array(list)) = serde_json::from_str(plugins) {
        for p in &list {
            let Some(id) = p.get("id").and_then(|i| i.as_str()) else { continue };
            if id == PLUGIN_ID {
                out.ours = Some(p.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string());
            } else if id.split('@').next() == Some(PLUGIN) {
                out.foreign.push(id.to_string());
            }
        }
    }
    out
}

/// One call of the plugin CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    MarketplaceAdd(String),
    MarketplaceRemove,
    MarketplaceUpdate,
    Install,
    Update,
    Uninstall,
}

impl Call {
    fn args(&self) -> Vec<&str> {
        match self {
            Call::MarketplaceAdd(dir) => vec!["plugin", "marketplace", "add", dir.as_str(), "--scope", "user"],
            Call::MarketplaceRemove => vec!["plugin", "marketplace", "remove", MARKETPLACE],
            Call::MarketplaceUpdate => vec!["plugin", "marketplace", "update", MARKETPLACE],
            Call::Install => vec!["plugin", "install", PLUGIN_ID, "--scope", "user"],
            Call::Update => vec!["plugin", "update", PLUGIN_ID, "--scope", "user"],
            Call::Uninstall => vec!["plugin", "uninstall", PLUGIN_ID, "--scope", "user"],
        }
    }

    fn describe(&self) -> String {
        match self {
            Call::MarketplaceAdd(dir) => format!("added the {MARKETPLACE} marketplace ({dir})"),
            Call::MarketplaceRemove => format!("removed the {MARKETPLACE} marketplace"),
            Call::MarketplaceUpdate => format!("refreshed the {MARKETPLACE} marketplace"),
            Call::Install => format!("installed {PLUGIN_ID}"),
            Call::Update => format!("updated {PLUGIN_ID}; running sessions keep their version until they restart"),
            Call::Uninstall => format!("uninstalled {PLUGIN_ID}"),
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
        Some(path) if have.ours.is_some() && gone(path) => Market::Moved,
        Some(path) => Market::Foreign(path.to_string()),
    }
}

/// The note for a marketplace this app will not touch. `recover` is the
/// command that removes it from this target, after which a repair adds ours.
fn foreign_note(from: &str, recover: &str) -> String {
    format!(
        "an {MARKETPLACE} marketplace from {from}, which this app did not add, is configured; it and its plugins are left as they are. \
         To hand it to this app, run `{recover}` and then `ways manage repair`"
    )
}

/// The exact command that removes the marketplace from one target.
fn recover_command(cfg: Option<&Path>) -> String {
    match cfg {
        Some(d) => format!("CLAUDE_CONFIG_DIR={} claude plugin marketplace remove {MARKETPLACE}", d.display()),
        None => format!("claude plugin marketplace remove {MARKETPLACE}"),
    }
}

/// Install, or bring up to `shipped`, the mod from `dir`. `recorded` is the
/// folder a repair last added the marketplace from.
pub fn plan_converge(have: &Installed, dir: &str, recorded: Option<&str>, shipped: &str, recover: &str) -> Plan {
    let mut plan = Plan::default();
    if !have.foreign.is_empty() {
        plan.notes.push(format!(
            "{} is installed from another marketplace; left as it is, and {PLUGIN_ID} is not installed beside it",
            have.foreign.join(", ")
        ));
        return plan;
    }
    match market(have, dir, recorded) {
        Market::Foreign(from) => plan.notes.push(foreign_note(&from, recover)),
        Market::Moved => {
            // Removing the marketplace uninstalls the plugin with it.
            plan.calls.extend([Call::MarketplaceRemove, Call::MarketplaceAdd(dir.to_string()), Call::Install]);
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

/// Remove the mod and the marketplace, where they are ours. Under a
/// marketplace that is not ours, `attend@agent-ways` is the operator's too.
pub fn plan_withdraw(have: &Installed, dir: &str, recorded: Option<&str>, recover: &str) -> Plan {
    let mut plan = Plan::default();
    match market(have, dir, recorded) {
        Market::Foreign(from) => plan.notes.push(foreign_note(&from, recover)),
        Market::Current | Market::Moved => {
            if have.ours.is_some() {
                plan.calls.push(Call::Uninstall);
            }
            plan.calls.push(Call::MarketplaceRemove);
        }
        Market::Absent if have.ours.is_some() => plan.calls.push(Call::Uninstall),
        Market::Absent => {}
    }
    plan
}

/// Ask Claude Code what one config directory holds.
fn query(claude: &Path, cfg: Option<&Path>) -> Result<Installed, String> {
    let markets = claude_stdout(claude, cfg, &["plugin", "marketplace", "list", "--json"])?;
    let plugins = claude_stdout(claude, cfg, &["plugin", "list", "--json"])?;
    Ok(parse_installed(&markets, &plugins))
}

/// Where a target records the folder a repair added the marketplace from,
/// beside its settings merge base.
fn record_path(dest_root: &Path) -> PathBuf {
    super::reconcile::base_path_for(&crate::paths::state_root(), dest_root).with_file_name("attend-mod-marketplace")
}

fn read_record(dest_root: &Path) -> Option<String> {
    std::fs::read_to_string(record_path(dest_root)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Run a plan's calls in order, stopping at the first failure, and keep the
/// record of where the marketplace was added from. Returns one line per call
/// made and per note.
fn execute(claude: &Path, cfg: Option<&Path>, dest_root: &Path, plan: &Plan, dry_run: bool) -> Vec<String> {
    let mut lines: Vec<String> = plan.notes.iter().map(|n| format!("attend mod: {n}")).collect();
    for call in &plan.calls {
        if dry_run {
            lines.push(format!("attend mod: would run `claude {}`", call.args().join(" ")));
            continue;
        }
        match claude_stdout(claude, cfg, &call.args()) {
            Ok(_) => {
                lines.push(format!("attend mod: {}", call.describe()));
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
                    lines.push(format!("attend mod: ⚠ could not record the marketplace folder in {}: {e}", record.display()));
                }
            }
            Err(e) => {
                lines.push(format!("attend mod: ⚠ `claude {}` failed: {e}", call.args().join(" ")));
                break;
            }
        }
    }
    lines
}

/// Converge one enabled target: install, update, or (with `install` false)
/// withdraw. Returns the lines to report; empty when nothing changed.
pub fn converge(source_root: &Path, dest_root: &Path, default_root: &Path, install: bool, dry_run: bool) -> Vec<String> {
    let Some(shipped) = shipped_version(source_root) else { return Vec::new() };
    let Some(claude) = claude_bin() else {
        return vec![format!(
            "attend mod: not {}: `claude` is not on PATH; re-run `ways manage repair` once it is",
            if install { "installed" } else { "checked" }
        )];
    };
    let (cfg, _) = config_dir(dest_root, default_root);
    let have = match query(&claude, cfg.as_deref()) {
        Ok(h) => h,
        Err(e) => return vec![format!("attend mod: ⚠ could not read Claude Code's plugins: {e}")],
    };
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    let recorded = read_record(dest_root);
    let recover = recover_command(cfg.as_deref());
    let plan = if install {
        plan_converge(&have, &dir, recorded.as_deref(), &shipped, &recover)
    } else {
        plan_withdraw(&have, &dir, recorded.as_deref(), &recover)
    };
    execute(&claude, cfg.as_deref(), dest_root, &plan, dry_run)
}

/// Remove the mod from a disabled target, before its links go.
pub fn withdraw(source_root: &Path, dest_root: &Path, default_root: &Path, dry_run: bool) -> Vec<String> {
    if shipped_version(source_root).is_none() {
        return Vec::new();
    }
    let Some(claude) = claude_bin() else {
        return vec![format!("attend mod: ⚠ not checked in {}: `claude` is not on PATH", dest_root.display())];
    };
    let (cfg, _) = config_dir(dest_root, default_root);
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    match query(&claude, cfg.as_deref()) {
        Ok(have) => {
            let plan = plan_withdraw(&have, &dir, read_record(dest_root).as_deref(), &recover_command(cfg.as_deref()));
            execute(&claude, cfg.as_deref(), dest_root, &plan, dry_run)
        }
        Err(e) => vec![format!("attend mod: ⚠ could not read Claude Code's plugins: {e}")],
    }
}

/// For `ways status`: the installed and the shipped version in one target.
pub fn status(source_root: &Path, dest_root: &Path, default_root: &Path) -> serde_json::Value {
    let shipped = shipped_version(source_root);
    let installed = claude_bin()
        .ok_or_else(|| "`claude` is not on PATH".to_string())
        .and_then(|c| query(&c, config_dir(dest_root, default_root).0.as_deref()));
    match installed {
        Ok(h) => serde_json::json!({
            "installed": h.ours,
            "shipped": shipped,
            "foreign": h.foreign,
        }),
        Err(e) => serde_json::json!({ "installed": null, "shipped": shipped, "error": e }),
    }
}

/// `status` as one line.
pub fn status_line(v: &serde_json::Value) -> String {
    let shipped = v["shipped"].as_str().unwrap_or("none");
    if let Some(e) = v["error"].as_str() {
        return format!("{PLUGIN_ID} unknown ({e}); shipped {shipped}");
    }
    let foreign: Vec<&str> = v["foreign"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let installed = match v["installed"].as_str() {
        Some(i) => format!("{PLUGIN_ID} {i} installed"),
        None => format!("{PLUGIN_ID} not installed"),
    };
    let foreign = if foreign.is_empty() { String::new() } else { format!("; {} from another marketplace", foreign.join(", ")) };
    format!("{installed}, shipped {shipped}{foreign}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/app/plugins";

    fn have(marketplace: Option<&str>, ours: Option<&str>) -> Installed {
        Installed { marketplace: marketplace.map(str::to_string), ours: ours.map(str::to_string), foreign: Vec::new() }
    }

    #[test]
    fn parses_what_claude_lists() {
        let markets = r#"[{"name":"claude-plugins-official","source":"github","repo":"x/y"},{"name":"agent-ways","source":"directory","path":"/app/plugins","installLocation":"/app/plugins"}]"#;
        let plugins = r#"[{"id":"attend@agent-ways","version":"0.17.0","scope":"user"},{"id":"attend@elsewhere","version":"9.0.0"},{"id":"attendant@agent-ways","version":"1.0.0"}]"#;
        let h = parse_installed(markets, plugins);
        assert_eq!(h.marketplace.as_deref(), Some("/app/plugins"));
        assert_eq!(h.ours.as_deref(), Some("0.17.0"));
        assert_eq!(h.foreign, vec!["attend@elsewhere".to_string()]);
        let gh = parse_installed(r#"[{"name":"agent-ways","source":"github","repo":"aaronsb/agent-ways"}]"#, "[]");
        assert_eq!(gh.marketplace.as_deref(), Some(""));
        assert_eq!(parse_installed("not json", "[]"), Installed::default());
    }

    #[test]
    fn a_fresh_target_gets_the_marketplace_then_the_plugin() {
        let p = plan_converge(&have(None, None), DIR, None, "0.17.0", "CMD");
        assert_eq!(p.calls, vec![Call::MarketplaceAdd(DIR.into()), Call::Install]);
        assert_eq!(Call::MarketplaceAdd(DIR.into()).args(), ["plugin", "marketplace", "add", DIR, "--scope", "user"]);
        assert_eq!(Call::Install.args(), ["plugin", "install", "attend@agent-ways", "--scope", "user"]);
    }

    #[test]
    fn current_is_nothing_older_is_an_update_newer_is_left() {
        assert_eq!(plan_converge(&have(Some(DIR), Some("0.17.0")), DIR, None, "0.17.0", "CMD"), Plan::default());
        assert_eq!(plan_converge(&have(Some(DIR), Some("0.16.2")), DIR, None, "0.17.0", "CMD").calls, vec![Call::MarketplaceUpdate, Call::Update]);
        assert_eq!(plan_converge(&have(Some(DIR), Some("0.18.0")), DIR, None, "0.17.0", "CMD"), Plan::default());
        assert_eq!(plan_converge(&have(Some(DIR), None), DIR, None, "0.17.0", "CMD").calls, vec![Call::Install]);
    }

    #[test]
    fn a_moved_app_re_adds_the_marketplace_and_reinstalls() {
        // Removing the marketplace uninstalls the plugin, whatever version.
        for v in [Some("0.17.0"), Some("0.16.0"), None] {
            let p = plan_converge(&have(Some("/old/plugins"), v), DIR, Some("/old/plugins"), "0.17.0", "CMD");
            assert_eq!(p.calls, vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into()), Call::Install], "{v:?}");
        }
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
        let p = plan_converge(&have(Some(&dev), Some("0.17.0")), DIR, Some("/old/plugins"), "0.18.0", "CMD");
        assert!(p.calls.is_empty(), "{p:?}");
        assert!(p.notes[0].contains(&dev), "{p:?}");
        assert!(p.notes[0].contains("run `CMD` and then `ways manage repair`"), "the way out: {p:?}");
        let w = plan_withdraw(&have(Some(&dev), Some("0.17.0")), DIR, None, "CMD");
        assert!(w.calls.is_empty(), "its plugin is the operator's too: {w:?}");
        assert_eq!(w.notes.len(), 1);
        std::fs::remove_dir_all(&dev).ok();
    }

    #[test]
    fn a_gone_folder_with_attend_installed_is_ours_moved_without_a_record() {
        let gone = "/nowhere/agent-ways/plugins";
        let p = plan_converge(&have(Some(gone), Some("0.17.0")), DIR, None, "0.17.0", "CMD");
        assert_eq!(p.calls, vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into()), Call::Install]);
        let w = plan_withdraw(&have(Some(gone), Some("0.17.0")), DIR, None, "CMD");
        assert_eq!(w.calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        // A checkout that is still there, only without plugins/ for now (a
        // branch switch), is not claimed: reported, with the way out.
        let dev = checkout("branch-switch");
        std::fs::remove_dir_all(&dev).unwrap();
        let p = plan_converge(&have(Some(&dev), Some("0.17.0")), DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty(), "{p:?}");
        assert!(p.notes[0].contains("run `CMD`"), "{p:?}");
        let w = plan_withdraw(&have(Some(&dev), Some("0.17.0")), DIR, None, "CMD");
        assert!(w.calls.is_empty(), "{w:?}");
        // With no attend plugin from it, a gone folder proves nothing.
        let p = plan_converge(&have(Some(gone), None), DIR, None, "0.17.0", "CMD");
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
        let p = plan_converge(&h, DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty());
        assert!(p.notes[0].contains("attend@mine"), "{p:?}");
        let p = plan_converge(&have(Some(""), None), DIR, None, "0.17.0", "CMD");
        assert!(p.calls.is_empty());
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn withdrawal_uninstalls_then_removes_the_marketplace() {
        assert_eq!(plan_withdraw(&have(Some(DIR), Some("0.17.0")), DIR, None, "CMD").calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        assert_eq!(plan_withdraw(&have(Some("/old/plugins"), Some("0.17.0")), DIR, Some("/old/plugins"), "CMD").calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        assert_eq!(plan_withdraw(&have(None, None), DIR, None, "CMD"), Plan::default());
        let mut h = have(None, None);
        h.foreign.push("attend@mine".into());
        assert_eq!(plan_withdraw(&h, DIR, None, "CMD"), Plan::default(), "a foreign attend stays");
    }

    #[test]
    fn the_status_line_names_both_versions() {
        let v = serde_json::json!({ "installed": "0.16.0", "shipped": "0.17.0", "foreign": [] });
        assert_eq!(status_line(&v), "attend@agent-ways 0.16.0 installed, shipped 0.17.0");
        let v = serde_json::json!({ "installed": null, "shipped": "0.17.0", "error": "`claude` is not on PATH" });
        assert!(status_line(&v).contains("not on PATH"));
    }
}
