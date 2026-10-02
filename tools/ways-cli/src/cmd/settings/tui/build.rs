//! The registry as the screens' tree: a tab per root of ways' dotted names
//! (`ways`, `matching`, `gate`, `install`) and two for attend's (`attend`,
//! and `sensors` for `attend.sensors`), each key a row with its value,
//! the layer and file it resolves from, the file a change writes, and the
//! lint findings of its file. Keys the TUI cannot set (read-only, secret)
//! carry the action commands that change them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_settings::load::{resolve, Finding, Layer};
use agent_settings::{Bound, Kind, LayerScope, Scope};
use agent_tui::tree::{self, quote, Action, Arg, Kind as TKind, Node, Setting};
use serde_yaml::Value;

use super::super::{help_text, layer_label, plain, target_file};
use super::{Ctx, Ways};

/// A settings tab: its name, the prefix of the keys it shows, and the
/// prefixes under that one another tab shows instead.
#[derive(Debug, Clone, Copy)]
pub struct Tab {
    pub name: &'static str,
    pub prefix: &'static str,
    pub skip: &'static [&'static str],
}

impl Tab {
    const fn root(name: &'static str) -> Tab {
        Tab { name, prefix: name, skip: &[] }
    }

    /// Whether a dotted name or section belongs on this tab.
    pub fn holds(&self, name: &str) -> bool {
        agent_settings::registry::under(name, self.prefix) && !self.skip.iter().any(|s| agent_settings::registry::under(name, s))
    }
}

/// The settings tabs, in order; the theme tab follows them. attend's
/// settings are two tabs beside ways' (ADR-504 §8): its governor, engagement
/// and cleanup, and its sensors.
pub const TABS: [Tab; 6] = [
    Tab::root("ways"),
    Tab::root("matching"),
    Tab::root("gate"),
    Tab::root("install"),
    Tab { name: "attend", prefix: "attend", skip: &["attend.sensors"] },
    Tab { name: "sensors", prefix: "attend.sensors", skip: &[] },
];

/// The tab named `name`.
pub fn tab_named(name: &str) -> Option<&'static Tab> {
    TABS.iter().find(|t| t.name == name)
}

/// `p` with the home directory as `~` and `/` between parts, as the
/// screens show paths.
pub fn tilde(p: &Path, home: &Path) -> String {
    let s = match p.strip_prefix(home) {
        Ok(r) if r.as_os_str().is_empty() => "~".to_string(),
        Ok(r) if !home.as_os_str().is_empty() => format!("~/{}", r.display()),
        _ => p.display().to_string(),
    };
    s.replace('\\', "/")
}

/// Every path in `text` under the home directory, written with `~`.
fn tilde_text(text: &str, home: &Path) -> String {
    let h = home.display().to_string();
    if h.is_empty() || h == "/" {
        return text.to_string();
    }
    text.replace(&h, "~").replace('\\', "/")
}

/// A value as the screens show it and as `set` reads it back: text bare,
/// a toggle as true or false, a list in brackets.
pub fn display(v: Option<&Value>, kind: Kind) -> String {
    match (v, kind) {
        (Some(Value::Mapping(m)), Kind::Toggle) => m.get("enabled").and_then(Value::as_bool).unwrap_or(true).to_string(),
        (Some(Value::Sequence(items)), Kind::List) => {
            format!("[{}]", items.iter().map(|i| plain(Some(i))).collect::<Vec<_>>().join(", "))
        }
        // No list and no default: the reader keeps its own built-in list
        // (attend's processes watch), which an empty list would replace.
        (None, Kind::List) => "(built-in)".into(),
        _ => plain(v),
    }
}

fn kind(k: Kind) -> TKind {
    match k {
        Kind::Bool | Kind::Toggle => TKind::Bool,
        Kind::Int { min, max } => TKind::Int { min, max },
        Kind::Float { min, max } => TKind::Float { min, max },
        Kind::Choice(c) => TKind::Choice(c.iter().map(|s| s.to_string()).collect()),
        Kind::Text | Kind::Path | Kind::List => TKind::Text,
        Kind::ReadOnly => TKind::ReadOnly,
        Kind::Secret => TKind::Secret,
    }
}

