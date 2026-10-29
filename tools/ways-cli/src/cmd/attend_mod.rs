//! Installation of the mods agent-ways ships with Claude Code: the attend mod
//! (ADR-406 item 8) and the ways-judge mod (ADR-708 §9), each kept to its
//! wanted state, `mods.<name>.install` (ADR-512 item 3).
//!
//! Each mod is a plugin in the app source, `plugins/<name>/`, beside
//! `plugins/.claude-plugin/marketplace.json`, which declares the local
//! marketplace `agent-ways` they share. For each enabled target, after the
//! settings merge and the MCP registration, repair resolves each mod's
//! `install` from the user config and the target's config. A wanted mod is
//! installed at user scope through Claude Code's own plugin CLI, and updated
//! when the installed version is older than the shipped one; one that is not
//! wanted is uninstalled, and the marketplace goes with the last of them. A
//! wanted mod is installed only where `claude --version` is at least the
//! mod's floor; below it the mod is pending, reported with the floor, and
//! tried again at every repair. A disabled target has every mod uninstalled.
//!
//! Repair keeps a record per target, beside its settings merge base: the
//! folder it added the marketplace from, and the mods it installed there and
//! has not removed since. A wanted mod that is missing while the record lists
//! it was removed by hand: repair does not put it back, and writes
//! `mods.<name>.install: false` in that target's config instead, so the
//! removal shows as a setting. A mod of ours found installed from this app's
//! marketplace joins the record, so a lost record heals at the next repair.
//!
//! Each call runs under `CLAUDE_CONFIG_DIR` by the rule ADR-501 item 9 set for
//! the MCP registration (see [`super::mcp_register::config_dir`]), and is
//! killed past [`super::mcp_register::CLAUDE_TIMEOUT`].
//!
//! The `agent-ways` marketplace is ours when its source is this app's
//! `plugins/` folder; or the folder the record names, which a moved app
//! leaves behind; or, with no record, a folder whose parent (the app or
//! checkout that held `plugins/`) is gone while a mod is installed from it,
//! since an app that is gone has no other owner. A folder that is only
//! missing its marketplace for now, in a checkout that is still there (a
//! branch without `plugins/`), is reported, not claimed. Any other
//! `agent-ways` marketplace, and a plugin of the mod's name from another
//! marketplace, whether Claude Code lists it or the target's settings.json
//! enables it (ADR-517 item 8), are the operator's own: reported, with the command that hands
//! the marketplace to this app, and left alone. Removing a marketplace
//! uninstalls its plugins, so a moved app's marketplace is removed, added
//! again and every mod of ours that was installed from it installed again in
//! one pass; and a mod withdrawn while another of ours stays wanted leaves
//! the marketplace in place. A missing `claude` binary or a failed call is
//! reported and never fails a repair.

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
    /// The oldest Claude Code checked to carry every call the mod makes.
    pub min_claude_code: &'static str,
    /// The mod missing, disabled or pending in a target leaves agent-ways
    /// degraded there, which the settings screens and `ways status` flag.
    pub critical: bool,
}

impl Mod {
    /// Whether the mod counts as critical now: ways-judge only while
    /// `gate.engine` is `claude-code` (`claude_code`), the one engine it
    /// hosts.
    pub fn critical_under(&self, claude_code: bool) -> bool {
        self.critical && (*self != JUDGE || claude_code)
    }
}

/// attend's agent-facing driver (ADR-406): function-hook modules, which
/// Claude Code 2.1.296 offers. Wanted by default.
pub const ATTEND: Mod = Mod { name: "attend", id: "attend@agent-ways", label: "attend mod", min_claude_code: "2.1.296", critical: true };
/// The judge host for the `claude-code` engine (ADR-708 §3). Beside the
/// modules it calls `$.model.complete` and `$.process.spawn`; its floor is
/// the oldest Claude Code checked to carry both (2.1.304's bundled mod
/// types). An older release may carry them too, unchecked. Wanted while
/// `gate.engine` is `claude-code` (ADR-512 item 4).
pub const JUDGE: Mod = Mod { name: "ways-judge", id: "ways-judge@agent-ways", label: "ways-judge mod", min_claude_code: "2.1.304", critical: true };

/// Claude Code follows the active agent-ways theme (ADR-514). The mod calls
/// `ui.render` on AssistantMessage, ToolUse, ToolGroup and TurnDuration,
/// `$.ui.invalidate`, `$.config.list`, the `config.set` hook,
/// `$.process.run` with `timeoutMs`, `$.fs` and `$.env`. Its floor is
/// 2.1.309, the release the mod was seen running on, whose engine wrote the
/// mod's type declarations carrying every one of them; nothing older is
/// claimed. As for every mod, the floor is checked when the mod is
/// installed, so a Claude Code downgraded afterwards is not caught.
pub const THEME: Mod = Mod { name: "ways-theme", id: "ways-theme@agent-ways", label: "ways-theme mod", min_claude_code: "2.1.309", critical: false };

/// Draws pictures inline in the terminal transcript (ADR-517). Its floor is
/// 2.1.304, whose engine wrote the mod's type declarations carrying every
/// call it makes (`Image`, `Raster`, `$.ui.blit`, `onScreen`), the release it
/// was verified on. Not critical: agent-ways loses nothing without it.
pub const IMAGES: Mod = Mod { name: "images", id: "images@agent-ways", label: "images mod", min_claude_code: "2.1.304", critical: false };

/// Every mod the marketplace ships, in its order. Each has an entry under
/// `mods` in the settings registry (ADR-512 item 1).
pub const MODS: [Mod; 4] = [ATTEND, JUDGE, THEME, IMAGES];

