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
use super::{classify_build_groups, exe, git_head, installed_version, package_field, suite_bins, version_older, way_embed_source_version};
use crate::cmd::attend_mod;
use crate::cmd::lifecycle::{self, DevLink, Mark, Report};
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
}

impl Component {
    pub fn moves(&self) -> bool {
        match (&self.installed, &self.target) {
            (Some(i), Some(t)) => version_older(i, t),
            (None, Some(_)) => true,
            _ => false,
        }
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
    pub verbose: bool,
}

/// The name of the attend mod's row.
const MOD: &str = attend_mod::ATTEND.label;
/// The name of the ways-judge mod's row.
const JUDGE_MOD: &str = attend_mod::JUDGE.label;

/// True for a mod's row: a plugin, not a binary the update builds.
fn is_mod(name: &str) -> bool {
    name == MOD || name == JUDGE_MOD
}

/// Gather the what-if for `channel`. Runs a fetch and reads; writes nothing
/// else.
pub fn gather(app: &Path, channel: Channel, verbose: bool) -> WhatIf {
    let head = git_head(app);
    let (target, fetch_error) = resolve_target(app, &channel);
    let target_rev = target.clone().or_else(|| head.clone());
    let components = components(app, target_rev.as_deref());
    let moving = components.iter().any(Component::moves);
    let notes = if moving { notes::fetch() } else { Ok(notes::Fetched::default()) };
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
    WhatIf { app: app.to_path_buf(), channel, head, target, fetch_error, relation, components, notes, delta, dropped, local, actions, dev, verbose }
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
                // everything and resolve it here. The first error matters
                // only when the ref still does not resolve.
                let err = fetch(app, &["fetch", "--quiet", "--tags", "origin"]);
                let found = rev(app, r).or_else(|| rev(app, &format!("origin/{r}")));
                let err = if found.is_some() { None } else { err.or(Some(e)) };
                (found, err)
            }
        },
    }
}

/// Run a fetch; `Some(why)` when it failed.
fn fetch(app: &Path, args: &[&str]) -> Option<String> {
    match Command::new("git").args(args).current_dir(app).env("GIT_TERMINAL_PROMPT", "0").output() {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("git fetch failed").trim().to_string()),
        Err(e) => Some(format!("git fetch could not run ({e})")),
    }
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

/// The suite at `rev`, the installed suite and way-embed, and the attend
/// mod, with their versions on each side.
fn components(app: &Path, rev: Option<&str>) -> Vec<Component> {
    let target_versions = rev.map(|r| versions_at(app, r)).unwrap_or_default();
    let mut names = suite_bins(app);
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
            Component { name, installed, target }
        })
        .collect();
    let embed_installed = crate::paths::way_embed_in(&crate::paths::corpus_dir()).and_then(|b| installed_version(&b));
    let embed_target = match rev {
        Some(r) => show(app, r, "tools/way-embed/way-embed.cpp").and_then(|src| define_version(&src)),
        None => way_embed_source_version(app),
    };
    out.push(Component { name: "way-embed".into(), installed: embed_installed, target: embed_target });
    let mod_version = |text: Option<String>| -> Option<String> {
        let json: serde_json::Value = serde_json::from_str(&text?).ok()?;
        json.get("version")?.as_str().map(str::to_string)
    };
    for (m, row) in [(attend_mod::ATTEND, MOD), (attend_mod::JUDGE, JUDGE_MOD)] {
        let plugin = format!("plugins/{}/.claude-plugin/plugin.json", m.name);
        let c = Component {
            name: row.into(),
            installed: mod_version(std::fs::read_to_string(app.join(&plugin)).ok()),
            target: mod_version(rev.and_then(|r| show(app, r, &plugin))),
        };
        // An app that ships no ways-judge mod on either side has no row for it.
        if row == MOD || c.installed.is_some() || c.target.is_some() {
            out.push(c);
        }
    }
    out
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
            let (wt_cargo, wt_embed) = classify_build_groups(local.iter().filter_map(|l| porcelain_path(l)));
            cargo_changed |= wt_cargo;
            embed_changed |= wt_embed;
            let mut any = false;
            for c in comps.iter().filter(|c| !is_mod(&c.name) && c.name != "way-embed") {
                if !(cargo_changed || c.moves()) {
                    continue;
                }
                any = true;
                out.push(binary_action(c, releases, cargo));
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
    let attend = comps.iter().find(|c| c.name == MOD);
    let judge = comps.iter().find(|c| c.name == JUDGE_MOD);
    for t in crate::config::global().targets() {
        let dir = t.dir();
        if !t.enabled {
            out.push((Mark::Info, format!("{}: disabled, so repair keeps it withdrawn", dir.display())));
            continue;
        }
        out.push((Mark::Act, format!("reproject {} (ways manage repair)", dir.display())));
        if ways_core::settings::attend_mod_install(&t.config_path()) {
            out.extend(mod_action(&dir, MOD, attend));
        }
        // Repair keeps the ways-judge mod current where the target has it;
        // `ways judge-setup` installs it.
        if attend_mod::listed_in(&attend_mod::JUDGE, &dir) {
            out.extend(mod_action(&dir, JUDGE_MOD, judge));
        } else if judge.is_some() {
            out.push((Mark::Info, format!("{}: the ways-judge mod is not installed; `ways judge-setup` installs it", dir.display())));
        }
    }
    out.push((Mark::Info, "check the judge's keys".to_string()));
    out
}