/// The files that do not parse, which fail closed and take no write.
pub fn broken(layers: &[Layer]) -> Vec<(PathBuf, String)> {
    layers
        .iter()
        .filter(|l| l.present)
        .flat_map(|l| l.findings.iter().filter(|f| f.is_parse_failure()).filter_map(|f| Some((f.file.clone()?, f.to_string()))))
        .collect()
}

/// The finding of `layers` that applies to the key at `path` in `section`:
/// its own value, its unit, or its whole section.
fn finding_for(layers: &[Layer], file: &str, section: &str, path: &[String]) -> Option<Finding> {
    let key = path.join(".");
    layers.iter().filter(|l| l.present && l.file == file).flat_map(|l| l.findings.iter()).find(|f| {
        f.section.as_deref() == Some(section)
            && match &f.key {
                None => true,
                Some(k) => key == *k || key.starts_with(&format!("{k}.")) || k.starts_with(&format!("{key}.")),
            }
    }).cloned()
}

/// The path of names under the tab for a key: the fixed parts of its name
/// with each wildcard's binding whole, a way id split at its `/`.
fn segments(b: &Bound) -> Vec<String> {
    let mut it = b.bound.iter();
    let mut out = Vec::new();
    for part in b.spec.name.split('.') {
        if part == "*" {
            let v = it.next().cloned().unwrap_or_default();
            if b.spec.name.starts_with("ways.project.") {
                out.extend(v.split('/').map(str::to_string));
            } else {
                out.push(v);
            }
        } else {
            out.push(part.to_string());
        }
    }
    out
}

/// Where a way's file comes from, highest precedence first.
#[derive(Debug, Clone, Copy, PartialEq)]
enum WayScope {
    Project,
    User,
    Shipped,
    /// Named in a ways.yaml, found in no root.
    Missing,
    /// Another project's, listed in the all projects view.
    Other,
}

impl WayScope {
    const ORDER: [WayScope; 4] = [WayScope::Project, WayScope::User, WayScope::Shipped, WayScope::Missing];

    fn label(self) -> &'static str {
        match self {
            WayScope::Project => "this project",
            WayScope::User => "your ways",
            WayScope::Shipped => "shipped",
            WayScope::Missing => "not found",
            WayScope::Other => "another project",
        }
    }

    fn doc(self, ctx: &Ctx) -> String {
        let home = &ctx.home;
        match self {
            WayScope::Project => format!("The ways of this project, in {}/.claude/ways/. They shadow a user or shipped way of the same id.", tilde(&ctx.project, home)),
            WayScope::User => format!("Your own ways, in {}. They survive updates and shadow a shipped way of the same id.", tilde(&ctx.user_ways, home)),
            WayScope::Shipped => "The ways agent-ways ships.".into(),
            WayScope::Missing => "Switches this project's ways.yaml names for ways no root holds any more.".into(),
            WayScope::Other => "The ways of another project Claude Code knows.".into(),
        }
    }
}

/// A way's scope and its file.
struct Located {
    scope: WayScope,
    /// The root of its scope, which the detail names its file against.
    root: PathBuf,
    /// None when the directory holds no way file sessions would read.
    file: Option<PathBuf>,
}

/// Another project Claude Code knows, with ways of its own.
pub struct Other {
    pub project: PathBuf,
    pub ways: PathBuf,
    pub ids: Vec<String>,
}

/// A name for each of `projects` under the `project` group: its directory's
/// name, else with its parent's, else its whole path, whichever first
/// differs from every other project's and from the names in `taken`.
fn project_names(projects: &[&Path], taken: &BTreeSet<String>, home: &Path) -> Vec<String> {
    let name = |p: &Path, level: usize| {
        let base = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let parent = p.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        match (level, parent) {
            (0, _) => base,
            (1, Some(parent)) => format!("{parent}/{base}"),
            _ => tilde(p, home),
        }
    };
    let mut levels = vec![0; projects.len()];
    loop {
        let names: Vec<String> = projects.iter().zip(&levels).map(|(p, l)| name(p, *l)).collect();
        let mut moved = false;
        for (i, n) in names.iter().enumerate() {
            let clash = taken.contains(n) || names.iter().enumerate().any(|(j, m)| j != i && m == n);
            if clash && levels[i] < 2 {
                levels[i] += 1;
                moved = true;
            }
        }
        if !moved {
            return names;
        }
    }
}