/// The marketplace folder in the app source.
pub fn marketplace_dir(source_root: &Path) -> PathBuf {
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
    /// The ids in `foreign` that only the target's `settings.json` enables,
    /// and that file. Claude Code lists none of them.
    pub settings_only: Vec<String>,
    pub settings_file: Option<PathBuf>,
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

/// Adds to `have.foreign` each plugin of `m`'s name from another marketplace
/// that the `settings.json` of the target at `dest_root` enables. A plugin
/// enabled from a marketplace declared in settings (`extraKnownMarketplaces`)
/// is missing from `claude plugin list` until a session has recorded that
/// marketplace, and ours installed beside it would draw everything twice.
pub fn add_enabled_foreign(m: &Mod, have: &mut Installed, dest_root: &Path) {
    let file = dest_root.join("settings.json");
    let Ok(text) = std::fs::read_to_string(&file) else { return };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let Some(enabled) = json.get("enabledPlugins").and_then(|e| e.as_object()) else { return };
    for (id, on) in enabled {
        let name = id.split_once('@').map_or(id.as_str(), |(n, _)| n);
        if on.as_bool() == Some(true) && name == m.name && id != m.id && !have.foreign.contains(id) {
            have.foreign.push(id.clone());
            have.settings_only.push(id.clone());
            have.settings_file = Some(file.clone());
        }
    }
}

/// How a repair note and a check line name a plugin that only `settings.json`
/// enables: uninstalling it does nothing, the `enabledPlugins` entry goes.
pub fn settings_only_words(id: &str, file: Option<&Path>) -> String {
    let file = file.map_or("settings.json".to_string(), |f| f.display().to_string());
    format!("{id} is enabled in {file} (enabledPlugins) from a marketplace Claude Code has not recorded")
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

/// The exact command that enables `m` in the target whose `CLAUDE_CONFIG_DIR`
/// is `cfg` (`None` for the default target).
fn enable_command_in(m: &Mod, cfg: Option<&Path>) -> String {
    match cfg {
        Some(d) => format!("CLAUDE_CONFIG_DIR={} claude plugin enable {}", d.display(), m.id),
        None => format!("claude plugin enable {}", m.id),
    }
}

/// The command that enables `m` in the target at `dest_root`.
pub fn enable_command(m: &Mod, dest_root: &Path, default_root: &Path) -> String {
    enable_command_in(m, config_dir(dest_root, default_root).0.as_deref())
}

/// Install `m`, or bring it up to `shipped`, from `dir`. `recorded` is the
/// folder a repair last added the marketplace from.
pub fn plan_converge(m: &Mod, have: &Installed, dir: &str, recorded: Option<&str>, shipped: &str, recover: &str) -> Plan {
    let mut plan = Plan::default();
    if !have.foreign.is_empty() {
        let (only, listed): (Vec<_>, Vec<_>) = have.foreign.iter().partition(|f| have.settings_only.contains(f));
        if !listed.is_empty() {
            let ids: Vec<&str> = listed.iter().map(|s| s.as_str()).collect();
            plan.notes.push(format!(
                "{} is installed from another marketplace; left as it is, and {} is not installed beside it",
                ids.join(", "),
                m.id
            ));
        }
        for id in only {
            plan.notes.push(format!(
                "{}; left as it is, and {} is not installed beside it. Fix: remove that enabledPlugins entry, then `ways manage mod repair`",
                settings_only_words(id, have.settings_file.as_deref()),
                m.id
            ));
        }
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

/// What one config directory lists: `claude plugin marketplace list --json`
/// and `claude plugin list --json`, read once for every mod.
struct Lists {
    markets: String,
    plugins: String,
}

/// Both lists, each a JSON array. Anything else (a notice printed before
/// the JSON, a changed format) is an error: read as empty, it would make
/// every mod look removed by hand.
fn query_lists(claude: &Path, cfg: Option<&Path>) -> Result<Lists, String> {
    let markets = claude_stdout(claude, cfg, &["plugin", "marketplace", "list", "--json"])?;
    let plugins = claude_stdout(claude, cfg, &["plugin", "list", "--json"])?;
    for (out, what) in [(&markets, "plugin marketplace list --json"), (&plugins, "plugin list --json")] {
        if !matches!(serde_json::from_str(out), Ok(serde_json::Value::Array(_))) {
            let first = out.lines().next().unwrap_or_default().trim();
            return Err(format!("`claude {what}` printed no JSON list (it began {first:?})"));
        }
    }
    Ok(Lists { markets, plugins })
}

/// Ask Claude Code what one config directory holds, as `m` sees it.
fn query(m: &Mod, claude: &Path, cfg: Option<&Path>) -> Result<Installed, String> {
    query_lists(claude, cfg).map(|l| parse_installed(m, &l.markets, &l.plugins))
}

/// Repair's record for one target (ADR-512 item 3), in the
/// `attend-mod-marketplace` file beside the target's settings merge base: on
/// its first line the folder a repair last added the marketplace from, then
/// one `installed <id>` line per mod a repair installed there and has not
/// removed since. The record says what repair did; what is wanted lives only
/// in settings. A file with no `installed` line reads as an empty list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub folder: Option<String>,
    pub installed: Vec<String>,
}

const INSTALLED: &str = "installed ";

impl Record {
    fn parse(text: &str) -> Record {
        let mut r = Record::default();
        for (i, line) in text.lines().map(str::trim).enumerate() {
            match line.strip_prefix(INSTALLED) {
                Some(id) => r.add(id.trim()),
                None if i == 0 && !line.is_empty() => r.folder = Some(line.to_string()),
                None => {}
            }
        }
        r
    }

    fn render(&self) -> String {
        let mut text = format!("{}\n", self.folder.as_deref().unwrap_or_default());
        for id in &self.installed {
            text.push_str(&format!("{INSTALLED}{id}\n"));
        }
        text
    }

    fn path(dest_root: &Path) -> PathBuf {
        super::reconcile::base_path_for(&crate::paths::state_root(), dest_root).with_file_name("attend-mod-marketplace")
    }

    /// Delete the `ways-judge-installed` file a release before ADR-512 kept
    /// beside the record; nothing reads it.
    fn forget_old(dest_root: &Path) {
        let _ = std::fs::remove_file(Record::path(dest_root).with_file_name("ways-judge-installed"));
    }

    /// The record of the target at `dest_root`; empty where there is none.
    pub fn read(dest_root: &Path) -> Record {
        std::fs::read_to_string(Record::path(dest_root)).map(|t| Record::parse(&t)).unwrap_or_default()
    }

    /// Change the record under its lock: a repair the gate screen started in
    /// the background and an update running beside it never lose each
    /// other's change.
    fn update(dest_root: &Path, change: impl FnOnce(&mut Record)) -> std::io::Result<()> {
        Record::update_within(dest_root, None, change)
    }

    /// [`Record::update`], waiting at most `wait` for the lock when given:
    /// a writer on a screen's thread must not hang behind another holder.
    fn update_within(dest_root: &Path, wait: Option<std::time::Duration>, change: impl FnOnce(&mut Record)) -> std::io::Result<()> {
        use agent_settings::writer::Lock;
        let path = Record::path(dest_root);
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        let _lock = match wait {
            None => Lock::acquire(&path)?,
            Some(limit) => Lock::acquire_within(&path, limit)?
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("another writer holds {}", path.display())))?,
        };
        let mut record = Record::read(dest_root);
        change(&mut record);
        record.write(dest_root)
    }

    /// Keep the record; an empty one removes the file.
    fn write(&self, dest_root: &Path) -> std::io::Result<()> {
        let path = Record::path(dest_root);
        if self.folder.is_none() && self.installed.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(&path, self.render())
    }

    pub fn has(&self, id: &str) -> bool {
        self.installed.iter().any(|i| i == id)
    }

    fn add(&mut self, id: &str) {
        if !self.has(id) {
            self.installed.push(id.to_string());
        }
    }

    fn remove(&mut self, id: &str) {
        self.installed.retain(|i| i != id);
    }
}

/// Forget that repair installed `m` in the target at `dest_root`: an explicit
/// want (`ways manage mod install`, the claude-code engine) that follows a
/// removal by hand no repair has seen yet installs the mod again, rather
/// than writing the removal back as `install: false`.
pub fn forget(dest_root: &Path, m: &Mod) -> std::io::Result<()> {
    forget_within(dest_root, m, None)
}

/// [`forget`], waiting at most `wait` for the record's lock when given.
pub fn forget_within(dest_root: &Path, m: &Mod, wait: Option<std::time::Duration>) -> std::io::Result<()> {
    if !Record::read(dest_root).has(m.id) {
        return Ok(());
    }
    Record::update_within(dest_root, wait, |r| r.remove(m.id))
}

/// The mods of ours `have` reports installed: the one being converged and
/// its siblings. A marketplace removal takes exactly these.
fn installed_mods(m: &Mod, have: &Installed) -> Vec<Mod> {
    MODS
        .into_iter()
        .filter(|o| if o.id == m.id { have.ours.is_some() } else { have.siblings.iter().any(|s| s == o.id) })
        .collect()
}

/// Run a plan's calls in order and keep the target's record. A failed
/// marketplace call stops the plan, since every later call needs the
/// marketplace; a failed plugin call is reported and the rest still run, so
/// one mod's failure never costs the other its install. An install that
/// succeeds adds its mod to the record, and a failed one adds nothing; an
/// uninstall removes it, and a marketplace removal removes the mods `have`
/// reports installed, which it takes with it. Returns one line per call made
/// and per note.
fn execute(m: &Mod, claude: &Path, cfg: Option<&Path>, dest_root: &Path, plan: &Plan, have: &Installed, dry_run: bool) -> Vec<String> {
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
                let kept = Record::update(dest_root, |record| match call {
                    Call::MarketplaceAdd(dir) => record.folder = Some(dir.clone()),
                    Call::MarketplaceRemove => {
                        record.folder = None;
                        installed_mods(m, have).iter().for_each(|o| record.remove(o.id));
                    }
                    Call::Install => record.add(m.id),
                    Call::InstallSibling(id) => record.add(id),
                    Call::Uninstall => record.remove(m.id),
                    Call::MarketplaceUpdate | Call::Update => {}
                });
                if let Err(e) = kept {
                    lines.push(format!("{label}: ⚠ could not keep repair's record in {}: {e}", Record::path(dest_root).display()));
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

/// One enabled target as repair converges it: its Claude Code config
/// directory, the default projection root that decides whether a call names
/// it in `CLAUDE_CONFIG_DIR`, and its ways config, where `install` is
/// resolved over the user config and a removal by hand is written.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub dest_root: &'a Path,
    pub default_root: &'a Path,
    pub config: &'a Path,
}

impl Target<'_> {
    /// Whether `m` is wanted here: `mods.<name>.install`, the target's
    /// config over the user config ([`wants_in`]).
    pub fn wants(&self, m: &Mod) -> bool {
        wants_in(m, self.config)
    }
}

/// Whether `m` is wanted in the target whose ways config is `config`:
/// `mods.<name>.install`, the target's config over the user config. An
/// install from before ADR-512 may hold `gate.engine: claude-code` and no
/// `mods.ways-judge.install` at all; that reads as the engine wanting the mod,
/// and [`align_judge`] writes it down at the next repair that applies.
pub fn wants_in(m: &Mod, config: &Path) -> bool {
    use ways_core::settings::{mod_install, mod_install_in_file, mod_install_in_user};
    if *m == JUDGE && mod_install_in_user(m.name).is_none() && mod_install_in_file(m.name, config).is_none() {
        return super::settings::engine::claude_code_chosen();
    }
    mod_install(m.name, config)
}

/// The line for a wanted mod the host cannot run yet.
fn pending_line(m: &Mod, why: &str) -> String {
    format!(
        "{}: ⚠ pending: {why}; upgrade Claude Code to {} or later, then run `ways manage update --commit`",
        m.label, m.min_claude_code
    )
}

/// A wanted mod repair installed in this target, now missing: the operator
/// removed it with Claude Code's own CLI. Repair writes
/// `mods.<name>.install: false` in the target's config and drops the mod from
/// the record. A write that fails keeps the record, so the next repair tries
/// again.
fn removed_by_hand(m: &Mod, t: &Target, dry_run: bool) -> String {
    let (label, name, config) = (m.label, m.name, t.config.display());
    let back = format!("`ways manage mod install {name} --commit` installs it again");
    if dry_run {
        return format!("{label}: would write mods.{name}.install: false in {config}: it was removed by hand; {back}");
    }
    if let Err(e) = super::settings::write_mod_install(t.config, name, Some(false)) {
        return format!("{label}: ⚠ removed by hand, and mods.{name}.install: false could not be written in {config}: {e}");
    }
    let kept = Record::update(t.dest_root, |r| r.remove(m.id)).err().map(|e| format!("; ⚠ the record could not be updated: {e}")).unwrap_or_default();
    format!("{label}: removed by hand, so repair wrote mods.{name}.install: false in {config} and leaves it out of this target; {back}{kept}")
}

/// Converge the mods the source ships (or only `only`) in one enabled
/// target, each to its wanted state (ADR-512 item 3). Returns the lines to
/// report; empty when nothing changed.
pub fn converge_target(source_root: &Path, t: &Target, only: Option<&Mod>, dry_run: bool) -> Vec<String> {
    let mods: Vec<Mod> =
        MODS.into_iter().filter(|m| only.is_none_or(|o| o.id == m.id) && shipped_version(m, source_root).is_some()).collect();
    if !dry_run {
        Record::forget_old(t.dest_root);
    }
    if mods.is_empty() {
        return Vec::new();
    }
    let Some(claude) = claude_bin() else {
        return mods
            .iter()
            .filter(|m| t.wants(m) || listed_in(m, t.dest_root))
            .map(|m| format!("{}: not converged: `claude` is not on PATH; re-run `ways manage repair --commit` once it is", m.label))
            .collect();
    };
    let (cfg, _) = config_dir(t.dest_root, t.default_root);
    let cfg = cfg.as_deref();
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    let recover = recover_command(cfg);
    let mut version: Option<Result<String, String>> = None;
    let mut lists: Option<Lists> = None;
    let mut lines = Vec::new();
    for m in &mods {
        let shipped = shipped_version(m, source_root).unwrap_or_default();
        let now = match lists.take().map(Ok).unwrap_or_else(|| query_lists(&claude, cfg)) {
            Ok(l) => l,
            Err(e) => {
                lines.push(format!("{}: ⚠ could not read Claude Code's plugins: {e}", m.label));
                break;
            }
        };
        let mut have = parse_installed(m, &now.markets, &now.plugins);
        add_enabled_foreign(m, &mut have, t.dest_root);
        let record = Record::read(t.dest_root);
        let whose = market(&have, &dir, record.folder.as_deref());
        // A mod of ours installed from this app's marketplace is one repair
        // keeps, whoever installed it: a lost record heals here. One the
        // record lists that is gone while not wanted leaves the list, so a
        // later want does not read it as removed by hand.
        let heal = match (have.ours.is_some(), record.has(m.id)) {
            (true, false) if whose == Market::Current => Some(true),
            (false, true) if !t.wants(m) => Some(false),
            _ => None,
        };
        if let (Some(add), false) = (heal, dry_run) {
            if let Err(e) = Record::update(t.dest_root, |r| if add { r.add(m.id) } else { r.remove(m.id) }) {
                lines.push(format!("{}: ⚠ could not keep repair's record in {}: {e}", m.label, Record::path(t.dest_root).display()));
            }
        }
        let missing = have.ours.is_none() && have.foreign.is_empty() && !matches!(whose, Market::Foreign(_));
        let plan = if !t.wants(m) {
            // Another mod wanted here, and shipped, keeps the marketplace for itself.
            let other = MODS.iter().any(|o| o.id != m.id && t.wants(o) && shipped_version(o, source_root).is_some());
            plan_withdraw(&have, &dir, record.folder.as_deref(), &recover, other)
        } else if missing && record.has(m.id) {
            lines.push(removed_by_hand(m, t, dry_run));
            lists = Some(now);
            continue;
        } else if missing {
            let out = version.get_or_insert_with(|| claude_stdout(&claude, cfg, &["--version"])).clone();
            if let Err(why) = out.map_err(|e| format!("`claude --version` failed: {e}")).and_then(|o| new_enough(m, &o)) {
                lines.push(pending_line(m, &why));
                lists = Some(now);
                continue;
            }
            plan_converge(m, &have, &dir, record.folder.as_deref(), &shipped, &recover)
        } else {
            plan_converge(m, &have, &dir, record.folder.as_deref(), &shipped, &recover)
        };
        let changes = !plan.calls.is_empty() && !dry_run;
        lines.extend(execute(m, &claude, cfg, t.dest_root, &plan, &have, dry_run));
        lists = (!changes).then_some(now);
    }
    lines
}

/// An install from before ADR-512 may hold `gate.engine: claude-code` with
/// no `mods.ways-judge.install` in the user config, which a repair would read
/// as not wanted. Repair writes the `true` the engine choice writes (ADR-512
/// item 4), once, before it converges. Returns the line to report.
pub fn align_judge(dry_run: bool) -> Vec<String> {
    if ways_core::settings::mod_install_in_user(JUDGE.name).is_some() || !super::settings::engine::claude_code_chosen() {
        return Vec::new();
    }
    let user = ways_core::paths::user_config();
    let what = format!("mods.ways-judge.install: true in {}, which gate.engine: claude-code needs", user.display());
    if dry_run {
        return vec![format!("{}: would write {what}", JUDGE.label)];
    }
    vec![match super::settings::write_mod_install(&user, JUDGE.name, Some(true)) {
        Ok(_) => format!("{}: wrote {what}", JUDGE.label),
        Err(e) => format!("{}: ⚠ could not write {what}: {e}", JUDGE.label),
    }]
}

/// Remove every mod from a disabled target, before its links go (ADR-184
/// item 4), whatever its setting, and empty the record's list; the settings
/// stay, so enabling the target again installs what is wanted. The ways-judge
/// mod goes first, so attend's withdrawal finds no other mod of ours and
/// removes the marketplace too.
pub fn withdraw_target(source_root: &Path, dest_root: &Path, default_root: &Path, dry_run: bool) -> Vec<String> {
    if !dry_run {
        Record::forget_old(dest_root);
    }
    let record = Record::read(dest_root);
    let mut lines = Vec::new();
    for m in MODS.iter().rev() {
        // ways-judge is not wanted by default: only a target that may hold
        // it pays for the call.
        if *m == JUDGE && !record.has(m.id) && !listed_in(m, dest_root) {
            continue;
        }
        lines.extend(withdraw(m, source_root, dest_root, default_root, dry_run));
    }
    if !dry_run {
        if let Err(e) = Record::update(dest_root, |r| r.installed.clear()) {
            lines.push(format!("⚠ could not empty repair's record in {}: {e}", Record::path(dest_root).display()));
        }
    }
    lines
}

/// Remove `m` from a disabled target. It runs where the source ships the mod
/// or its marketplace, or the record lists it, so a source that stopped
/// shipping the mod still takes it out.
fn withdraw(m: &Mod, source_root: &Path, dest_root: &Path, default_root: &Path, dry_run: bool) -> Vec<String> {
    let marketplace = marketplace_dir(source_root).join(".claude-plugin/marketplace.json").is_file();
    let record = Record::read(dest_root);
    if shipped_version(m, source_root).is_none() && !marketplace && !record.has(m.id) {
        return Vec::new();
    }
    let Some(claude) = claude_bin() else {
        return vec![format!("{}: ⚠ not checked in {}: `claude` is not on PATH", m.label, dest_root.display())];
    };
    let (cfg, _) = config_dir(dest_root, default_root);
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    match query(m, &claude, cfg.as_deref()) {
        Ok(have) => {
            let plan = plan_withdraw(&have, &dir, record.folder.as_deref(), &recover_command(cfg.as_deref()), false);
            execute(m, &claude, cfg.as_deref(), dest_root, &plan, &have, dry_run)
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
        Some(v) => Err(format!(
            "Claude Code {v} is older than {}, the oldest checked to carry the calls the {} makes",
            m.min_claude_code, m.label
        )),
        None => Err(format!("`claude --version` printed no version ({})", out.trim())),
    }
}

/// Whether this host can run `m`: `claude` is on PATH, it is at least the
/// oldest Claude Code checked to carry the mod's calls (`m.min_claude_code`),
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

/// One mod in one enabled target as `ways manage mod list` and `check`, `ways
/// status` and the settings screens' mod tab read it (ADR-512 items 5, 6).
/// Read-only: Claude Code is asked, nothing is changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub dir: PathBuf,
    /// `mods.<name>.install` resolved for this target.
    pub wanted: bool,
    /// The target's own config sets `install: false`.
    pub target_off: bool,
    /// The installed version, when Claude Code lists the mod.
    pub installed: Option<String>,
    /// `enabledPlugins` for the mod in the target's settings.
    pub enabled: Option<bool>,
    /// Repair's record lists the mod.
    pub recorded: bool,
    /// The `agent-ways` marketplace: absent, this app's, or someone else's.
    pub market: MarketState,
    /// Plugins of the mod's name from other marketplaces.
    pub foreign: Vec<String>,
    /// The ids in `foreign` that only `settings.json` enables, and that file.
    pub settings_only: Vec<String>,
    pub settings_file: Option<PathBuf>,
    /// Whether the host can run the mod: Claude Code's version against the
    /// floor, or why it could not be read.
    pub host: Result<String, String>,
    /// Claude Code's version, when `claude --version` named one.
    pub claude_code: Option<String>,
    /// Claude Code's plugin lists were read. Without them (no `claude`)
    /// nothing is known of what is installed, and repair's record says
    /// nothing of a removal by hand.
    pub read: bool,
    /// Why Claude Code's plugins could not be read.
    pub error: Option<String>,
    /// The version sessions run: the shipped one when the mod is installed
    /// from this app's folder, which sessions read in place.
    pub runs: Option<String>,
}

/// Whose the `agent-ways` marketplace is, as [`View`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketState {
    Absent,
    Ours,
    Foreign(String),
}

