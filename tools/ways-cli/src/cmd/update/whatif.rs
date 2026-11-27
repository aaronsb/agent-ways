// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `ways manage update` without `--commit`: what the update would change,
//! read from a fetch, with nothing else on disk touched (ADR-511).
//!
//! The report has six parts: the version of each component installed and at
//! the target; the release notes of every release in between; the source
//! delta grouped by area; the local changes the pull would stash; the
//! actions `--commit` would take on this machine; and the binary links a
//! dev install left, which the update would point back at the app.

use super::delta::{self, Delta};
use super::notes::{self, Release};
use super::{classify_build_groups, embedded_asset_prefixes, embedded_assets, exe, git_head, installed_version, package_field, suite_bins, version_older, way_embed_source_version, EMBEDDED_ASSETS};
use crate::cmd::attend_mod;
use crate::cmd::lifecycle::{self, DevLink, Mark, Report};
use crate::cmd::settings::retired::{self, Held};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the update would go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// The release channel: fast-forward to the branch's upstream.
    Release { upstream: Option<String> },
    /// `--ref`: check the ref out detached and build from source.
    Ref(String),
}

/// One component's versions: installed, and at the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub installed: Option<String>,
    pub target: Option<String>,
    /// The plugin's name when the row is a mod, which repair converges
    /// through Claude Code rather than the update building it.
    pub plugin: Option<String>,
    /// The target ships this component and the checkout does not: it is in
    /// the target's suite-bins or marketplace and not in the checkout's.
    pub new_at_target: bool,
    /// A component that ships with a release the update would download,
    /// whose release tag is not in the checkout yet: the version bump is on
    /// the branch and the release is not cut.
    pub awaiting_tag: bool,
}

impl Component {
    /// True for a mod's row: a plugin, not a binary the update builds.
    pub fn is_mod(&self) -> bool {
        self.plugin.is_some()
    }