/// `n` and the noun for it, one or many.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A way's file, found as sessions find it: the first `.md` with
/// frontmatter in `<root>/<id>/`, whatever its name.
fn way_file(root: &Path, id: &str) -> Option<PathBuf> {
    crate::session::find_way_in_dir(&root.join(id))
}

/// What a way is, for the detail pane: its description, then the fields
/// that decide when it fires, and its macro with the first lines it runs.
fn way_about(w: &Located, home: &Path) -> String {
    let row = |k: &str, v: String| format!("{k:<11}{v}");
    let Some(file) = &w.file else {
        return format!("No way file in this way's directory, so sessions skip it.\n\n{}", row("from", w.scope.label().into()));
    };
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let field = |name: &str| ways_core::frontmatter::field_in(&text, name).filter(|v| !v.is_empty());
    let mut out = vec![field("description").unwrap_or_else(|| "(no description)".into()), String::new()];
    out.push(row("from", w.scope.label().into()));
    out.push(row("root", tilde(&w.root, home)));
    out.push(row("file", file.strip_prefix(&w.root).unwrap_or(file).display().to_string()));
    for k in ["vocabulary", "pattern", "files", "commands", "trigger", "scope", "refire"] {
        if let Some(v) = field(k) {
            out.push(row(k, v));
        }
    }
    if let Some(m) = field("macro") {
        let script = file.with_file_name("macro.sh");
        out.push(row("macro", format!("{m} · macro.sh")));
        let body = std::fs::read_to_string(&script).unwrap_or_default();
        let runs = body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).take(3);
        out.extend(runs.map(|l| row("", l.to_string())));
    }
    out.join("\n")
}

/// Each group under `n` sums up its switches, as the files were read:
/// how many ways, how many off. Only a row with a store is a switch.
/// Returns `(ways, off)` for `n`.
fn summarize(n: &mut Node) -> (usize, usize) {
    let own = n.setting.as_ref().filter(|s| s.store.is_some()).map_or((0, 0), |s| (1, usize::from(s.loaded == "false")));
    if n.children.is_empty() {
        return own;
    }
    let (mut ways, mut off) = own;
    for c in &mut n.children {
        let (w, o) = summarize(c);
        ways += w;
        off += o;
    }
    if n.about.is_empty() {
        let noun = if ways == 1 { "way" } else { "ways" };
        n.about_title = "ways".into();
        n.about = format!("{ways} {noun}, {off} switched off as loaded.");
    }
    (ways, off)
}

/// Put `leaf` at `path` under `root`, making the groups between. A node
/// already there keeps its children and takes the leaf's setting: a way
/// with ways under it is both.
fn insert(root: &mut Node, path: &[String], leaf: Node, docs: &dyn Fn(&str) -> String, prefix: &str) {
    let (first, rest) = path.split_first().expect("a key has a name");
    let name = format!("{prefix}.{first}");
    let at = match root.children.iter().position(|c| c.name == *first) {
        Some(i) => i,
        None => {
            root.children.push(Node::group(first.clone(), docs(&name), vec![]));
            root.children.len() - 1
        }
    };
    let node = &mut root.children[at];
    if rest.is_empty() {
        let children = std::mem::take(&mut node.children);
        let open = node.open;
        *node = leaf;
        node.children.extend(children);
        node.open |= open;
    } else {
        insert(node, rest, leaf, docs, &name);
    }
}

impl Ways {
    /// The keys of a tab: every concrete key under it, and a toggle for each
    /// way of the corpus on the ways tab.
    fn keys_of(&self, tab: &Tab, layers: &[Layer], scopes: &BTreeMap<String, Located>) -> Vec<Bound> {
        let mut keys = self.reg.concrete(tab.prefix, layers);
        keys.retain(|b| tab.holds(&b.name()));
        if tab.name == "ways" {
            if let Some(b) = self.reg.lookup("ways.project.x") {
                // The ways a file names and the ways of the corpus, as one
                // sorted list: the order is the ids', whatever a file holds,
                // so setting a toggle never moves a row.
                let project = |k: &Bound| k.spec.name == b.spec.name;
                let mut ids: BTreeSet<String> = keys.iter().filter(|k| project(k)).filter_map(|k| k.bound.first().cloned()).collect();
                ids.extend(scopes.keys().cloned());
                let at = keys.iter().position(project).unwrap_or(keys.len());
                keys.retain(|k| !project(k));
                let ways: Vec<Bound> = ids.into_iter().map(|id| Bound { bound: vec![id], ..b.clone() }).collect();
                keys.splice(at.min(keys.len())..at.min(keys.len()), ways);
            }
        }
        keys
    }