/// A mod's state in one target, in a word (ADR-512 item 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Wanted and installed, enabled, at the shipped version or newer.
    Converged,
    /// Wanted and missing, and the host cannot run it yet.
    Pending,
    /// Kept out of this target by its config, or missing while the record
    /// lists it: removed by hand.
    Removed,
    /// Installed and disabled in Claude Code.
    Disabled,
    /// Not wanted and not installed.
    Off,
    /// Anything a repair would change, or that Claude Code could not tell.
    Drift,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Converged => "converged",
            State::Pending => "pending",
            State::Removed => "removed",
            State::Disabled => "disabled",
            State::Off => "off",
            State::Drift => "drift",
        }
    }

    /// The mod's word over every enabled target: the first of pending,
    /// removed, disabled and drift that any target is in, then off when every
    /// target is off, else converged.
    pub fn over(states: &[State]) -> State {
        for s in [State::Pending, State::Removed, State::Disabled, State::Drift] {
            if states.contains(&s) {
                return s;
            }
        }
        if !states.is_empty() && states.iter().all(|s| *s == State::Off) {
            State::Off
        } else if states.contains(&State::Converged) {
            State::Converged
        } else {
            State::Off
        }
    }
}

/// Why a critical mod leaves agent-ways degraded in one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Degraded {
    /// Claude Code does not list it.
    Missing,
    /// Installed and disabled in Claude Code.
    Disabled,
    /// Missing, and the host cannot run it yet.
    Pending,
}