    pub fn moves(&self) -> bool {
        match (&self.installed, &self.target) {
            (Some(i), Some(t)) => version_older(i, t),
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// Whether the update brings this component to the target. An installed
    /// one that moves counts, and so does one new at the target. A component
    /// that is not installed and not new, such as way-embed on a machine
    /// without it or a binary whose `--version` fails, is not installed by
    /// `--commit`, so it stays out. A suite binary the release channel
    /// downloads, and a known mod, count only once the release they ship
    /// with has its tag fetched. Anything built from source under `--ref`,
    /// and a mod this binary does not know, follow the first rule alone.
    pub fn converges(&self) -> bool {
        self.moves() && (self.installed.is_some() || self.new_at_target) && !self.awaiting_tag
    }
}

/// Where the checkout stands against the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// At the target.
    Same,
    /// Behind it: a fast-forward reaches it.
    Behind,
    /// Ahead of it: the target is an ancestor of HEAD.
    Ahead,
    /// Each side has commits the other lacks.
    Diverged,
    /// Git could not name one side.
    Unknown,
}

/// Everything the report shows, gathered before it is rendered.
#[derive(Debug, Clone)]
pub struct WhatIf {
    pub app: PathBuf,
    pub channel: Channel,
    pub head: Option<String>,
    pub target: Option<String>,
    pub fetch_error: Option<String>,
    pub relation: Relation,
    pub components: Vec<Component>,
    pub notes: Result<notes::Fetched, String>,
    pub delta: Option<Delta>,
    /// On `--ref`, the commits the checkout would leave behind when the ref
    /// is behind HEAD.
    pub dropped: Option<Delta>,
    pub local: Vec<String>,
    pub actions: Vec<(Mark, String)>,
    pub dev: Vec<DevLink>,
    /// The files that still hold a section a rename moved (ADR-512 item 2,
    /// ADR-417 item 1).
    pub retired: Vec<Held>,
    /// Why the update's `git pull --ff-only` would stop, on the release
    /// channel: `detached`, `no-upstream` or `diverged`.
    pub blocked: Option<&'static str>,
    pub verbose: bool,
}

/// Where the release notes come from, or None to fetch none.
pub type NotesSource<'a> = Option<&'a dyn Fn() -> Result<notes::Fetched, String>>;

/// Gather the what-if for `channel`. Runs a fetch and reads; writes nothing
/// else. The release notes come from `notes_from`; with None none are
/// fetched, so the call makes no request to GitHub.
pub fn gather(app: &Path, channel: Channel, verbose: bool, notes_from: NotesSource) -> WhatIf {
    let head = git_head(app);
    let (target, fetch_error) = resolve_target(app, &channel);
    let target_rev = target.clone().or_else(|| head.clone());
    let mut components = components(app, target_rev.as_deref());
    if matches!(channel, Channel::Release { .. }) {
        for c in components.iter_mut().filter(|c| c.moves()) {
            let Some(release) = release_of(c) else { continue };
            c.awaiting_tag = !c.target.as_deref().is_some_and(|t| has_tag(app, &format!("{release}-v{t}")));
        }
    }
    let moving = components.iter().any(Component::moves);
    let notes = if let (true, Some(fetch)) = (moving, notes_from) { fetch() } else { Ok(notes::Fetched::default()) };
    let relation = relation(app, head.as_deref(), target.as_deref());
    let delta = match (&head, &target) {
        (Some(h), Some(t)) if matches!(relation, Relation::Behind | Relation::Diverged) => delta::read(app, h, t),
        _ => None,
    };
    let dropped = match (&channel, &head, &target) {
        (Channel::Ref(_), Some(h), Some(t)) if relation == Relation::Ahead => delta::read(app, t, h),
        _ => None,
    };
    let local = git_lines(app, &["status", "--porcelain"]);
    let releases: &[Release] = notes.as_ref().map(|f| f.releases.as_slice()).unwrap_or(&[]);
    let actions = actions(app, &channel, relation, head.as_deref(), target.as_deref(), &components, releases, &local);
    let dev = lifecycle::update_dev_links(app);
    let retired = retired::files_holding(None);
    let blocked = block_reason(app, &channel, relation);
    WhatIf { app: app.to_path_buf(), channel, head, target, fetch_error, relation, components, notes, delta, dropped, local, actions, dev, retired, blocked, verbose }
}

/// The what-if as one JSON document: the channel, the target, the fetch
/// error, whether the install is behind, and each component's versions.
/// `behind` is true when any component the update converges moves, and false
/// when none does, compared against the target `gather` resolved. A
/// component that is not installed counts only when it is new at the
/// target, and a suite binary, or a mod that ships with one, only once that
/// release's tag is fetched. When the pull would stop, `behind` is null,
/// nothing moves, and `blocked` says why. A failed fetch leaves the last fetched target in place, so
/// `behind` still answers and `fetch_error` carries the error. `behind` is
/// null, and no component moves, also when no target resolved at all.
/// Release notes are not part of it. The update-check sensor reads
/// `behind`, `summary` and each component's `name`, `installed`, `target`
/// and `moves`.
///
/// References:
/// - ADR-416 item 6: the update check the attend sensor reads, and its
///   comparison against the last fetched target when a fetch fails.
pub fn json(w: &WhatIf) -> serde_json::Value {
    let (channel, upstream, git_ref) = match &w.channel {
        Channel::Release { upstream } => ("release", upstream.clone(), None),
        Channel::Ref(r) => ("ref", None, Some(r.clone())),
    };
    let blocked = w.blocked;
    let known = w.target.is_some();
    // With no target, every target version is a fallback to the checkout's
    // own, so nothing moves.
    let moves = |c: &Component| known && blocked.is_none() && c.converges();
    let moving: Vec<&Component> = w.components.iter().filter(|c| moves(c)).collect();
    let summary = moving
        .iter()
        .map(|c| match (&c.installed, &c.target) {
            (Some(i), Some(t)) => format!("{} {i} → {t}", c.name),
            (None, Some(t)) => format!("{} (new) → {t}", c.name),
            _ => c.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    serde_json::json!({
        "channel": channel,
        "upstream": upstream,
        "ref": git_ref,
        "target": w.target,
        "fetch_error": w.fetch_error,
        "behind": (known && blocked.is_none()).then_some(!moving.is_empty()),
        "blocked": blocked,
        "summary": summary,
        "components": w.components.iter().map(|c| serde_json::json!({
            "name": c.name,
            "installed": c.installed,
            "target": c.target,
            "moves": moves(c),
        })).collect::<Vec<_>>(),
    })
}

/// Where HEAD stands against the target.
fn relation(app: &Path, head: Option<&str>, target: Option<&str>) -> Relation {
    match (head, target) {
        (Some(h), Some(t)) if h == t => Relation::Same,
        (Some(h), Some(t)) if is_ancestor(app, h, t) => Relation::Behind,
        (Some(h), Some(t)) if is_ancestor(app, t, h) => Relation::Ahead,
        (Some(_), Some(_)) => Relation::Diverged,
        _ => Relation::Unknown,
    }
}

/// Fetch, then the commit the update would land on.
fn resolve_target(app: &Path, channel: &Channel) -> (Option<String>, Option<String>) {
    match channel {
        Channel::Release { upstream } => {
            let err = fetch(app, &["fetch", "--quiet", "origin"]);
            let Some(up) = upstream else { return (rev(app, "origin/main"), err) };
            (rev(app, up), err)
        }
        Channel::Ref(r) => match fetch(app, &["fetch", "--quiet", "origin", r]) {
            None => (rev(app, "FETCH_HEAD"), None),
            Some(e) => {
                // A bare sha the server will not serve by name: fetch
                // everything and resolve it here. The remote-tracking ref
                // comes before a local one, so a failed fetch compares
                // against the last fetched target. The error stays whenever
                // the fetch failed.
                let err = fetch(app, &["fetch", "--quiet", "--tags", "origin"]);
                let found = rev(app, &format!("origin/{r}")).or_else(|| rev(app, r));
                (found, err.or(Some(e)))
            }
        },
    }
}

/// Run a fetch; `Some(why)` when it failed.
fn fetch(app: &Path, args: &[&str]) -> Option<String> {
    match Command::new("git").args(args).current_dir(app).env("GIT_TERMINAL_PROMPT", "0").output() {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(failure_line(&String::from_utf8_lossy(&o.stderr))),
        Err(e) => Some(format!("git fetch could not run ({e})")),
    }
}

/// Whether the checkout has the tag `name`.
fn has_tag(app: &Path, name: &str) -> bool {
    !git_lines(app, &["tag", "--list", name]).is_empty()
}

/// The component whose release a moving component ships with, whose tag
/// `<release>-v<version>` says the release is cut. A suite binary ships with
/// itself. The attend mod ships with attend, and the other known mods with
/// ways, as `scripts/release.sh` bumps them together. way-embed and a mod
/// this binary does not know follow the version rule alone.
fn release_of(c: &Component) -> Option<&str> {
    match c.plugin.as_deref() {
        None => (c.name != "way-embed").then_some(c.name.as_str()),
        Some("attend") => Some("attend"),
        Some("ways-judge" | "ways-theme" | "images") => Some("ways"),
        Some(_) => None,
    }
}

/// Why the update's `git pull --ff-only` would stop on the release channel:
/// `detached` when HEAD is no branch, `no-upstream` when the branch has no
/// `branch.<name>.merge` setting, and `diverged` when the checkout and its
/// upstream each have commits the other lacks. A branch whose upstream is
/// set but not fetched yet is not blocked. None under `--ref`, which checks
/// the ref out itself.
fn block_reason(app: &Path, channel: &Channel, relation: Relation) -> Option<&'static str> {
    if !matches!(channel, Channel::Release { .. }) {
        return None;
    }
    let Some(branch) = git_lines(app, &["symbolic-ref", "-q", "--short", "HEAD"]).into_iter().next() else { return Some("detached") };
    if git_lines(app, &["config", "--get", &format!("branch.{branch}.merge")]).is_empty() {
        return Some("no-upstream");
    }
    (relation == Relation::Diverged).then_some("diverged")
}

/// The line of git's stderr that names the failure: the first starting
/// `fatal:` or `error:`, else the last line. The last line alone can be a
/// fragment such as "and the repository exists.".
fn failure_line(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.iter().find(|l| l.starts_with("fatal:") || l.starts_with("error:")).or(lines.last()).map_or("git fetch failed", |l| l).to_string()
}

/// The branch's upstream (`origin/main`), or None on a detached HEAD.
pub fn upstream(app: &Path) -> Option<String> {
    git_lines(app, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).into_iter().next()
}

fn rev(app: &Path, name: &str) -> Option<String> {
    git_lines(app, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).into_iter().next()
}

/// A read-only git query: `--no-optional-locks` keeps `git status` from
/// refreshing the index.
fn git_lines(app: &Path, args: &[&str]) -> Vec<String> {
    Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(app)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// A file at a revision.
fn show(app: &Path, rev: &str, path: &str) -> Option<String> {
    let out = Command::new("git").args(["--no-optional-locks", "show", &format!("{rev}:{path}")]).current_dir(app).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `(package, version)` of each crate under `tools/` at `rev`.
fn versions_at(app: &Path, rev: &str) -> Vec<(String, String)> {
    git_lines(app, &["ls-tree", "--name-only", rev, "tools/"])
        .into_iter()
        .filter_map(|dir| show(app, rev, &format!("{dir}/Cargo.toml")))
        .filter_map(|m| Some((package_field(&m, "name")?, package_field(&m, "version")?)))
        .collect()
}

/// The suite at `rev`, the installed suite and way-embed, and the mods, with
/// their versions on each side. The mods are those the marketplace lists at
/// `rev` and in the checkout: the update's reproject runs the newly installed
/// binary, which converges what the pulled marketplace ships. A mod this
/// binary does not know is named by its plugin.
fn components(app: &Path, rev: Option<&str>) -> Vec<Component> {
    let target_versions = rev.map(|r| versions_at(app, r)).unwrap_or_default();
    let mut names = suite_bins(app);
    let checkout_bins = names.clone();
    if let Some(list) = rev.and_then(|r| show(app, r, "tools/suite-bins")) {
        for n in list.lines().filter(|l| l.starts_with(|c: char| c.is_ascii_lowercase())).filter_map(|l| l.split_whitespace().next()) {
            if !names.iter().any(|x| x == n) {
                names.push(n.to_string());
            }
        }
    }
    let mut out: Vec<Component> = names
        .into_iter()
        .map(|name| {
            let installed = installed_version(&app.join("bin").join(exe(&name)));
            let target = target_versions.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone());
            let new_at_target = !checkout_bins.contains(&name);
            Component { name, installed, target, plugin: None, new_at_target, awaiting_tag: false }
        })
        .collect();
    let embed_installed = crate::paths::way_embed_in(&crate::paths::corpus_dir()).and_then(|b| installed_version(&b));
    let embed_target = match rev {
        Some(r) => show(app, r, "tools/way-embed/way-embed.cpp").and_then(|src| define_version(&src)),
        None => way_embed_source_version(app),
    };
    out.push(Component { name: "way-embed".into(), installed: embed_installed, target: embed_target, plugin: None, new_at_target: false, awaiting_tag: false });
    let mod_version = |text: Option<String>| -> Option<String> {
        let json: serde_json::Value = serde_json::from_str(&text?).ok()?;
        json.get("version")?.as_str().map(str::to_string)
    };
    const MARKETPLACE: &str = "plugins/.claude-plugin/marketplace.json";
    let mut plugins = marketplace_plugins(rev.and_then(|r| show(app, r, MARKETPLACE)).as_deref());
    let target_plugins = plugins.clone();
    let checkout_plugins = marketplace_plugins(std::fs::read_to_string(app.join(MARKETPLACE)).ok().as_deref());
    for name in checkout_plugins.iter().cloned() {
        if !plugins.contains(&name) {
            plugins.push(name);
        }
    }
    // attend's row always shows, shipped or not.
    if !plugins.iter().any(|n| n == attend_mod::ATTEND.name) {
        plugins.insert(0, attend_mod::ATTEND.name.to_string());
    }
    for name in plugins {
        let manifest = format!("plugins/{name}/.claude-plugin/plugin.json");
        out.push(Component {
            name: known_mod(&name).map_or_else(|| format!("{name} mod"), |m| m.label.to_string()),
            installed: mod_version(std::fs::read_to_string(app.join(&manifest)).ok()),
            target: mod_version(rev.and_then(|r| show(app, r, &manifest))),
            new_at_target: target_plugins.contains(&name) && !checkout_plugins.contains(&name),
            awaiting_tag: false,
            plugin: Some(name),
        });
    }
    out
}

/// The plugin names a marketplace.json lists, in its order.
fn marketplace_plugins(text: Option<&str>) -> Vec<String> {
    let Some(json) = text.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()) else { return Vec::new() };
    json.get("plugins")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("name")?.as_str().map(str::to_string))
        .collect()
}

/// The mod of [`attend_mod::MODS`] that the plugin `name` is, if this binary
/// knows it.
fn known_mod(name: &str) -> Option<attend_mod::Mod> {
    attend_mod::MODS.into_iter().find(|m| m.name == name)
}

/// The `#define VERSION "x"` of way-embed's source text.
fn define_version(src: &str) -> Option<String> {
    src.lines().find_map(|l| l.trim().strip_prefix("#define VERSION ")?.split('"').nth(1).map(str::to_string))
}

/// What `--commit` would do on this machine, in order.
#[allow(clippy::too_many_arguments)]
fn actions(app: &Path, channel: &Channel, relation: Relation, head: Option<&str>, target: Option<&str>, comps: &[Component], releases: &[Release], local: &[String]) -> Vec<(Mark, String)> {
    let dirty = !local.is_empty();
    let mut out = Vec::new();
    let cargo = super::tool_present("cargo");
    // Whether --commit puts a new ways binary in place: a mod this binary
    // does not know is converged only by a binary that knows it.
    let mut ways_replaced = false;
    let short = |s: &str| s.chars().take(8).collect::<String>();
    match channel {
        Channel::Release { upstream } => {
            let Some(up) = upstream else {
                out.push((Mark::Warn, "the checkout is on a detached HEAD (a --ref install), so the pull would stop. `ways manage update --ref main --commit` returns to the release channel".to_string()));
                return out;
            };
            match (relation, head, target) {
                (Relation::Same, _, Some(t)) => out.push((Mark::Same, format!("pull: already at {up} ({})", short(t)))),
                (Relation::Behind, Some(h), Some(t)) => {
                    if dirty {
                        out.push((Mark::Act, "stash the local changes, then restore them after the pull".to_string()));
                    }
                    out.push((Mark::Act, format!("pull: fast-forward {} → {} ({up})", short(h), short(t))));
                }
                (Relation::Ahead, Some(h), Some(t)) => out.push((Mark::Same, format!("pull: the checkout is ahead of {up} by {}, so the pull changes nothing", lifecycle::count(count_commits(app, t, h), "commit", "commits")))),
                (Relation::Diverged, _, _) => out.push((Mark::Warn, format!("pull: the checkout has diverged from {up}, so the fast-forward would stop and nothing after it would run"))),
                _ => out.push((Mark::Warn, "pull: git could not name the commits on either side".to_string())),
            }
            let (mut cargo_changed, mut embed_changed) = match (relation, head, target) {
                (Relation::Behind, Some(h), Some(t)) => changed(app, h, t),
                _ => (false, false),
            };
            // The working tree's own edits, from the `status --porcelain`
            // lines already read: `git diff HEAD` would refresh and rewrite
            // the index, and a what-if writes nothing (ADR-511).
            let (wt_cargo, wt_embed) = classify_build_groups(local.iter().filter_map(|l| porcelain_path(l)), &embedded_assets(app));
            cargo_changed |= wt_cargo;
            embed_changed |= wt_embed;
            let mut any = false;
            for c in comps.iter().filter(|c| !c.is_mod() && c.name != "way-embed") {
                if !(cargo_changed || c.moves()) {
                    continue;
                }
                any = true;
                let action = binary_action(c, releases, cargo);
                if c.name == "ways" {
                    ways_replaced = action.0 == Mark::Act;
                }
                out.push(action);
            }
            if let Some(e) = comps.iter().find(|c| c.name == "way-embed").filter(|c| embed_changed || c.moves()) {
                any = true;
                out.push((Mark::Act, format!("way-embed: the latest release, kept when at or ahead of {}; else a source build (needs cmake)", e.target.as_deref().unwrap_or("its source"))));
            }
            if !any {
                out.push((Mark::Same, "binaries: no binary source changed, so none is replaced".to_string()));
            }
        }
        Channel::Ref(r) => {
            if dirty {
                out.push((Mark::Warn, "local changes that conflict with the ref stop the checkout".to_string()));
            }
            out.push((Mark::Act, format!("check out {r} ({}) on a detached HEAD", target.map(short).unwrap_or_else(|| "unresolved".into()))));
            ways_replaced = cargo;
            if cargo {
                out.push((Mark::Act, "build the whole suite from source".to_string()));
            } else {
                out.push((Mark::Warn, "building the suite needs cargo, which is not on PATH, so --commit would stop here".to_string()));
            }
            out.push((Mark::Act, "build way-embed from source (needs cmake; optional)".to_string()));
        }
    }
    out.push((Mark::Act, format!("relink the suite onto PATH in {}", lifecycle::xdg_bin().display())));
    out.push((Mark::Act, "regenerate the corpus".to_string()));
    let claude_code_chosen = crate::cmd::settings::engine::claude_code_chosen();
    for t in crate::config::global().targets() {
        let dir = t.dir();
        if !t.enabled {
            out.push((Mark::Info, format!("{}: disabled, so repair keeps it withdrawn", dir.display())));
            continue;
        }
        out.push((Mark::Act, format!("reproject {} (ways manage repair)", dir.display())));
        let config = t.config_path();
        let record = attend_mod::Record::read(&dir);
        for c in comps.iter().filter(|c| c.is_mod()) {
            let Some(m) = c.plugin.as_deref().and_then(known_mod) else {
                out.extend(new_mod_line(&dir, c, ways_replaced));
                continue;
            };
            let here = ModHere {
                wanted: attend_mod::wants_in(&m, &config),
                held: ways_core::settings::mod_entry_ignored(m.name, &config),
                installed: attend_mod::listed_in(&m, &dir),
                recorded: record.has(m.id),
                held_out: if m == attend_mod::JUDGE {
                    claude_code_chosen
                } else {
                    ways_core::settings::mod_install_in_file(m.name, &config) == Some(false)
                },
            };
            out.extend(mod_line(&dir, &m, &here, c));
        }
    }
    out.push((Mark::Info, "check the judge's keys".to_string()));
    out
}

/// What the preview reads of one mod in one target, the way repair decides
/// it: from the target's settings, its ways config and repair's record, with
/// no call to `claude`.
///
/// References:
/// - ADR-511: a what-if writes nothing, and the `claude` CLI rewrites a
///   target's settings.json as it starts, so the preview never runs it.
#[derive(Debug, Clone, Default)]
struct ModHere {
    /// `mods.<name>.install` resolved for the target.
    wanted: bool,
    /// The finding that has the entry ignored; repair then leaves the mod as
    /// it finds it.
    held: Option<agent_settings::Finding>,
    /// The target's settings name the mod, as Claude Code's install writes.
    installed: bool,
    /// Repair's record says it installed the mod here.
    recorded: bool,
    /// A mod not wanted here is worth a line: the target's config sets its
    /// `install` to false, or, for the ways-judge mod, Claude Code internal
    /// is the chosen engine, the one the mod hosts.
    held_out: bool,
}

/// Mod `m`'s line for the target at `dir`, following what repair does with
/// it. A mod the target rev does not ship has none, since repair converges
/// only what the marketplace ships. One not wanted is removed where it is
/// installed, left as it is under an ignored entry (with repair's warning),
/// and otherwise named only where it is `held_out`. One wanted is installed
/// where it is missing, unless repair installed it before and it was removed
/// by hand, and otherwise updated or left.
fn mod_line(dir: &Path, m: &attend_mod::Mod, here: &ModHere, c: &Component) -> Option<(Mark, String)> {
    let d = dir.display();
    let target = c.target.as_deref()?;
    if !here.wanted {
        if let Some(f) = &here.held {
            let what = if here.installed { "left installed" } else { "not installed" };
            return Some((Mark::Warn, format!("{d}: {}: {what}: {}", m.label, attend_mod::held_why(m, dir, f, here.installed))));
        }
        if here.installed {
            return Some((Mark::Act, format!("{d}: remove the {} (mods.{}.install is false)", m.label, m.name)));
        }
        return here.held_out.then(|| kept_out(dir, m));
    }
    if !here.installed && here.recorded {
        return Some((Mark::Act, format!("{d}: the {} was removed by hand, so repair writes mods.{}.install: false and leaves it out", m.label, m.name)));
    }
    if !here.installed {
        // The preview runs no `claude`, so it names the floor rather than
        // checking it; --commit leaves the mod pending below it.
        return Some((Mark::Act, format!("{d}: install the {} {target} (needs Claude Code {} or later)", m.label, m.min_claude_code)));
    }
    Some(mod_action(dir, m.label, c))
}

/// The line for a mod the marketplace lists that this binary does not know.
/// Only a ways binary that knows the mod converges it, so it depends on
/// whether --commit puts a new ways binary in place (`ways_replaced`); when
/// it does not, the mod waits for one.
fn new_mod_line(dir: &Path, c: &Component, ways_replaced: bool) -> Option<(Mark, String)> {
    c.target.as_ref()?;
    let d = dir.display();
    Some(match (ways_replaced, c.installed.is_none()) {
        (true, true) => (Mark::Act, format!("{d}: install the {} where it is wanted (new in this release)", c.name)),
        (true, false) => (Mark::Info, format!("{d}: the {} is one this ways binary does not know; the updated binary converges it", c.name)),
        (false, _) => (Mark::Warn, format!("{d}: the {} waits for a ways binary that knows it; this update keeps the current ways binary", c.name)),
    })
}

/// A mod this target's config keeps out (`install: false`), which repair
/// wrote after a removal by hand or the operator set.
fn kept_out(dir: &Path, m: &attend_mod::Mod) -> (Mark, String) {
    (
        Mark::Info,
        format!(
            "{}: the {} stays out: mods.{}.install is false in this target's config; `ways manage mod install {} --target {} --commit` installs it",
            dir.display(),
            m.label,
            m.name,
            m.name,
            dir.display()
        ),
    )
}

/// What repair does to an installed mod in the target at `dir`: update it
/// when its row moves, else leave it.
fn mod_action(dir: &Path, row: &str, c: &Component) -> (Mark, String) {
    if c.moves() {
        (Mark::Act, format!("{}: update the {row} {} → {}", dir.display(), c.installed.as_deref().unwrap_or("none"), c.target.as_deref().unwrap_or("?")))
    } else {
        (Mark::Same, format!("{}: the {row} stays as it is", dir.display()))
    }
}

/// How one suite binary would be refreshed.
fn binary_action(c: &Component, releases: &[Release], cargo: bool) -> (Mark, String) {
    let target = c.target.as_deref().unwrap_or("?");
    if !c.moves() {
        return (Mark::Same, format!("{}: stays at {} (its source did not move past it)", c.name, c.installed.as_deref().unwrap_or("?")));
    }
    let latest = notes::latest(releases, &c.name);
    match latest {
        Some(l) if !version_older(l, target) => (Mark::Act, format!("{}: download the {}-v{l} release", c.name, c.name)),
        _ if cargo => (Mark::Act, format!("{}: build {target} from source (no {target} release{})", c.name, latest.map(|l| format!("; the latest is {l}")).unwrap_or_default())),
        _ => (Mark::Warn, format!("{}: no {target} release and no cargo, so it keeps {}", c.name, c.installed.as_deref().unwrap_or("its current binary"))),
    }
}

/// The build groups the commits `a..b` touch, classified by the embedded-file
/// list at `b`, the checkout the update would land on.
fn changed(app: &Path, a: &str, b: &str) -> (bool, bool) {
    let paths = git_lines(app, &["diff", "--name-only", a, b]);
    classify_build_groups(paths.iter().map(String::as_str), &embedded_asset_prefixes(show(app, b, EMBEDDED_ASSETS).as_deref()))
}

/// The path a `git status --porcelain` line names (the destination of a
/// rename), or None for an untracked file, which no build reads until it is
/// tracked.
fn porcelain_path(line: &str) -> Option<&str> {
    if line.starts_with("??") || line.len() < 4 {
        return None;
    }
    let path = &line[3..];
    Some(path.rsplit_once(" -> ").map_or(path, |(_, to)| to).trim_matches('"'))
}

/// Commits in `a..b`.
fn count_commits(app: &Path, a: &str, b: &str) -> usize {
    git_lines(app, &["rev-list", "--count", &format!("{a}..{b}")]).first().and_then(|n| n.parse().ok()).unwrap_or(0)
}

fn is_ancestor(app: &Path, a: &str, b: &str) -> bool {
    Command::new("git").args(["--no-optional-locks", "merge-base", "--is-ancestor", a, b]).current_dir(app).status().is_ok_and(|s| s.success())
}

/// The report, ready to print.
pub fn render(w: &WhatIf) -> Report {
    let command = match &w.channel {
        Channel::Release { .. } => "ways manage update".to_string(),
        Channel::Ref(r) => format!("ways manage update --ref {}", lifecycle::shell_word(r)),
    };
    let on_ref = matches!(w.channel, Channel::Ref(_));
    let mut r = Report::new(format!("{command}: what-if"));
    let short = |s: &Option<String>| s.as_deref().map(|s| s.chars().take(8).collect::<String>()).unwrap_or_else(|| "?".into());
    r.subtitle(format!("app {} · at {} · target {}", w.app.display(), short(&w.head), short(&w.target)));
    if let Some(e) = &w.fetch_error {
        r.section("Fetch");
        r.item(Mark::Warn, format!("the fetch failed ({e}); the report reads what the checkout already has"));
    }

    r.section("Versions");
    let width = w.components.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in &w.components {
        let pad = width - c.name.len();
        let name = format!("{}{:pad$}", c.name, "");
        match (&c.installed, &c.target) {
            (Some(i), Some(t)) if version_older(i, t) => r.item(Mark::Up, format!("{name}  {i} → {t}")),
            // `--ref` checks the ref out and builds it, older or not.
            (Some(i), Some(t)) if on_ref && version_older(t, i) && !c.is_mod() => r.item(Mark::Down, format!("{name}  {i} → {t} (a downgrade: --commit rebuilds it from source)")),
            (Some(i), Some(t)) if on_ref && version_older(t, i) => r.item(Mark::Down, format!("{name}  {i} → {t} (a downgrade)")),
            (Some(i), Some(t)) if i != t => r.item(Mark::Same, format!("{name}  {i} (ahead of {t}; unchanged)")),
            (Some(i), _) => r.item(Mark::Same, format!("{name}  {i} unchanged")),
            (None, Some(t)) => r.item(Mark::Add, format!("{name}  not installed → {t}")),
            (None, None) => r.item(Mark::Info, format!("{name}  not installed")),
        };
    }

    let moving: Vec<&Component> = w.components.iter().filter(|c| c.moves() && !c.is_mod()).collect();
    if !moving.is_empty() || w.components.iter().any(|c| c.is_mod() && c.moves()) {
        r.section("Release notes");
        match &w.notes {
            Err(e) => {
                r.item(Mark::Warn, format!("the release notes could not be fetched ({e}); the versions above still hold"));
            }
            Ok(fetched) => {
                if let Some(warning) = &fetched.warning {
                    r.item(Mark::Warn, warning);
                }
                let releases = fetched.releases.as_slice();
                for c in &moving {
                    let to = c.target.as_deref().unwrap_or("?");
                    let found = notes::between(releases, &c.name, c.installed.as_deref(), to);
                    r.sub(format!("{} {} → {to}", c.name, c.installed.as_deref().unwrap_or("none")));
                    match c.installed.as_deref() {
                        None => {
                            r.muted(4, "not installed, so only the target's own release is shown");
                        }
                        Some(i) if !notes::orderable(i) => {
                            r.muted(4, format!("the installed version {i} does not order against releases, so only the target's own release is shown"));
                        }
                        _ => {}
                    }
                    if found.is_empty() {
                        r.muted(4, format!("no {} release between them yet", c.name));
                    }
                    for rel in found {
                        r.tagged(Mark::Up, 4, &rel.tag, 0, "");
                        for line in notes::trim(&rel.body) {
                            if line.starts_with('#') {
                                r.strong(6, line);
                            } else {
                                r.text(6, line);
                            }
                        }
                    }
                    r.blank();
                }
                // The attend mod moves with attend's version, the others this
                // binary knows with ways's.
                for c in w.components.iter().filter(|c| c.moves()) {
                    let Some(m) = c.plugin.as_deref().and_then(known_mod) else { continue };
                    let notes = if m == attend_mod::ATTEND { "attend's" } else { "the ways" };
                    r.muted(2, format!("The {}'s changes are in {notes} release notes.", m.label));
                }
            }
        }
    }

    r.section("Changes");
    match (w.relation, &w.delta, &w.dropped) {
        (Relation::Same, _, _) => {
            r.item(Mark::Same, "the checkout is at the target: no new commits");
        }
        (Relation::Ahead, _, Some(d)) => {
            r.item(Mark::Warn, "the ref is behind the checkout: --commit leaves these commits behind");
            delta::render(&mut r, d, w.verbose, delta::Direction::Dropped);
        }
        (Relation::Ahead, _, None) => {
            r.item(Mark::Same, "the checkout is ahead of the target: no new commits");
        }
        (_, Some(d), _) => delta::render(&mut r, d, w.verbose, delta::Direction::Incoming),
        _ => {
            r.item(Mark::Warn, "git could not list the commits between the checkout and the target");
        }
    }

    if !w.local.is_empty() {
        r.section("Local changes");
        let differ = if w.local.len() == 1 { "differs" } else { "differ" };
        r.item(Mark::Warn, format!("{} in the app {differ} from its commit; the pull stashes them and restores them after", lifecycle::count(w.local.len(), "path", "paths")));
        for l in &w.local {
            r.muted(4, l);
        }
    }

    r.section("Actions");
    r.muted(2, "--commit would, on this machine:");
    for (mark, text) in &w.actions {
        r.item(*mark, text);
    }
    retired_section(&mut r, &w.retired);

    lifecycle::dev_links_section(&mut r, &w.dev, "The update", &crate::paths::data_root());

    let level = w.relation == Relation::Same || (!on_ref && w.relation == Relation::Ahead);
    // A file holding a retired section is the operator's to fix: `--commit`
    // changes nothing about it.
    let up_to_date = level && !w.components.iter().any(Component::moves) && w.local.is_empty() && w.dev.is_empty();
    if up_to_date && !w.retired.is_empty() {
        r.footer(Mark::Ok, "Up to date; each file above names the `ways settings fix` command that moves its section.");
    } else if up_to_date {
        r.footer(Mark::Ok, "Up to date. Nothing changed.");
    } else {
        r.footer(Mark::Info, lifecycle::commit_hint(&command));
    }
    r
}

/// The files that still hold a section a rename moved, one ⚠ line each
/// saying whether it is still read, with the command that moves it.
/// Nothing in the update rewrites them (ADR-512 item 2, ADR-417 item 1).
pub fn retired_section(r: &mut Report, held: &[Held]) {
    if held.is_empty() {
        return;
    }
    r.section("Retired settings");
    r.muted(2, "the update leaves these as they are:");
    for h in held {
        r.item(Mark::Warn, retired_line(h));
    }
}

/// One file's line: where the old section is, whether it is still read,
/// and the command that moves it.
pub fn retired_line(h: &Held) -> String {
    h.moved_line()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_theme::Painter;

    const MOD: &str = attend_mod::ATTEND.label;
    const JUDGE_MOD: &str = attend_mod::JUDGE.label;

    fn comp(name: &str, i: Option<&str>, t: Option<&str>) -> Component {
        Component { name: name.into(), installed: i.map(str::to_string), target: t.map(str::to_string), plugin: None, new_at_target: false, awaiting_tag: false }
    }

    fn modc(m: &attend_mod::Mod, i: Option<&str>, t: Option<&str>) -> Component {
        Component { plugin: Some(m.name.to_string()), ..comp(m.label, i, t) }
    }

    #[test]
    fn the_report_has_its_sections_in_order_with_notes_trimmed() {
        let w = WhatIf {
            app: PathBuf::from("/app"),
            channel: Channel::Release { upstream: Some("origin/main".into()) },
            head: Some("aaaaaaaaaaaa".into()),
            target: Some("bbbbbbbbbbbb".into()),
            fetch_error: None,
            relation: Relation::Behind,
            components: vec![comp("ways", Some("1.40.0"), Some("1.41.0")), comp("attend", Some("0.28.1"), Some("0.28.1")), modc(&attend_mod::ATTEND, Some("0.28.1"), Some("0.28.1"))],
            notes: Ok(notes::Fetched {
                releases: vec![
                    Release { tag: "ways-v1.41.0".into(), body: "## ways 1.41.0\n\n### Added\n- a thing (#5)\n\n---\nPre-built ways CLI binaries for 4 platforms.\n".into() },
                    Release { tag: "ways-v1.40.0".into(), body: "old".into() },
                ],
                warning: None,
            }),
            delta: Some(Delta { commits: vec![delta::classify("feat(ways): a thing", &[])], prs: vec![5] }),
            dropped: None,
            local: vec![" M settings.json".into()],
            actions: vec![(Mark::Act, "pull: fast-forward aaaaaaaa → bbbbbbbb (origin/main)".into())],
            dev: Vec::new(),
            retired: vec![
                Held { path: PathBuf::from("/p/.claude/ways.yaml"), line: Some(3), kind: retired::Kind::Project(PathBuf::from("/p")), moved: &retired::ATTEND_MOD },
                Held { path: PathBuf::from("/p/.claude/ways.yaml"), line: Some(7), kind: retired::Kind::Project(PathBuf::from("/p")), moved: &retired::KEEPALIVE },
            ],
            blocked: None,
            verbose: false,
        };
        let text = render(&w).render(&Painter::plain());
        let order: Vec<usize> = ["── Versions", "── Release notes", "── Changes", "── Local changes", "── Actions", "── Retired settings"].iter().map(|h| text.find(h).unwrap_or_else(|| panic!("{h} missing:\n{text}"))).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(text.contains("↑ ways        1.40.0 → 1.41.0"), "{text}");
        assert!(text.contains("= attend      0.28.1 unchanged"), "{text}");
        assert!(text.contains("    ways-v1.41.0\n      ### Added\n      - a thing (#5)\n"), "{text}");
        assert!(!text.contains("Pre-built"), "the CI tail is dropped");
        assert!(!text.contains("ways-v1.40.0"), "the installed release is not repeated");
        assert!(text.contains("⚠ /p/.claude/ways.yaml:3 holds attend_mod, now mods.attend; nothing reads it: `ways settings fix mods.attend --project /p` moves it"), "{text}");
        assert!(
            text.contains("⚠ /p/.claude/ways.yaml:7 holds mods.attend.keepalive, now mods.attend.continuity; it is still read: `ways settings fix mods.attend.continuity --project /p` moves it"),
            "{text}"
        );
        assert!(text.ends_with("· Nothing changed. Run `ways manage update --commit` to apply.\n"), "{text}");
    }

    #[test]
    fn unreachable_notes_are_named_and_the_report_goes_on() {
        let w = WhatIf {
            app: PathBuf::from("/app"),
            channel: Channel::Ref("main".into()),
            head: Some("a".into()),
            target: Some("a".into()),
            fetch_error: Some("could not resolve host".into()),
            relation: Relation::Same,
            components: vec![comp("ways", Some("1.0.0"), Some("1.1.0"))],
            notes: Err("GitHub Releases could not be reached".into()),
            delta: None,
            dropped: None,
            local: Vec::new(),
            actions: Vec::new(),
            dev: Vec::new(),
            retired: Vec::new(),
            blocked: None,
            verbose: false,
        };
        let text = render(&w).render(&Painter::plain());
        assert!(text.contains("⚠ the fetch failed (could not resolve host)"), "{text}");
        assert!(text.contains("⚠ the release notes could not be fetched (GitHub Releases could not be reached)"), "{text}");
        assert!(text.contains("no new commits"), "{text}");
        assert!(text.contains("`ways manage update --ref main --commit`"), "{text}");
    }

    #[test]
    fn porcelain_lines_name_their_paths() {
        assert_eq!(porcelain_path(" M tools/ways-cli/src/main.rs"), Some("tools/ways-cli/src/main.rs"));
        assert_eq!(porcelain_path("R  docs/a.md -> tools/b.rs"), Some("tools/b.rs"));
        assert_eq!(porcelain_path("?? tools/new.rs"), None, "untracked");
        assert_eq!(classify_build_groups([" M tools/x/src/lib.rs", "?? tools/way-embed/y.cpp"].iter().filter_map(|l| porcelain_path(l)), &[]), (true, false));
    }

    #[test]
    fn define_version_reads_way_embeds_source() {
        assert_eq!(define_version("#define VERSION_MAJOR 1\n#define VERSION \"1.2.0\"\n").as_deref(), Some("1.2.0"));
    }

    #[test]
    fn an_installed_mod_is_updated_or_left_like_attends() {
        let dir = Path::new("/t/claude");
        let judge = modc(&attend_mod::JUDGE, Some("1.50.0"), Some("1.51.0"));
        assert_eq!(mod_action(dir, JUDGE_MOD, &judge), (Mark::Act, "/t/claude: update the ways-judge mod 1.50.0 → 1.51.0".into()));
        let same = modc(&attend_mod::JUDGE, Some("1.51.0"), Some("1.51.0"));
        assert_eq!(mod_action(dir, JUDGE_MOD, &same), (Mark::Same, "/t/claude: the ways-judge mod stays as it is".into()));
        let attend = modc(&attend_mod::ATTEND, Some("0.28.1"), Some("0.28.4"));
        assert_eq!(mod_action(dir, MOD, &attend), (Mark::Act, "/t/claude: update the attend mod 0.28.1 → 0.28.4".into()));
        assert!(attend.is_mod() && !comp("ways", None, None).is_mod());
    }

    #[test]
    fn a_mod_line_follows_what_repair_does_with_the_mod() {
        let dir = Path::new("/t/claude");
        let line = |m: &attend_mod::Mod, here: ModHere, c: &Component| mod_line(dir, m, &here, c);
        let installed = ModHere { installed: true, ..ModHere::default() };
        let wanted = ModHere { wanted: true, installed: true, ..ModHere::default() };
        let judge = modc(&attend_mod::JUDGE, Some("1.50.0"), Some("1.51.0"));

        // Wanted and installed: updated or left.
        assert_eq!(line(&attend_mod::JUDGE, wanted.clone(), &judge).unwrap().0, Mark::Act, "wanted: repair updates it");
        let theme = modc(&attend_mod::THEME, Some("1.53.0"), Some("1.53.1"));
        assert_eq!(line(&attend_mod::THEME, wanted.clone(), &theme), Some((Mark::Act, "/t/claude: update the ways-theme mod 1.53.0 → 1.53.1".into())));
        // Wanted and missing: installed, unless it was removed by hand.
        let missing = ModHere { wanted: true, ..ModHere::default() };
        assert_eq!(
            line(&attend_mod::THEME, missing.clone(), &theme),
            Some((Mark::Act, "/t/claude: install the ways-theme mod 1.53.1 (needs Claude Code 2.1.309 or later)".into()))
        );
        let by_hand = line(&attend_mod::THEME, ModHere { recorded: true, ..missing }, &theme).unwrap();
        assert!(by_hand.1.contains("removed by hand, so repair writes mods.ways-theme.install: false"), "{}", by_hand.1);

        // Not wanted and installed: repair removes it.
        assert_eq!(
            line(&attend_mod::IMAGES, installed.clone(), &modc(&attend_mod::IMAGES, Some("1.53.1"), Some("1.53.1"))),
            Some((Mark::Act, "/t/claude: remove the images mod (mods.images.install is false)".into()))
        );
        // Not wanted under an ignored entry: left as it is, with repair's warning.
        let finding = agent_settings::Finding {
            file: Some(PathBuf::from("/c/config.yaml")),
            line: Some(4),
            section: Some("mods".into()),
            unit: Some("mods.images".into()),
            key: None,
            message: "an unknown key".into(),
            fallback: true,
            repair: None,
            closed: false,
        };
        let held = line(&attend_mod::IMAGES, ModHere { held: Some(finding), ..installed }, &modc(&attend_mod::IMAGES, Some("1.53.1"), Some("1.53.1"))).unwrap();
        assert_eq!(held.0, Mark::Warn);
        assert!(held.1.starts_with("/t/claude: images mod: left installed: /c/config.yaml:4") && held.1.contains("repair does not uninstall it"), "{}", held.1);

        // Not wanted and absent: named only where it is held out.
        let out = line(&attend_mod::JUDGE, ModHere { held_out: true, ..ModHere::default() }, &judge).unwrap();
        assert_eq!(out.0, Mark::Info);
        assert!(out.1.contains("mods.ways-judge.install is false in this target's config") && out.1.contains("ways manage mod install ways-judge --target /t/claude --commit"), "{}", out.1);
        assert_eq!(line(&attend_mod::JUDGE, ModHere::default(), &judge), None, "not held out: nothing to say");

        // The target rev does not ship it: repair converges nothing.
        assert_eq!(line(&attend_mod::JUDGE, wanted, &modc(&attend_mod::JUDGE, Some("1.50.0"), None)), None);
    }

    /// The preview once named only the attend and ways-judge mods, while the
    /// reproject that followed also updated the ways-theme and images mods.
    /// The rows now come from the marketplace the checkout ships.
    #[test]
    fn every_mod_the_marketplace_lists_has_a_row() {
        let app = crate::test_dir::TestDir::at(std::env::temp_dir().join(format!("ways-whatif-mods-{}", std::process::id())));
        let names = ["attend", "ways-judge", "ways-theme", "images", "sparkle"];
        for name in names {
            let dir = app.join("plugins").join(name).join(".claude-plugin");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plugin.json"), "{\"version\": \"1.53.1\"}").unwrap();
        }
        std::fs::create_dir_all(app.join("plugins/.claude-plugin")).unwrap();
        let listed: Vec<String> =names.iter().map(|n| format!("{{\"name\": \"{n}\", \"source\": \"./{n}\"}}")).collect();
        std::fs::write(app.join("plugins/.claude-plugin/marketplace.json"), format!("{{\"name\": \"agent-ways\", \"plugins\": [{}]}}", listed.join(","))).unwrap();
        let rows = components(&app, None);
        for m in attend_mod::MODS {
            let row = rows.iter().find(|c| c.name == m.label).unwrap_or_else(|| panic!("no row for the {}: {rows:?}", m.label));
            assert_eq!(row.installed.as_deref(), Some("1.53.1"));
            assert_eq!(row.plugin.as_deref(), Some(m.name));
        }
        let new = rows.iter().find(|c| c.plugin.as_deref() == Some("sparkle")).expect("a mod this binary does not know has a row");
        assert_eq!(new.name, "sparkle mod");
    }

    #[test]
    fn a_mod_new_in_the_release_is_named_for_install() {
        let dir = Path::new("/t/claude");
        let new = Component { name: "sparkle mod".into(), installed: None, target: Some("1.0.0".into()), plugin: Some("sparkle".into()), new_at_target: true, awaiting_tag: false };
        // A new ways binary is put in place: it converges the mod.
        assert_eq!(new_mod_line(dir, &new, true), Some((Mark::Act, "/t/claude: install the sparkle mod where it is wanted (new in this release)".into())));
        // The ways binary is not replaced: the old one knows nothing of the mod.
        assert_eq!(
            new_mod_line(dir, &new, false),
            Some((Mark::Warn, "/t/claude: the sparkle mod waits for a ways binary that knows it; this update keeps the current ways binary".into()))
        );
        assert_eq!(new_mod_line(dir, &Component { target: None, ..new }, true), None, "not shipped at the target");
    }

    /// Review finding 3: a file holding the retired section is the
    /// operator's to fix; the update itself is up to date, and the footer
    /// names the fix rather than `--commit`.
    #[test]
    fn retired_settings_alone_leave_the_update_up_to_date() {
        let w = WhatIf {
            app: PathBuf::from("/app"),
            channel: Channel::Ref("main".into()),
            head: Some("a".into()),
            target: Some("a".into()),
            fetch_error: None,
            relation: Relation::Same,
            components: vec![comp("ways", Some("1.1.0"), Some("1.1.0"))],
            notes: Err("offline".into()),
            delta: None,
            dropped: None,
            local: Vec::new(),
            actions: Vec::new(),
            dev: Vec::new(),
            retired: vec![Held { path: PathBuf::from("/u/config.yaml"), line: Some(1), kind: retired::Kind::User, moved: &retired::ATTEND_MOD }],
            blocked: None,
            verbose: false,
        };
        let text = render(&w).render(&Painter::plain());
        assert!(text.contains("Up to date; each file above names the `ways settings fix` command that moves its section."), "{text}");
        assert!(!text.contains("--commit` to apply"), "{text}");
    }

    fn bare(components: Vec<Component>, target: Option<&str>) -> WhatIf {
        WhatIf {
            app: PathBuf::from("/app"),
            channel: Channel::Release { upstream: Some("origin/main".into()) },
            head: Some("a".into()),
            target: target.map(str::to_string),
            fetch_error: None,
            relation: Relation::Behind,
            components,
            notes: Ok(notes::Fetched::default()),
            delta: None,
            dropped: None,
            local: Vec::new(),
            actions: Vec::new(),
            dev: Vec::new(),
            retired: Vec::new(),
            blocked: None,
            verbose: false,
        }
    }

    #[test]
    fn json_leaves_out_a_component_the_update_does_not_install() {
        let w = bare(vec![comp("ways", Some("1.0.0"), Some("1.0.0")), comp("way-embed", None, Some("1.2.0"))], Some("b"));
        let doc = json(&w);
        assert_eq!(doc["behind"], false, "way-embed is not installed by --commit: {doc}");
        assert_eq!(doc["summary"], "");
        assert_eq!(doc["components"][1], serde_json::json!({"name": "way-embed", "installed": null, "target": "1.2.0", "moves": false}));
    }

    #[test]
    fn json_counts_a_component_that_is_new_at_the_target() {
        let new = Component { new_at_target: true, ..comp("sparkle", None, Some("0.1.0")) };
        let doc = json(&bare(vec![comp("ways", Some("1.0.0"), Some("1.0.0")), new], Some("b")));
        assert_eq!(doc["behind"], true, "{doc}");
        assert_eq!(doc["summary"], "sparkle (new) → 0.1.0");
        assert_eq!(doc["components"][1]["moves"], true);
    }

    #[test]
    fn json_moves_nothing_when_no_target_resolves() {
        let w = bare(vec![comp("ways", Some("1.0.0"), Some("1.1.0"))], None);
        let doc = json(&w);
        assert_eq!(doc["behind"], serde_json::Value::Null);
        assert_eq!(doc["summary"], "");
        assert_eq!(doc["components"][0]["moves"], false, "a fallback target says nothing: {doc}");
    }

    #[test]
    fn a_fetch_failure_names_the_fatal_line_not_a_trailing_fragment() {
        let stderr = "fatal: '/x/no-such-origin' does not appear to be a git repository\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n";
        assert_eq!(failure_line(stderr), "fatal: '/x/no-such-origin' does not appear to be a git repository");
        assert_eq!(failure_line("warning: x\nerror: cannot lock ref\nhint: later\n"), "error: cannot lock ref");
        assert_eq!(failure_line("only a note\nlast line\n"), "last line");
        assert_eq!(failure_line(""), "git fetch failed");
    }

    /// The JSON check over a real app dir: a git clone of a bare origin,
    /// with a `ways` binary that reports the installed version.
    #[cfg(unix)]
    mod json_fixture {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn git(dir: &Path, args: &[&str]) {
            let out = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
                .args(args)
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }

        /// Commit the seed at `version`, tag it `ways-v<version>` and push
        /// both to origin: a cut release.
        fn release(seed: &Path, version: &str) {
            bump(seed, version);
            git(seed, &["tag", &format!("ways-v{version}")]);
            git(seed, &["push", "-q", "origin", &format!("ways-v{version}")]);
        }

        /// Commit the seed at `version` and push the branch alone: a version
        /// bump on main whose release is not cut.
        fn bump(seed: &Path, version: &str) {
            std::fs::create_dir_all(seed.join("tools/ways")).unwrap();
            std::fs::write(seed.join("tools/suite-bins"), "ways\n").unwrap();
            std::fs::write(seed.join("tools/ways/Cargo.toml"), format!("[package]\nname = \"ways\"\nversion = \"{version}\"\n")).unwrap();
            git(seed, &["add", "."]);
            git(seed, &["commit", "-q", "-m", version]);
            git(seed, &["push", "-q", "origin", "main"]);
        }

        /// Write the ways-judge mod at `version` into the seed, which a ways
        /// release bumps with ways, without committing it.
        fn write_mod(seed: &Path, version: &str) {
            std::fs::create_dir_all(seed.join("plugins/.claude-plugin")).unwrap();
            std::fs::create_dir_all(seed.join("plugins/ways-judge/.claude-plugin")).unwrap();
            std::fs::write(seed.join("plugins/.claude-plugin/marketplace.json"), r#"{"name":"agent-ways","plugins":[{"name":"ways-judge","source":"./ways-judge"}]}"#).unwrap();
            std::fs::write(seed.join("plugins/ways-judge/.claude-plugin/plugin.json"), format!("{{\"version\": \"{version}\"}}")).unwrap();
        }

        /// An app cloned from an origin at `released`, with `ways` installed
        /// at `installed`. The app is cloned before a later release, so
        /// that release is one it has not fetched.
        struct Fixture {
            _dir: crate::test_dir::TestDir,
            seed: PathBuf,
            app: PathBuf,
        }

        fn fixture(tag: &str, installed: &str, released: &str) -> Fixture {
            let dir = crate::test_dir::TestDir::at(std::env::temp_dir().join(format!("ways-whatif-json-{tag}-{}", std::process::id())));
            let (origin, seed, app) = (dir.join("origin.git"), dir.join("seed"), dir.join("app"));
            std::fs::create_dir_all(&origin).unwrap();
            std::fs::create_dir_all(&seed).unwrap();
            git(&origin, &["init", "-q", "--bare"]);
            git(&seed, &["init", "-q"]);
            git(&seed, &["remote", "add", "origin", origin.to_str().unwrap()]);
            release(&seed, released);
            git(&dir, &["clone", "-q", origin.to_str().unwrap(), "app"]);
            std::fs::create_dir_all(app.join("bin")).unwrap();
            let bin = app.join("bin/ways");
            std::fs::write(&bin, format!("#!/bin/sh\necho \"ways {installed}\"\n")).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            Fixture { seed, app, _dir: dir }
        }

        fn check(app: &Path) -> serde_json::Value {
            let channel = Channel::Release { upstream: upstream(app) };
            json(&gather(app, channel, false, None))
        }

        fn ways(doc: &serde_json::Value) -> &serde_json::Value {
            doc["components"].as_array().unwrap().iter().find(|c| c["name"] == "ways").expect("a ways component")
        }

        #[test]
        fn json_reports_an_install_behind_its_channel() {
            let f = fixture("behind", "1.0.0", "1.0.0");
            release(&f.seed, "1.1.0");
            let doc = check(&f.app);
            assert_eq!(doc["behind"], true, "{doc}");
            assert_eq!(doc["channel"], "release");
            assert_eq!(doc["blocked"], serde_json::Value::Null, "{doc}");
            assert_eq!(doc["upstream"], "origin/main");
            assert_eq!(doc["fetch_error"], serde_json::Value::Null);
            assert_eq!(doc["target"].as_str().map(str::len), Some(40), "{doc}");
            assert_eq!(doc["summary"], "ways 1.0.0 → 1.1.0");
            assert_eq!(*ways(&doc), serde_json::json!({"name": "ways", "installed": "1.0.0", "target": "1.1.0", "moves": true}));
        }

        #[test]
        fn json_reports_a_current_install_as_not_behind() {
            let f = fixture("current", "1.1.0", "1.1.0");
            let doc = check(&f.app);
            assert_eq!(doc["behind"], false, "{doc}");
            assert_eq!(doc["summary"], "");
            assert_eq!(doc["fetch_error"], serde_json::Value::Null);
            assert_eq!(*ways(&doc), serde_json::json!({"name": "ways", "installed": "1.1.0", "target": "1.1.0", "moves": false}));
        }

        /// An app that fetched 1.1.0, then lost its origin after 1.2.0 landed
        /// there unfetched: the last fetched target is 1.1.0, and HEAD (at
        /// 1.0.0) differs from it.
        fn lost_origin(tag: &str) -> Fixture {
            let f = fixture(tag, "1.0.0", "1.0.0");
            release(&f.seed, "1.1.0");
            git(&f.app, &["fetch", "-q", "origin"]);
            release(&f.seed, "1.2.0");
            git(&f.app, &["remote", "set-url", "origin", f.app.join("no-such-origin").to_str().unwrap()]);
            f
        }

        fn rev_parse(dir: &Path, name: &str) -> String {
            let out = Command::new("git").args(["rev-parse", name]).current_dir(dir).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        #[test]
        fn json_compares_against_the_last_fetched_target_when_the_fetch_fails() {
            let f = lost_origin("failed");
            let doc = check(&f.app);
            assert_eq!(doc["behind"], true, "the last fetched target still answers: {doc}");
            assert!(doc["fetch_error"].as_str().is_some_and(|e| !e.is_empty()), "{doc}");
            let fetched = rev_parse(&f.app, "origin/main");
            assert_eq!(doc["target"], fetched.as_str(), "the target is origin/main as last fetched");
            assert_ne!(fetched, rev_parse(&f.app, "HEAD"), "and not a fall back to HEAD");
            assert_eq!(*ways(&doc), serde_json::json!({"name": "ways", "installed": "1.0.0", "target": "1.1.0", "moves": true}));
        }

        #[test]
        fn a_ref_whose_fetch_fails_keeps_the_error_and_the_last_fetched_target() {
            let f = lost_origin("ref-failed");
            let w = gather(&f.app, Channel::Ref("main".into()), false, None);
            assert_eq!(w.target, Some(rev_parse(&f.app, "origin/main")), "origin/main comes before the local main");
            assert!(w.fetch_error.as_deref().is_some_and(|e| !e.is_empty()), "the error stays though the ref resolves: {:?}", w.fetch_error);
            let doc = json(&w);
            assert_eq!(doc["behind"], true, "{doc}");
            assert_eq!(doc["channel"], "ref");
            assert_eq!(doc["ref"], "main");
        }

        #[test]
        fn notes_are_fetched_for_the_report_and_left_out_for_json() {
            let f = fixture("notes", "1.0.0", "1.0.0");
            release(&f.seed, "1.1.0");
            let source = || Ok(notes::Fetched { releases: vec![Release { tag: "ways-v1.1.0".into(), body: "## ways 1.1.0\n".into() }], warning: None });
            let channel = || Channel::Release { upstream: upstream(&f.app) };
            let report = gather(&f.app, channel(), false, Some(&source));
            let checked = gather(&f.app, channel(), false, None);
            assert_eq!(report.notes.as_ref().unwrap().releases.len(), 1, "the report fetches the notes");
            assert!(checked.notes.as_ref().unwrap().releases.is_empty(), "the JSON path fetches none");
        }

        #[test]
        fn a_version_bump_waits_for_its_release_tag() {
            let f = fixture("tag", "1.0.0", "1.0.0");
            bump(&f.seed, "1.1.0");
            let doc = check(&f.app);
            assert_eq!(doc["behind"], false, "the bump is on main and its release is not cut: {doc}");
            assert_eq!(doc["summary"], "");
            assert_eq!(*ways(&doc), serde_json::json!({"name": "ways", "installed": "1.0.0", "target": "1.1.0", "moves": false}));
            git(&f.seed, &["tag", "ways-v1.1.0"]);
            git(&f.seed, &["push", "-q", "origin", "ways-v1.1.0"]);
            let doc = check(&f.app);
            assert_eq!(doc["behind"], true, "the tag is pushed: {doc}");
            assert_eq!(doc["summary"], "ways 1.0.0 → 1.1.0");
            assert_eq!(ways(&doc)["moves"], true);
        }

        #[test]
        fn a_mod_waits_for_the_release_it_ships_with() {
            let f = fixture("mod-tag", "1.0.0", "1.0.0");
            write_mod(&f.seed, "1.0.0");
            git(&f.seed, &["add", "."]);
            git(&f.seed, &["commit", "-q", "-m", "mod"]);
            git(&f.seed, &["push", "-q", "origin", "main"]);
            git(&f.app, &["pull", "-q", "--ff-only"]);
            // One release PR bumps ways and the mod together; its tag follows.
            write_mod(&f.seed, "1.1.0");
            bump(&f.seed, "1.1.0");
            let doc = check(&f.app);
            let judge = doc["components"].as_array().unwrap().iter().find(|c| c["name"] == "ways-judge mod").expect("a ways-judge row").clone();
            assert_eq!(judge["installed"], "1.0.0");
            assert_eq!(judge["target"], "1.1.0");
            assert_eq!(doc["behind"], false, "the mod bump is in the window before the tag: {doc}");
            assert_eq!(doc["summary"], "");
            assert_eq!(judge["moves"], false);
            git(&f.seed, &["tag", "ways-v1.1.0"]);
            git(&f.seed, &["push", "-q", "origin", "ways-v1.1.0"]);
            let doc = check(&f.app);
            assert_eq!(doc["behind"], true, "{doc}");
            assert_eq!(doc["summary"], "ways 1.0.0 → 1.1.0, ways-judge mod 1.0.0 → 1.1.0");
        }

        #[test]
        fn a_branch_without_an_upstream_is_blocked_and_one_never_fetched_is_not() {
            let f = fixture("no-upstream", "1.0.0", "1.0.0");
            release(&f.seed, "1.1.0");
            git(&f.app, &["branch", "--unset-upstream"]);
            let doc = check(&f.app);
            assert_eq!(doc["blocked"], "no-upstream", "{doc}");
            assert_eq!(doc["behind"], serde_json::Value::Null, "{doc}");
            assert_eq!(doc["upstream"], serde_json::Value::Null);

            // The upstream is configured and its tracking ref is not fetched
            // yet: the pull would work, and no target resolves.
            let g = fixture("never-fetched", "1.0.0", "1.0.0");
            git(&g.app, &["remote", "set-url", "origin", g.app.join("no-such-origin").to_str().unwrap()]);
            git(&g.app, &["update-ref", "-d", "refs/remotes/origin/main"]);
            let doc = check(&g.app);
            assert_eq!(doc["blocked"], serde_json::Value::Null, "{doc}");
            assert_eq!(doc["behind"], serde_json::Value::Null, "{doc}");
            assert_eq!(doc["target"], serde_json::Value::Null);
        }

        #[test]
        fn a_ref_build_needs_no_release_tag() {
            let f = fixture("ref-untagged", "1.0.0", "1.0.0");
            bump(&f.seed, "1.1.0");
            let doc = json(&gather(&f.app, Channel::Ref("main".into()), false, None));
            assert_eq!(doc["behind"], true, "a --ref update builds from source: {doc}");
            assert_eq!(doc["blocked"], serde_json::Value::Null);
        }

        #[test]
        fn a_detached_checkout_is_blocked_and_behind_is_unknown() {
            let f = fixture("detached", "1.0.0", "1.0.0");
            release(&f.seed, "1.1.0");
            git(&f.app, &["checkout", "-q", "--detach"]);
            let doc = check(&f.app);
            assert_eq!(doc["blocked"], "detached", "{doc}");
            assert_eq!(doc["behind"], serde_json::Value::Null, "{doc}");
            assert_eq!(doc["upstream"], serde_json::Value::Null);
            assert_eq!(ways(&doc)["installed"], "1.0.0");
            assert_eq!(ways(&doc)["target"], "1.1.0");
            assert_eq!(ways(&doc)["moves"], false, "nothing moves while the pull is blocked: {doc}");
            assert_eq!(doc["summary"], "");
        }

        #[test]
        fn a_diverged_checkout_is_blocked_and_behind_is_unknown() {
            let f = fixture("diverged", "1.0.0", "1.0.0");
            git(&f.app, &["commit", "-q", "--allow-empty", "-m", "local"]);
            release(&f.seed, "1.1.0");
            let doc = check(&f.app);
            assert_eq!(doc["blocked"], "diverged", "{doc}");
            assert_eq!(doc["behind"], serde_json::Value::Null, "{doc}");
            assert_eq!(ways(&doc)["target"], "1.1.0");
            assert_eq!(ways(&doc)["moves"], false, "{doc}");
            assert_eq!(doc["summary"], "");
        }

        #[test]
        fn json_reports_unknown_when_no_target_resolves() {
            let f = fixture("untargeted", "1.0.0", "1.1.0");
            git(&f.app, &["remote", "set-url", "origin", f.app.join("no-such-origin").to_str().unwrap()]);
            git(&f.app, &["update-ref", "-d", "refs/remotes/origin/main"]);
            let doc = check(&f.app);
            assert_eq!(doc["behind"], serde_json::Value::Null, "no target is unknown, not false: {doc}");
            assert_eq!(doc["target"], serde_json::Value::Null);
            assert!(doc["fetch_error"].as_str().is_some_and(|e| !e.is_empty()), "{doc}");
        }
    }
}