    /// The tree: one root per tab.
    pub fn build(&self, layers: &[Layer]) -> Vec<Node> {
        TABS.iter().map(|tab| self.tab(tab, layers)).collect()
    }

    /// The doc of a group: its section's, or for one of attend's sensors, a
    /// line on that sensor.
    fn section_doc(&self, name: &str) -> String {
        if let Some(s) = name.strip_prefix("attend.sensors.").filter(|s| !s.contains('.')) {
            return attend_config::schema::sensor_doc(s);
        }
        self.reg.section(name).map(|(_, s)| s.doc.to_string()).unwrap_or_default()
    }

    fn tab(&self, tab: &Tab, layers: &[Layer]) -> Node {
        let home = &self.ctx.home;
        let broken = broken(layers);
        let mut root = Node::group(tab.name, help_text(Some(tab.name)).unwrap_or_default(), vec![]).opened();
        let docs = |name: &str| self.section_doc(name);
        let mut files: BTreeSet<&'static str> = BTreeSet::new();
        // The parts of a name the tab itself stands for.
        let depth = tab.prefix.split('.').count();
        let scopes = if tab.name == "ways" { self.way_scopes() } else { BTreeMap::new() };
        for b in self.keys_of(tab, layers, &scopes) {
            files.insert(b.spec.file);
            let segs = segments(&b);
            let rest = &segs[depth.min(segs.len())..];
            if rest.is_empty() {
                continue;
            }
            if b.spec.name == "install.targets" {
                insert(&mut root, rest, self.targets(&b, layers), &docs, tab.prefix);
                continue;
            }
            let r = resolve(b.spec, &b.bound, layers);
            let (layer, file) = layer_label(&r, layers);
            let source = match file {
                Some(f) => format!("{layer} · {}", tilde(Path::new(&f), home)),
                None => layer,
            };
            let mut s = Setting::new(kind(b.spec.kind), display(r.value.as_ref(), b.spec.kind), source);
            if let Some(d) = r.default.as_ref().or(b.spec.default_for(&b.bound).as_ref()) {
                s = s.default(display(Some(d), b.spec.kind));
            }
            let settable = !matches!(b.spec.kind, Kind::ReadOnly | Kind::Secret) && b.spec.computed.is_none();
            if settable {
                let project = (b.spec.scope == Scope::Project).then_some(self.ctx.project.as_path());
                if let Ok((path, scope)) = target_file(&b, project) {
                    let layer = if scope == agent_settings::LayerScope::Project { "project" } else { "user" };
                    if broken.iter().any(|(f, _)| *f == path) {
                        s = s.lock(format!(
                            "{} does not parse, so it fails closed: it sets nothing and takes no write until its syntax is fixed by hand",
                            tilde(&path, home)
                        ));
                    }
                    let shown = tilde(&path, home);
                    s = s.store(layer, path, b.name()).shown_as(shown);
                }
            }
            let doc = match b.spec.long {
                "" => b.spec.doc.to_string(),
                long => format!("{}\n\n{long}", b.spec.doc),
            };
            let mut node = Node::leaf(rest.last().cloned().unwrap_or_default(), doc, s).with_actions(self.actions_for(&b));
            if let Some(f) = finding_for(layers, b.spec.file, b.spec.section, &b.path()) {
                node = node.with_finding(tilde_text(&f.to_string(), home));
            }
            if b.spec.name == "ways.project.*" {
                // A way's switch sits under the scope its file comes from.
                let id = b.bound.first().cloned().unwrap_or_default();
                let found = scopes.get(&id);
                let scope = found.map_or(WayScope::Missing, |w| w.scope);
                if let Some(w) = found {
                    node = node.about("way", way_about(w, home));
                }
                let mut at = rest.to_vec();
                at.insert(1, scope.label().to_string());
                insert(&mut root, &at, node, &docs, tab.prefix);
                continue;
            }
            insert(&mut root, rest, node, &docs, tab.prefix);
        }
        if tab.name == "ways" {
            self.scope_sections(&mut root);
            self.other_projects(&mut root);
        }
        self.headers(&mut root, tab.prefix);
        let mut found = self.findings(tab, &files, layers);
        if !found.children.is_empty() {
            found.open = true;
            root.children.insert(0, found);
        }
        match tab.name {
            "ways" => root.actions = self.ways_actions(),
            "install" => {
                root.actions = vec![
                    activate(),
                    Action::new("reconcile", "ways reconcile")
                        .confirm()
                        .doc("Rewrites each recorded target's projection to match the settings: hooks, settings.json and the corpus.")
                        .touches("every recorded target directory"),
                ];
            }
            _ => {}
        }
        root
    }