impl Degraded {
    pub fn word(self) -> &'static str {
        match self {
            Degraded::Missing => "not installed",
            Degraded::Disabled => "disabled",
            Degraded::Pending => "pending",
        }
    }
}

impl View {
    /// Why the mod does not run in this target, whatever its wanted state:
    /// one kept out by `install: false` counts as missing. `None` when it
    /// runs, when Claude Code's plugins could not be read, or when a plugin
    /// of its name or an `agent-ways` marketplace from elsewhere is the
    /// operator's own, which repair leaves alone.
    pub fn degraded(&self) -> Option<Degraded> {
        if self.error.is_some() || !self.foreign.is_empty() || matches!(self.market, MarketState::Foreign(_)) {
            return None;
        }
        match (self.installed.is_some(), self.enabled) {
            (true, Some(false)) => Some(Degraded::Disabled),
            (true, _) => None,
            (false, _) if self.host.is_err() => Some(Degraded::Pending),
            (false, _) => Some(Degraded::Missing),
        }
    }

    pub fn state(&self, shipped: Option<&str>) -> State {
        if self.error.is_some() || !self.foreign.is_empty() || matches!(self.market, MarketState::Foreign(_)) {
            return State::Drift;
        }
        if !self.read {
            return match (self.wanted, self.target_off) {
                (true, _) => State::Pending,
                (false, true) => State::Removed,
                (false, false) => State::Off,
            };
        }
        let older = |v: &str| shipped.is_some_and(|s| super::update::version_older(v, s));
        match (self.wanted, self.installed.as_deref()) {
            (true, Some(_)) if self.enabled == Some(false) => State::Disabled,
            (true, Some(v)) if older(v) => State::Drift,
            (true, Some(_)) => State::Converged,
            (true, None) if self.recorded => State::Removed,
            (true, None) if self.host.is_err() => State::Pending,
            (true, None) => State::Drift,
            (false, Some(_)) => State::Drift,
            (false, None) if self.target_off => State::Removed,
            (false, None) => State::Off,
        }
    }