/// What repair does to an installed mod in the target at `dir`: update it
/// when its row moves, else leave it.
fn mod_action(dir: &Path, row: &str, c: Option<&Component>) -> Option<(Mark, String)> {
    let c = c?;
    Some(if c.moves() {
        (Mark::Act, format!("{}: update the {row} {} → {}", dir.display(), c.installed.as_deref().unwrap_or("none"), c.target.as_deref().unwrap_or("?")))
    } else {
        (Mark::Same, format!("{}: the {row} stays as it is", dir.display()))
    })
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

fn changed(app: &Path, a: &str, b: &str) -> (bool, bool) {
    let paths = git_lines(app, &["diff", "--name-only", a, b]);
    classify_build_groups(paths.iter().map(String::as_str))
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
            (Some(i), Some(t)) if on_ref && version_older(t, i) && !is_mod(&c.name) => r.item(Mark::Down, format!("{name}  {i} → {t} (a downgrade: --commit rebuilds it from source)")),
            (Some(i), Some(t)) if on_ref && version_older(t, i) => r.item(Mark::Down, format!("{name}  {i} → {t} (a downgrade)")),
            (Some(i), Some(t)) if i != t => r.item(Mark::Same, format!("{name}  {i} (ahead of {t}; unchanged)")),
            (Some(i), _) => r.item(Mark::Same, format!("{name}  {i} unchanged")),
            (None, Some(t)) => r.item(Mark::Add, format!("{name}  not installed → {t}")),
            (None, None) => r.item(Mark::Info, format!("{name}  not installed")),
        };
    }

    let moving: Vec<&Component> = w.components.iter().filter(|c| c.moves() && !is_mod(&c.name)).collect();
    if !moving.is_empty() || w.components.iter().any(|c| is_mod(&c.name) && c.moves()) {
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
                if w.components.iter().any(|c| c.name == MOD && c.moves()) {
                    r.muted(2, "The attend mod's changes are in attend's release notes.");
                }
                if w.components.iter().any(|c| c.name == JUDGE_MOD && c.moves()) {
                    r.muted(2, "The ways-judge mod's changes are in the ways release notes.");
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

    lifecycle::dev_links_section(&mut r, &w.dev, "The update", &crate::paths::data_root());

    let level = w.relation == Relation::Same || (!on_ref && w.relation == Relation::Ahead);
    let up_to_date = level && !w.components.iter().any(Component::moves) && w.local.is_empty() && w.dev.is_empty();
    if up_to_date {
        r.footer(Mark::Ok, "Up to date. Nothing changed.");
    } else {
        r.footer(Mark::Info, lifecycle::commit_hint(&command));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_theme::Painter;

    fn comp(name: &str, i: Option<&str>, t: Option<&str>) -> Component {
        Component { name: name.into(), installed: i.map(str::to_string), target: t.map(str::to_string) }
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
            components: vec![comp("ways", Some("1.40.0"), Some("1.41.0")), comp("attend", Some("0.28.1"), Some("0.28.1")), comp(MOD, Some("0.28.1"), Some("0.28.1"))],
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
            verbose: false,
        };
        let text = render(&w).render(&Painter::plain());
        let order: Vec<usize> = ["── Versions", "── Release notes", "── Changes", "── Local changes", "── Actions"].iter().map(|h| text.find(h).unwrap_or_else(|| panic!("{h} missing:\n{text}"))).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(text.contains("↑ ways        1.40.0 → 1.41.0"), "{text}");
        assert!(text.contains("= attend      0.28.1 unchanged"), "{text}");
        assert!(text.contains("    ways-v1.41.0\n      ### Added\n      - a thing (#5)\n"), "{text}");
        assert!(!text.contains("Pre-built"), "the CI tail is dropped");
        assert!(!text.contains("ways-v1.40.0"), "the installed release is not repeated");
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
        assert_eq!(classify_build_groups([" M tools/x/src/lib.rs", "?? tools/way-embed/y.cpp"].iter().filter_map(|l| porcelain_path(l))), (true, false));
    }

    #[test]
    fn define_version_reads_way_embeds_source() {
        assert_eq!(define_version("#define VERSION_MAJOR 1\n#define VERSION \"1.2.0\"\n").as_deref(), Some("1.2.0"));
    }

    #[test]
    fn an_installed_mod_is_updated_or_left_like_attends() {
        let dir = Path::new("/t/claude");
        let judge = comp(JUDGE_MOD, Some("1.50.0"), Some("1.51.0"));
        assert_eq!(mod_action(dir, JUDGE_MOD, Some(&judge)), Some((Mark::Act, "/t/claude: update the ways-judge mod 1.50.0 → 1.51.0".into())));
        let same = comp(JUDGE_MOD, Some("1.51.0"), Some("1.51.0"));
        assert_eq!(mod_action(dir, JUDGE_MOD, Some(&same)), Some((Mark::Same, "/t/claude: the ways-judge mod stays as it is".into())));
        let attend = comp(MOD, Some("0.28.1"), Some("0.28.4"));
        assert_eq!(mod_action(dir, MOD, Some(&attend)), Some((Mark::Act, "/t/claude: update the attend mod 0.28.1 → 0.28.4".into())));
        assert_eq!(mod_action(dir, JUDGE_MOD, None), None, "no row: the app ships no ways-judge mod");
        assert!(is_mod(MOD) && is_mod(JUDGE_MOD) && !is_mod("ways"));
    }
}