    /// Every way a session here can fire, by id: the project's own, the
    /// user's, then the shipped corpus, a higher scope shadowing a lower one
    /// as `ways corpus` does (ADR-143).
    fn way_scopes(&self) -> BTreeMap<String, Located> {
        let mut out = BTreeMap::new();
        let roots = [
            (WayScope::Project, crate::cmd::ways_roots::project_ways(&self.ctx.project)),
            (WayScope::User, Some(self.ctx.user_ways.clone())),
            (WayScope::Shipped, Some(self.ctx.corpus.clone())),
        ];
        for (scope, root) in roots {
            let Some(root) = root else { continue };
            for id in crate::cmd::scan::candidates::way_ids(&root) {
                out.entry(id.clone()).or_insert_with(|| Located { scope, file: way_file(&root, &id), root: root.clone() });
            }
        }
        out
    }

    /// The ways tab's `project` group, its switches gathered by scope into
    /// sections, in precedence order, each group summing up what it holds.
    /// A project with no ways of its own gets a row saying how to add some.
    fn scope_sections(&self, root: &mut Node) {
        let Some(project) = root.children.iter_mut().find(|n| n.name == "project") else { return };
        let mut sections: Vec<Node> = Vec::new();
        for scope in WayScope::ORDER {
            let at = project.children.iter().position(|c| c.name == scope.label());
            let mut s = match at {
                Some(i) => project.children.remove(i),
                None if scope == WayScope::Project => Node::group(scope.label(), "", vec![self.no_project_ways()]),
                None => continue,
            };
            s.section = true;
            s.open = true;
            s.columns = Some((String::new(), String::new()));
            s.doc = scope.doc(&self.ctx);
            sections.push(s);
        }
        sections.extend(std::mem::take(&mut project.children));
        project.children = sections;
        summarize(project);
    }

    /// The ways tab's actions: the setup flow, and the switch between this
    /// project's ways and every known project's.
    fn ways_actions(&self) -> Vec<Action> {
        let view = if self.all_projects.get() {
            Action::new("this project only", "view: this project's ways").doc("Lists this project's ways again, beside your own and the shipped ones.")
        } else {
            Action::new("all projects", "view: every known project's ways")
                .doc("Lists the ways of every project Claude Code knows on this machine, a group per project under this one's. A switch there writes that project's .claude/ways.yaml.")
        };
        vec![
            Action::new("set up", "guided: pick a project, preview what `ways init` writes there").arg(Arg::Flow("setup".into())),
            view.arg(Arg::View("projects".into())),
        ]
    }

    /// The other projects Claude Code knows that have ways, found once per
    /// adapter. This project is left out, by its ways directory.
    pub(super) fn others(&self) -> Rc<Vec<Other>> {
        if let Some(o) = self.others.borrow().as_ref() {
            return o.clone();
        }
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let mut seen: BTreeSet<PathBuf> = crate::cmd::ways_roots::project_ways(&self.ctx.project).map(|w| canon(&w)).into_iter().collect();
        let mut out: Vec<Other> = Vec::new();
        for (project, ways) in crate::cmd::ways_roots::known_project_ways_in(&self.ctx.claude.join("projects"), &|_| {}) {
            if !seen.insert(canon(&ways)) {
                continue;
            }
            let mut ids: Vec<String> = crate::cmd::scan::candidates::way_ids(&ways).into_iter().collect();
            ids.sort();
            if !ids.is_empty() {
                // The project is the directory its ways sit in, which may be
                // above where the session started.
                let project = ways.parent().and_then(Path::parent).map_or_else(|| PathBuf::from(project), Path::to_path_buf);
                out.push(Other { project, ways, ids });
            }
        }
        out.sort_by(|a, b| a.project.cmp(&b.project));
        let out = Rc::new(out);
        *self.others.borrow_mut() = Some(out.clone());
        out
    }