    /// The state in a few words, the deciding word first, as the mod tab's
    /// row shows it so it reads at 80 columns; [`View::describe`] says it in
    /// full.
    pub fn row(&self, m: &Mod, shipped: Option<&str>) -> String {
        if let Some(e) = &self.error {
            return format!("unknown · {e}");
        }
        if !self.foreign.is_empty() {
            return format!("foreign · {}", self.foreign.join(", "));
        }
        let installed = self.installed.as_deref().unwrap_or_default();
        match self.state(shipped) {
            State::Converged => format!("installed · {installed}"),
            State::Disabled => format!("disabled · {installed}"),
            State::Pending => match &self.claude_code {
                Some(v) => format!("pending · {v} < {}", m.min_claude_code),
                None => format!("pending · {}", self.host.as_ref().err().map(String::as_str).unwrap_or_default()),
            },
            State::Removed if self.target_off => "removed · install false here".to_string(),
            State::Removed => "removed · by hand; repair writes install false".to_string(),
            State::Off => "off · not wanted".to_string(),
            State::Drift => match (self.wanted, self.installed.as_deref(), &self.market) {
                (_, _, MarketState::Foreign(from)) => format!("foreign · marketplace {from}"),
                (true, Some(i), _) => format!("update · {i} → {}", shipped.unwrap_or("?")),
                (true, None, _) => "missing · repair installs it".to_string(),
                (false, Some(i), _) => format!("installed · {i}, not wanted"),
                (false, None, _) => "off · not wanted".to_string(),
            },
        }
    }

