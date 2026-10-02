//! The ways of the other projects Claude Code knows, on the ways tab: one
//! row counting them by default, a group per project in the all projects
//! view. Each switch there is read from and written to that project's
//! `.claude/ways.yaml`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_settings::load::{resolve, Layer};
use agent_settings::LayerScope;
use agent_tui::tree::{self, Arg, Kind as TKind, Node, Setting, Store};

use super::super::layer_label;
use super::build::{broken, display, insert, summarize, tilde, way_about, way_file, Located, WayScope};
use super::Ways;

/// Another project Claude Code knows, with ways of its own.
pub struct Other {
    pub project: PathBuf,
    pub ways: PathBuf,
    pub ids: Vec<String>,
}

impl Other {
    /// The file its switches write.
    pub fn file(&self) -> PathBuf {
        ways_core::settings::project_file(&self.project)
    }
}

/// A name for each of `projects` under the `project` group: its directory's
/// name, else with its parent's, else its whole path, whichever first
/// differs from every other project's and from the names in `taken`. A `.`
/// shows as `·`, so a name never adds a part to the dotted keys below it.
pub(super) fn project_names(projects: &[&Path], taken: &BTreeSet<String>, home: &Path) -> Vec<String> {
    let name = |p: &Path, level: usize| {
        let base = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let parent = p.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
        let n = match (level, parent) {
            (0, _) => base,
            (1, Some(parent)) => format!("{parent}/{base}"),
            _ => tilde(p, home),
        };
        n.replace('.', "·")
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

impl Ways {
    /// The other projects Claude Code knows that have ways. This project is
    /// left out, by its ways directory. The first ways-tab build resolves
    /// every project with a transcripts directory, which probes the
    /// filesystem; the result is kept until a view switch or a queued
    /// command clears it ([`Ways::forget_others`]).
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

    /// Drop the other projects found, so the next build finds them again.
    pub(super) fn forget_others(&self) {
        *self.others.borrow_mut() = None;
    }

    /// The project whose `.claude/ways.yaml` is `file`, among the others.
    pub(super) fn other_owning(&self, file: &Path) -> Option<PathBuf> {
        self.others().iter().find(|o| o.file() == file).map(|o| o.project.clone())
    }

    /// Why the narrow view would drop a pending edit: one is stored in an
    /// other project's file, which that view does not show.
    pub(super) fn narrow_refusal(&self, pending: &[&Store]) -> Option<String> {
        let others = self.others();
        let n = pending.iter().filter(|s| others.iter().any(|o| o.file() == s.file)).count();
        (n > 0).then(|| {
            format!(
                "{} to other projects' ways.yaml would be dropped; review and apply, or undo, {} first",
                count(n, "pending change", "pending changes"),
                if n == 1 { "it" } else { "them" }
            )
        })
    }

    /// The ways tab's other projects, in a section after this one's scopes:
    /// in the all projects view a group per project, by name, else a row
    /// saying what is left out.
    pub(super) fn other_projects(&self, root: &mut Node) {
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
            let mut named: Vec<(String, &Other)> = project_names(&paths, &taken, &self.ctx.home).into_iter().zip(others.iter()).collect();
            named.sort_by(|a, b| a.0.cmp(&b.0));
            named.into_iter().map(|(name, o)| self.other_project(o, name)).collect()
        } else {
            let actions = self.ways_actions();
            let key = actions.iter().zip(tree::action_keys(&actions)).find(|(a, _)| matches!(a.arg, Arg::View(_))).and_then(|(_, k)| k);
            let (shows, by) = match key {
                Some(k) => (format!(" · {k} shows them"), format!(" ({k})")),
                None => (String::new(), String::new()),
            };
            let doc = format!(
                "This view leaves out {} in {}. The projects: all action{by} lists them here, a group per project; a switch there writes that project's .claude/ways.yaml.",
                count(ways, "more way", "more ways"),
                count(others.len(), "other project", "other projects"),
            );
            let value = format!("{}{shows}", count(ways, "way", "ways"));
            vec![Node::leaf("(hidden)", doc, Setting::new(TKind::ReadOnly, value, "known projects")).headed("other projects, not shown")]
        };
        let doc = format!("The projects Claude Code knows on this machine, besides this one, with ways of their own: {}.", count(others.len(), "project", "projects"));
        project.children.push(Node::section("other projects", doc, ("", ""), rows));
    }

    /// One other project's ways as a group: a switch per way, read from and
    /// written to that project's .claude/ways.yaml.
    fn other_project(&self, o: &Other, name: String) -> Node {
        let home = &self.ctx.home;
        let file = o.file();
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
}