    /// The ways tab's other projects, in a section after this one's scopes:
    /// in the all projects view a group per project, else a row saying what
    /// is left out.
    fn other_projects(&self, root: &mut Node) {
        let others = self.others();
        if others.is_empty() {
            return;
        }
        let at = match root.children.iter().position(|n| n.name == "project") {
            Some(i) => i,
            None => {
                root.children.push(Node::group("project", self.section_doc("ways.project"), vec![]));
                root.children.len() - 1
            }
        };
        let project = &mut root.children[at];
        let ways: usize = others.iter().map(|o| o.ids.len()).sum();
        let rows = if self.all_projects.get() {
            // A section adds nothing to its rows' keys, so its rows' names
            // are taken at this level too.
            let taken: BTreeSet<String> =
                project.children.iter().flat_map(|c| if c.section { c.children.iter().collect() } else { vec![c] }).map(|c| c.name.clone()).collect();
            let paths: Vec<&Path> = others.iter().map(|o| o.project.as_path()).collect();
            others.iter().zip(project_names(&paths, &taken, &self.ctx.home)).map(|(o, name)| self.other_project(o, name)).collect()
        } else {
            let actions = self.ways_actions();
            let key = actions.iter().zip(tree::action_keys(&actions)).find(|(a, _)| matches!(a.arg, Arg::View(_))).and_then(|(_, k)| k);
            let (shows, by) = match key {
                Some(k) => (format!(" · {k} shows them"), format!(" ({k})")),
                None => (String::new(), String::new()),
            };
            let doc = format!(
                "This view leaves out {} in {}. The all projects action{by} lists them here, a group per project; a switch there writes that project's .claude/ways.yaml.",
                count(ways, "more way", "more ways"),
                count(others.len(), "other project", "other projects"),
            );
            vec![Node::leaf("(hidden)", doc, Setting::new(TKind::ReadOnly, format!("{}{shows}", count(ways, "way", "ways")), "known projects"))]
        };
        let doc = format!("The projects Claude Code knows on this machine, besides this one, with ways of their own: {}.", count(others.len(), "project", "projects"));
        project.children.push(Node::section("other projects", doc, ("", ""), rows));
    }

    /// One other project's ways as a group: a switch per way, read from and
    /// written to that project's .claude/ways.yaml.
    fn other_project(&self, o: &Other, name: String) -> Node {
        let home = &self.ctx.home;
        let file = ways_core::settings::project_file(&o.project);
        let shown = tilde(&file, home);
        let layers = [Layer::read(&ways_core::settings::SCHEMA, "project", ways_core::settings::FILE, LayerScope::Project, &file)];
        let doc = format!("The ways of {}, in {}. A switch here writes {shown}.", tilde(&o.project, home), tilde(&o.ways, home));
        let mut g = Node::group(name, doc, vec![]).columns(("way", "enabled"));
        let Some(b) = self.reg.lookup("ways.project.x") else { return g };
        let broken = broken(&layers);
        for id in &o.ids {
            let bound = vec![id.clone()];
            let r = resolve(b.spec, &bound, &layers);
            let source = match layer_label(&r, &layers) {
                (layer, Some(f)) => format!("{layer} · {}", tilde(Path::new(&f), home)),
                (layer, None) => layer,
            };
            let mut s = Setting::new(TKind::Bool, display(r.value.as_ref(), b.spec.kind), source)
                .default("true")
                .store("project", file.clone(), format!("ways.project.{id}"))
                .shown_as(shown.clone());
            if !broken.is_empty() {
                s = s.lock(format!("{shown} does not parse, so it fails closed: it sets nothing and takes no write until its syntax is fixed by hand"));
            }
            let w = Located { scope: WayScope::Other, root: o.ways.clone(), file: way_file(&o.ways, id) };
            let leaf = Node::leaf(id.rsplit('/').next().unwrap_or(id), format!("One way on or off in {}.", tilde(&o.project, home)), s).about("way", way_about(&w, home));
            let at: Vec<String> = id.split('/').map(str::to_string).collect();
            insert(&mut g, &at, leaf, &|_| String::new(), "");
        }
        summarize(&mut g);
        g
    }