    /// The state in one line, as `list`, `ways status` and the mod tab show
    /// it.
    pub fn describe(&self, m: &Mod, shipped: Option<&str>) -> String {
        if let Some(e) = &self.error {
            return format!("unknown: {e}");
        }
        if !self.foreign.is_empty() {
            return format!("{} from another marketplace, left as it is", self.foreign.join(", "));
        }
        let enabled = match self.enabled {
            Some(false) => ", disabled",
            _ => ", enabled",
        };
        let runs = |v: &str| match self.runs.as_deref() {
            Some(r) if r != v => format!("runs {r} from the app's folder (Claude Code records {v})"),
            _ => format!("installed {v}"),
        };
        let name = m.name;
        match self.state(shipped) {
            State::Converged | State::Disabled => format!("{}{enabled}", runs(self.installed.as_deref().unwrap_or_default())),
            State::Pending => match &self.claude_code {
                Some(v) => format!("pending: Claude Code {v} is below {}", m.min_claude_code),
                None => format!("pending: {}", self.host.as_ref().err().map(String::as_str).unwrap_or_default()),
            },
            State::Removed if self.target_off => "removed by hand or kept out; install false in this target's config".to_string(),
            State::Removed => "removed by hand; the next repair writes install false in this target's config".to_string(),
            State::Off => "not installed (not wanted)".to_string(),
            State::Drift => match (self.wanted, self.installed.as_deref(), &self.market) {
                (_, _, MarketState::Foreign(from)) => format!("the {MARKETPLACE} marketplace is someone else's ({from})"),
                (true, Some(v), _) => format!("{}{enabled}, older than the shipped {}", runs(v), shipped.unwrap_or("?")),
                (true, None, _) => "not installed; the next repair installs it".to_string(),
                (false, Some(v), _) => format!("installed {v}, not wanted; the next repair uninstalls it (mods.{name}.install is false)"),
                (false, None, _) => "not installed (not wanted)".to_string(),
            },
        }
    }
}

/// Every mod the source ships (or only `only`) in one enabled target, read
/// with one pair of list calls and one `claude --version`.
pub fn views(source_root: &Path, t: &Target, only: Option<&Mod>) -> Vec<(Mod, View)> {
    let mods: Vec<Mod> =
        MODS.into_iter().filter(|m| only.is_none_or(|o| o.id == m.id) && shipped_version(m, source_root).is_some()).collect();
    if mods.is_empty() {
        return Vec::new();
    }
    let claude = claude_bin();
    let cfg = config_dir(t.dest_root, t.default_root).0;
    let lists = claude.as_deref().ok_or_else(|| "`claude` is not on PATH".to_string()).and_then(|c| query_lists(c, cfg.as_deref()));
    let version = claude.as_deref().map(|c| claude_stdout(c, cfg.as_deref(), &["--version"]));
    let record = Record::read(t.dest_root);
    let dir = marketplace_dir(source_root).to_string_lossy().to_string();
    let target_off = |m: &Mod| ways_core::settings::mod_install_in_file(m.name, t.config) == Some(false);
    mods.into_iter()
        .map(|m| {
            let shipped = shipped_version(&m, source_root);
            let host = match &version {
                None => Err("`claude` is not on PATH".to_string()),
                Some(Err(e)) => Err(format!("`claude --version` failed: {e}")),
                Some(Ok(out)) => new_enough(&m, out).map(|()| claude_code_version(out).unwrap_or_default().to_string()),
            };
            let mut v = View {
                dir: t.dest_root.to_path_buf(),
                wanted: t.wants(&m),
                target_off: target_off(&m),
                installed: None,
                enabled: enabled_value(&m, t.dest_root),
                recorded: record.has(m.id),
                market: MarketState::Absent,
                foreign: Vec::new(),
                settings_only: Vec::new(),
                settings_file: None,
                host,
                claude_code: version.as_ref().and_then(|v| v.as_ref().ok()).and_then(|o| claude_code_version(o)).map(str::to_string),
                read: lists.is_ok(),
                error: None,
                runs: None,
            };
            match &lists {
                Ok(l) => {
                    let mut have = parse_installed(&m, &l.markets, &l.plugins);
                    add_enabled_foreign(&m, &mut have, t.dest_root);
                    v.market = match market(&have, &dir, record.folder.as_deref()) {
                        Market::Absent => MarketState::Absent,
                        Market::Current | Market::Moved => MarketState::Ours,
                        Market::Foreign(from) => MarketState::Foreign(from),
                    };
                    v.runs = runs_version(&have, &dir, shipped.as_deref());
                    v.installed = have.ours;
                    v.foreign = have.foreign;
                    v.settings_only = have.settings_only;
                    v.settings_file = have.settings_file;
                }
                // No `claude` at all is a host that cannot run the mod yet:
                // pending where wanted, off where not, never unknown.
                Err(e) if claude.is_some() => v.error = Some(e.clone()),
                Err(_) => {}
            }
            (m, v)
        })
        .collect()
}

