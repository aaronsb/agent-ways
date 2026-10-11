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
//! the MCP registration (see [`super::mcp_register::config_dir`]). An `attend`
//! plugin from another marketplace, and an `agent-ways` marketplace this app
//! did not add, are the operator's own: reported and left alone. A missing
//! `claude` binary or a failed call is reported and never fails a reconcile.

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

/// Install, or bring up to `shipped`, the mod from `dir`.
pub fn plan_converge(have: &Installed, dir: &str, shipped: &str) -> Plan {
    let mut plan = Plan::default();
    if !have.foreign.is_empty() {
        plan.notes.push(format!(
            "{} is installed from another marketplace; left as it is, and {PLUGIN_ID} is not installed beside it",
            have.foreign.join(", ")
        ));
        return plan;
    }
    match have.marketplace.as_deref() {
        None => plan.calls.push(Call::MarketplaceAdd(dir.to_string())),
        Some("") => {
            plan.notes.push(format!("a {MARKETPLACE} marketplace that is not a local folder is configured; left as it is"));
            return plan;
        }
        Some(path) if !super::reconcile::same_path(Path::new(path), Path::new(dir)) => {
            // A moved app: the marketplace names the old folder.
            plan.calls.push(Call::MarketplaceRemove);
            plan.calls.push(Call::MarketplaceAdd(dir.to_string()));
        }
        Some(_) => {}
    }
    match have.ours.as_deref() {
        None => plan.calls.push(Call::Install),
        Some(v) if super::update::version_older(v, shipped) => {
            if !plan.calls.iter().any(|c| matches!(c, Call::MarketplaceAdd(_))) {
                plan.calls.push(Call::MarketplaceUpdate);
            }
            plan.calls.push(Call::Update);
        }
        Some(_) => {}
    }
    plan
}

/// Remove the mod and the marketplace, where they are ours.
pub fn plan_withdraw(have: &Installed) -> Plan {
    let mut plan = Plan::default();
    if have.ours.is_some() {
        plan.calls.push(Call::Uninstall);
    }
    match have.marketplace.as_deref() {
        Some("") => plan.notes.push(format!("a {MARKETPLACE} marketplace that is not a local folder is configured; left as it is")),
        Some(_) => plan.calls.push(Call::MarketplaceRemove),
        None => {}
    }
    plan
}

/// Ask Claude Code what one config directory holds.
fn query(claude: &Path, cfg: Option<&Path>) -> Result<Installed, String> {
    let markets = claude_stdout(claude, cfg, &["plugin", "marketplace", "list", "--json"])?;
    let plugins = claude_stdout(claude, cfg, &["plugin", "list", "--json"])?;
    Ok(parse_installed(&markets, &plugins))
}

/// Run a plan's calls in order, stopping at the first failure. Returns one
/// line per call made and per note.
fn execute(claude: &Path, cfg: Option<&Path>, plan: &Plan, dry_run: bool) -> Vec<String> {
    let mut lines: Vec<String> = plan.notes.iter().map(|n| format!("attend mod: {n}")).collect();
    for call in &plan.calls {
        if dry_run {
            lines.push(format!("attend mod: would run `claude {}`", call.args().join(" ")));
            continue;
        }
        match claude_stdout(claude, cfg, &call.args()) {
            Ok(_) => lines.push(format!("attend mod: {}", call.describe())),
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
    let plan = if install {
        plan_converge(&have, &marketplace_dir(source_root).to_string_lossy(), &shipped)
    } else {
        plan_withdraw(&have)
    };
    execute(&claude, cfg.as_deref(), &plan, dry_run)
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
    match query(&claude, cfg.as_deref()) {
        Ok(have) => execute(&claude, cfg.as_deref(), &plan_withdraw(&have), dry_run),
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
        let p = plan_converge(&have(None, None), DIR, "0.17.0");
        assert_eq!(p.calls, vec![Call::MarketplaceAdd(DIR.into()), Call::Install]);
        assert_eq!(Call::MarketplaceAdd(DIR.into()).args(), ["plugin", "marketplace", "add", DIR, "--scope", "user"]);
        assert_eq!(Call::Install.args(), ["plugin", "install", "attend@agent-ways", "--scope", "user"]);
    }

    #[test]
    fn current_is_nothing_older_is_an_update_newer_is_left() {
        assert_eq!(plan_converge(&have(Some(DIR), Some("0.17.0")), DIR, "0.17.0"), Plan::default());
        assert_eq!(plan_converge(&have(Some(DIR), Some("0.16.2")), DIR, "0.17.0").calls, vec![Call::MarketplaceUpdate, Call::Update]);
        assert_eq!(plan_converge(&have(Some(DIR), Some("0.18.0")), DIR, "0.17.0"), Plan::default());
        assert_eq!(plan_converge(&have(Some(DIR), None), DIR, "0.17.0").calls, vec![Call::Install]);
    }

    #[test]
    fn a_moved_app_re_adds_the_marketplace() {
        let p = plan_converge(&have(Some("/old/plugins"), Some("0.17.0")), DIR, "0.17.0");
        assert_eq!(p.calls, vec![Call::MarketplaceRemove, Call::MarketplaceAdd(DIR.into())]);
    }

    #[test]
    fn foreign_plugins_and_marketplaces_are_reported_and_left_alone() {
        let mut h = have(None, None);
        h.foreign.push("attend@mine".into());
        let p = plan_converge(&h, DIR, "0.17.0");
        assert!(p.calls.is_empty());
        assert!(p.notes[0].contains("attend@mine"), "{p:?}");
        let p = plan_converge(&have(Some(""), None), DIR, "0.17.0");
        assert!(p.calls.is_empty());
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn withdrawal_uninstalls_then_removes_the_marketplace() {
        assert_eq!(plan_withdraw(&have(Some(DIR), Some("0.17.0"))).calls, vec![Call::Uninstall, Call::MarketplaceRemove]);
        assert_eq!(plan_withdraw(&have(None, None)), Plan::default());
        let mut h = have(None, None);
        h.foreign.push("attend@mine".into());
        assert_eq!(plan_withdraw(&h), Plan::default(), "a foreign attend stays");
    }

    #[test]
    fn the_status_line_names_both_versions() {
        let v = serde_json::json!({ "installed": "0.16.0", "shipped": "0.17.0", "foreign": [] });
        assert_eq!(status_line(&v), "attend@agent-ways 0.16.0 installed, shipped 0.17.0");
        let v = serde_json::json!({ "installed": null, "shipped": "0.17.0", "error": "`claude` is not on PATH" });
        assert!(status_line(&v).contains("not on PATH"));
    }
}