    /// The row a project without ways of its own shows: how to start.
    fn no_project_ways(&self) -> Node {
        Node::leaf(
            "(no project ways)",
            "A project's ways are markdown files at .claude/ways/<domain>/<name>/<name>.md in its root. `ways init` sets up .claude/ for a project; `ways author template` writes a way.",
            Setting::new(TKind::ReadOnly, "none · ways init", "project"),
        )
    }

    /// Header rows for a tab's sections: its loose keys gather under one
    /// `settings` section, and each top-level group names its columns as
    /// its section declares them, else `setting` and `value`.
    fn headers(&self, root: &mut Node, prefix: &str) {
        let loose = |n: &Node| n.setting.is_some() && n.children.is_empty();
        if let Some(at) = root.children.iter().position(loose) {
            let (keys, rest): (Vec<Node>, Vec<Node>) = std::mem::take(&mut root.children).into_iter().partition(loose);
            root.children = rest;
            root.children.insert(at.min(root.children.len()), Node::section("settings", "The tab's own keys.", ("", "value"), keys));
        }
        for g in root.children.iter_mut().filter(|g| !g.section && g.columns.is_none()) {
            let declared = self.reg.section(&format!("{prefix}.{}", g.name)).and_then(|(_, s)| s.columns);
            g.columns = Some(declared.map_or(("setting".into(), "value".into()), |(a, b)| (a.into(), b.into())));
        }
    }

    /// The commands that change a key the screens do not set: a provider
    /// key's add, rotate, remove and check.
    fn actions_for(&self, b: &Bound) -> Vec<Action> {
        if b.spec.kind != Kind::Secret || !b.spec.name.starts_with("gate.keys.") {
            return vec![];
        }
        let p = b.bound.first().cloned().unwrap_or_default();
        let present = b.spec.computed.map(|c| c(&b.bound)) == Some(Value::String("present".into()));
        let key = |verb: &str| format!("ways agent key {verb} --provider {p}");
        let mut out = vec![if present {
            Action::new("rotate", key("rotate")).arg(Arg::Secret).doc(format!("Replaces the stored {p} key with the one typed here, checking it first."))
        } else {
            Action::new("set", key("add")).arg(Arg::Secret).doc(format!("Stores the {p} key typed here, read from stdin, so the gate can call that provider."))
        }];
        if present {
            out.push(Action::new("remove", key("remove")).confirm().doc(format!("Deletes the stored {p} key; the gate cannot use that provider until a key is set again.")));
        }
        out.push(Action::new("check", key("check")).doc(format!("Asks {p} whether the stored key is accepted. Writes nothing.")));
        out
    }