/// The version sessions run: the shipped one when the mod is installed from
/// this app's folder `dir`, which sessions read in place.
fn runs_version(have: &Installed, dir: &str, shipped: Option<&str>) -> Option<String> {
    (have.ours.is_some() && have.marketplace.as_deref() == Some(dir)).then(|| shipped.map(str::to_string)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marketplace file, the mods the binary knows and the registry's
    /// `mods` entries name the same mods in the same order (ADR-512 item 1).
    #[test]
    fn the_marketplace_the_mods_and_the_settings_agree() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/.claude-plugin/marketplace.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let listed: Vec<&str> = json["plugins"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
        let known: Vec<&str> = MODS.iter().map(|m| m.name).collect();
        let settings: Vec<&str> = ways_core::settings::MODS.iter().map(|(n, _)| *n).collect();
        assert_eq!(listed, known, "marketplace.json against the binary's mods");
        assert_eq!(known, settings, "the binary's mods against the settings' mods entries");
        for m in MODS {
            assert_eq!(m.id, format!("{}@{MARKETPLACE}", m.name));
            assert!(ways_core::settings::SCHEMA.keys.iter().any(|k| k.name == format!("mods.{}.install", m.name)), "{}", m.name);
        }
    }

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
        // mods.attend.install false while ways-judge stays: attend goes, the
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
    fn the_enable_command_names_a_non_default_target() {
        assert_eq!(enable_command_in(&JUDGE, None), "claude plugin enable ways-judge@agent-ways");
        assert_eq!(
            enable_command_in(&JUDGE, Some(Path::new("/t/claude-b"))),
            "CLAUDE_CONFIG_DIR=/t/claude-b claude plugin enable ways-judge@agent-ways"
        );
    }

    #[test]
    fn the_recovery_command_names_a_non_default_target() {
        assert_eq!(recover_command(None), "claude plugin marketplace remove agent-ways");
        assert_eq!(recover_command(Some(Path::new("/t/claude-b"))), "CLAUDE_CONFIG_DIR=/t/claude-b claude plugin marketplace remove agent-ways");
    }

    /// With an empty plugin list only settings.json names the plugin, and
    /// uninstalling it would do nothing.
    #[test]
    fn a_plugin_only_settings_enables_is_worded_as_an_enabledplugins_entry() {
        let dir = std::env::temp_dir().join(format!("ways-attend-mod-settings-only-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"enabledPlugins":{"images@dotfiles":true}}"#).unwrap();
        let mut h = parse_installed(&IMAGES, "[]", "[]");
        add_enabled_foreign(&IMAGES, &mut h, &dir);
        assert_eq!(h.settings_only, vec!["images@dotfiles".to_string()]);
        let p = plan_converge(&IMAGES, &h, DIR, None, "1.48.0", "CMD");
        let want = format!(
            "images@dotfiles is enabled in {} (enabledPlugins) from a marketplace Claude Code has not recorded",
            dir.join("settings.json").display()
        );
        assert_eq!(p.notes.len(), 1, "{p:?}");
        assert!(p.notes[0].starts_with(&want) && p.notes[0].contains("remove that enabledPlugins entry, then `ways manage mod repair`"), "{p:?}");
        assert!(!p.notes[0].contains("is installed"), "{p:?}");

        // A plugin Claude Code lists keeps the installed wording.
        let mut listed = parse_installed(&IMAGES, "[]", r#"[{"id":"images@dotfiles","version":"0.1.0"}]"#);
        add_enabled_foreign(&IMAGES, &mut listed, &dir);
        assert!(listed.settings_only.is_empty());
        assert!(plan_converge(&IMAGES, &listed, DIR, None, "1.48.0", "CMD").notes[0].contains("is installed from another marketplace"));
        std::fs::remove_dir_all(&dir).unwrap();
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

    /// `images@dotfiles` enabled from a marketplace declared in settings.json,
    /// which `claude plugin list` does not show until a session records it.
    #[test]
    fn a_same_named_plugin_enabled_in_settings_is_foreign_and_nothing_installs_beside_it() {
        let dir = std::env::temp_dir().join(format!("ways-attend-mod-enabled-foreign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"enabledPlugins":{"images@dotfiles":true,"images@agent-ways":true,"images@old":false,"attend@agent-ways":true}}"#,
        )
        .unwrap();
        let mut h = parse_installed(&IMAGES, "[]", "[]");
        assert!(h.foreign.is_empty(), "the plugin list shows nothing");
        add_enabled_foreign(&IMAGES, &mut h, &dir);
        assert_eq!(h.foreign, vec!["images@dotfiles".to_string()], "ours and a disabled one are not foreign");
        let p = plan_converge(&IMAGES, &h, DIR, None, "1.48.0", "CMD");
        assert!(p.calls.is_empty(), "{p:?}");
        assert!(p.notes[0].contains("images@dotfiles") && p.notes[0].contains("images@agent-ways is not installed beside it"), "{p:?}");

        // Listed by Claude Code and enabled in settings, it is named once.
        let mut listed = parse_installed(&IMAGES, "[]", r#"[{"id":"images@dotfiles","version":"0.1.0"}]"#);
        add_enabled_foreign(&IMAGES, &mut listed, &dir);
        assert_eq!(listed.foreign, vec!["images@dotfiles".to_string()]);

        // The rule holds for every mod.
        let mut attend = parse_installed(&ATTEND, "[]", "[]");
        std::fs::write(dir.join("settings.json"), r#"{"enabledPlugins":{"attend@mine":true}}"#).unwrap();
        add_enabled_foreign(&ATTEND, &mut attend, &dir);
        assert_eq!(attend.foreign, vec!["attend@mine".to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_images_mod_runs_on_the_claude_code_it_was_verified_on() {
        assert!(new_enough(&IMAGES, "2.1.304 (Claude Code)").is_ok());
        assert!(new_enough(&IMAGES, "2.1.303 (Claude Code)").unwrap_err().contains("older than 2.1.304"));
        assert!(!IMAGES.critical_under(true) && !IMAGES.critical_under(false));
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
    fn the_theme_mod_runs_on_the_claude_code_it_was_seen_on() {
        assert!(new_enough(&THEME, "2.1.309 (Claude Code)").is_ok());
        assert!(new_enough(&THEME, "2.1.312 (Claude Code)").is_ok());
        let old = new_enough(&THEME, "2.1.308 (Claude Code)").unwrap_err();
        assert!(old.contains("older than 2.1.309"), "{old}");
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
    fn sessions_run_the_folder_version() {
        let ours = have(Some(DIR), Some("0.18.0"));
        assert_eq!(runs_version(&ours, DIR, Some("0.19.0")).as_deref(), Some("0.19.0"));
        assert_eq!(runs_version(&have(Some("/elsewhere"), Some("0.18.0")), DIR, Some("0.19.0")), None);
        assert_eq!(runs_version(&have(Some(DIR), None), DIR, Some("0.19.0")), None);
        let mut v = view(true, Some("0.18.0"));
        v.runs = Some("0.19.0".into());
        // Claude Code's record is older, which the next repair updates.
        assert_eq!(v.describe(&ATTEND, Some("0.19.0")), "runs 0.19.0 from the app's folder (Claude Code records 0.18.0), enabled, older than the shipped 0.19.0");
        v.installed = Some("0.19.0".into());
        assert_eq!(v.describe(&ATTEND, Some("0.19.0")), "installed 0.19.0, enabled");
    }

    /// The record keeps the folder on its first line and one line per mod
    /// repair installed; a record from before ADR-512, the folder alone,
    /// reads as an empty list.
    #[test]
    fn the_record_holds_the_folder_and_the_installed_mods() {
        assert_eq!(Record::parse("/app/plugins\n"), Record { folder: Some("/app/plugins".into()), installed: vec![] });
        let r = Record::parse("/app/plugins\ninstalled attend@agent-ways\ninstalled ways-judge@agent-ways\n");
        assert_eq!(r.installed, vec!["attend@agent-ways".to_string(), "ways-judge@agent-ways".to_string()]);
        assert_eq!(Record::parse(&r.render()), r, "it round-trips");
        let none = Record::parse("\ninstalled attend@agent-ways\n");
        assert_eq!((none.folder.clone(), none.has("attend@agent-ways")), (None, true), "a removed marketplace keeps the list");
        let mut r = Record::default();
        r.add("attend@agent-ways");
        r.add("attend@agent-ways");
        assert_eq!(r.installed.len(), 1);
        r.remove("attend@agent-ways");
        assert!(r.installed.is_empty());
    }

    fn view(wanted: bool, installed: Option<&str>) -> View {
        View {
            dir: PathBuf::from("/t/claude"),
            wanted,
            target_off: false,
            installed: installed.map(str::to_string),
            enabled: installed.map(|_| true),
            recorded: installed.is_some(),
            market: MarketState::Ours,
            foreign: vec![],
            settings_only: vec![],
            settings_file: None,
            host: Ok("2.1.309".into()),
            claude_code: Some("2.1.309".into()),
            read: true,
            error: None,
            runs: None,
        }
    }

    /// Re-review finding C: with Claude Code's lists unread (no `claude`), a
    /// mod on repair's record is pending where wanted and off where not;
    /// the record alone says nothing about a removal by hand.
    #[test]
    fn unread_lists_never_read_as_removed() {
        let s = Some("0.29.0");
        let mut v = view(true, None);
        v.recorded = true;
        v.read = false;
        v.host = Err("`claude` is not on PATH".into());
        v.claude_code = None;
        assert_eq!(v.state(s), State::Pending);
        v.wanted = false;
        assert_eq!(v.state(s), State::Off);
    }

    /// The mod tab's row leads with the deciding word, so it reads at 80
    /// columns.
    #[test]
    fn a_target_row_leads_with_its_word() {
        let s = Some("0.29.0");
        assert_eq!(view(true, Some("0.29.0")).row(&ATTEND, s), "installed · 0.29.0");
        let mut disabled = view(true, Some("0.29.0"));
        disabled.enabled = Some(false);
        assert_eq!(disabled.row(&ATTEND, s), "disabled · 0.29.0");
        let mut pending = view(true, None);
        pending.recorded = false;
        pending.host = Err("Claude Code 2.1.300 is older than 2.1.304".into());
        pending.claude_code = Some("2.1.300".into());
        assert_eq!(pending.row(&JUDGE, s), "pending · 2.1.300 < 2.1.304");
        let mut kept = view(false, None);
        kept.target_off = true;
        assert_eq!(kept.row(&ATTEND, s), "removed · install false here");
        assert_eq!(view(true, Some("0.28.0")).row(&ATTEND, s), "update · 0.28.0 → 0.29.0");
        assert_eq!(view(false, None).row(&ATTEND, s), "off · not wanted");
    }

    /// Each target's word, and the mod's over its targets (ADR-512 item 6).
    #[test]
    fn a_target_reads_as_one_word_and_a_mod_as_the_worst_of_them() {
        let s = Some("0.29.0");
        assert_eq!(view(true, Some("0.29.0")).state(s), State::Converged);
        assert_eq!(view(true, Some("0.28.0")).state(s), State::Drift, "older than shipped");
        assert_eq!(view(false, None).state(s), State::Off);
        assert_eq!(view(false, Some("0.29.0")).state(s), State::Drift, "installed, not wanted");
        let mut disabled = view(true, Some("0.29.0"));
        disabled.enabled = Some(false);
        assert_eq!(disabled.state(s), State::Disabled);
        let mut removed = view(true, None);
        removed.recorded = true;
        assert_eq!(removed.state(s), State::Removed, "missing while the record lists it");
        let mut kept = view(false, None);
        kept.target_off = true;
        assert_eq!(kept.state(s), State::Removed, "install false in the target's config");
        let mut pending = view(true, None);
        pending.host = Err("Claude Code 2.1.300 is older than 2.1.304".into());
        pending.claude_code = Some("2.1.300".into());
        assert_eq!(pending.state(s), State::Pending);
        assert_eq!(pending.describe(&JUDGE, s), "pending: Claude Code 2.1.300 is below 2.1.304");
        pending.claude_code = None;
        pending.host = Err("`claude` is not on PATH".into());
        assert_eq!(pending.describe(&JUDGE, s), "pending: `claude` is not on PATH");
        assert_eq!(view(true, None).state(s), State::Drift, "never installed: the next repair installs it");
        assert_eq!(State::over(&[State::Converged, State::Pending]), State::Pending);
        assert_eq!(State::over(&[State::Converged, State::Removed, State::Drift]), State::Removed);
        assert_eq!(State::over(&[State::Off, State::Off]), State::Off);
        assert_eq!(State::over(&[State::Converged, State::Off]), State::Converged);
        assert_eq!(State::over(&[]), State::Off);
    }
}