    /// `install.targets` as a group: a row per recorded target with its
    /// enable, disable and remove, and the group's activate, add and plan.
    /// With none recorded, a row says how to start.
    fn targets(&self, b: &Bound, layers: &[Layer]) -> Node {
        let r = resolve(b.spec, &b.bound, layers);
        let (layer, _) = layer_label(&r, layers);
        let mut rows: Vec<Node> = r
            .value
            .as_ref()
            .and_then(Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(|t| {
                        let path = t.get("path")?.as_str()?.to_string();
                        let on = t.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                        let cmd = |verb: &str| format!("ways target {verb} {}", quote(&path));
                        let toggle = if on {
                            Action::new("disable", cmd("disable")).confirm().doc("Stops projecting into this directory and withdraws what was projected.").touches(path.clone())
                        } else {
                            Action::new("enable", cmd("enable")).doc("Projects agent-ways into this directory again.").touches(path.clone())
                        };
                        let remove = Action::new("remove", cmd("remove")).confirm().doc("Forgets this directory and withdraws what was projected into it.").touches(path.clone());
                        Some(
                            Node::leaf(
                                path.clone(),
                                "A Claude Code config directory agent-ways projects into (ADR-184). Enabling, disabling and removing reconcile, so they are actions.",
                                Setting::new(TKind::ReadOnly, if on { "enabled" } else { "disabled" }, layer.clone()),
                            )
                            .with_actions(vec![toggle, remove]),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let none = rows.is_empty();
        if none {
            rows.push(
                Node::leaf(
                    "(none recorded)",
                    "No target is recorded, so the default ~/.claude applies. Its activate action guides activating agent-ways in a Claude instance.",
                    Setting::new(TKind::ReadOnly, "~/.claude (default)", "default"),
                )
                .with_actions(vec![activate()]),
            );
        }
        let mut g = Node::group("targets", format!("{}\n\n{}", b.spec.doc, b.spec.long), rows).with_actions(vec![
            activate(),
            Action::new("add", "ways target add {}")
                .arg(Arg::Text("directory".into()))
                .confirm()
                .doc("Records the directory as a target and projects agent-ways into it.")
                .touches("the directory given"),
            Action::new("plan", "ways target plan {}").arg(Arg::Text("directory".into())).doc("Previews what adding the directory would write. Writes nothing."),
        ]);
        g.open = none;
        if let Some(f) = finding_for(layers, b.spec.file, b.spec.section, &b.path()) {
            g = g.with_finding(tilde_text(&f.to_string(), &self.ctx.home));
        }
        g
    }

    /// The findings of the files a tab's keys live in, as rows: those of the
    /// tab's own sections, and those of the whole file (a file that does not
    /// parse, a key no section owns). A section's finding carries `fix`.
    fn findings(&self, tab: &Tab, files: &BTreeSet<&'static str>, layers: &[Layer]) -> Node {
        let home = &self.ctx.home;
        let mut rows = Vec::new();
        for l in layers.iter().filter(|l| l.present && files.contains(l.file)) {
            for f in &l.findings {
                let mine = match &f.section {
                    Some(s) => tab.holds(s),
                    None => true,
                };
                if !mine {
                    continue;
                }
                let at = match (&f.file, f.line) {
                    (Some(p), Some(n)) => format!("{}:{n}", tilde(p, home)),
                    (Some(p), None) => tilde(p, home),
                    _ => "input".into(),
                };
                let what = if f.is_parse_failure() {
                    "does not parse: the whole file fails closed".to_string()
                } else {
                    match (&f.section, &f.key) {
                        (Some(s), Some(k)) => format!("[{s}] {k}"),
                        (Some(s), None) => format!("[{s}]"),
                        (None, Some(k)) => k.clone(),
                        (None, None) => "finding".into(),
                    }
                };
                let doc = if f.is_parse_failure() {
                    "Fix the file's syntax by hand. Until then it sets nothing, every switch in its scope is off, and nothing is written to it.".to_string()
                } else {
                    "`ways settings lint` lists the findings; `fix` repairs what this section's findings point at, and a switch stays off.".to_string()
                };
                let mut node = Node::leaf(format!("#{}", rows.len() + 1), doc, Setting::new(TKind::ReadOnly, what, at)).with_finding(tilde_text(&f.to_string(), home));
                if let (Some(s), false, None) = (&f.section, f.is_parse_failure(), &f.repair) {
                    let project = (l.scope == agent_settings::LayerScope::Project).then(|| format!(" --project {}", quote(&self.ctx.project.display().to_string())));
                    let fix = Action::new("fix", format!("ways settings fix {s}{}", project.unwrap_or_default()))
                        .confirm()
                        .doc(format!("Repairs what the findings of {s} point at in {}; a switch keeps its closed reading.", tilde(l.path.as_deref().unwrap_or(Path::new("")), home)))
                        .touches(tilde(l.path.as_deref().unwrap_or(Path::new("")), home));
                    node = node.with_actions(vec![fix]);
                }
                rows.push(node);
            }
        }
        Node::group("findings", "What `ways settings lint` finds in the files this tab reads. A section with a finding falls through to the layers beneath; a file that does not parse fails closed.", rows)
    }
}

fn activate() -> Action {
    Action::new("activate", "guided: pick Claude config directories, preview the plan").arg(Arg::Flow("activate".into()))
}
