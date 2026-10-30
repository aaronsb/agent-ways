//! The settings screens (ADR-504 §8): `ways settings` on a terminal, on
//! `agent-tui`. A tab for each root of the registry's names, then the theme
//! tab. They read and write only through the registry and the one writer
//! `set` uses (ADR-503 §6, §7), so a change made here leaves the same bytes
//! as the `ways settings set` it stands for; queued actions run the same
//! command lines the CLI takes. A file that does not parse fails closed and
//! takes no write from here, as from `set`.
//!
//! `build` turns the registry into the tree, `flows` holds the guided flows.

pub mod build;
pub mod card;
pub mod flows;
pub mod judge;
pub mod mods;
pub mod modread;
pub mod others;
pub mod sensors;
mod themecc;
#[cfg(test)]
mod card_tests;
#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::io::{IsTerminal, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use agent_settings::load::{resolve, Layer};
use agent_settings::{exit, Registry, Scope};
use agent_theme::ColorDepth;
use agent_tui::flow::Flow;
use agent_tui::theme::Shape;
use agent_tui::tree::{Node, Queued, Store};
use agent_tui::adapter::{Ended, Job, Printed};
use agent_tui::{Adapter, App, Themes, Write};
use serde_yaml::Value;

use super::{fail, live_layers, lookup, registry, target_file, write_file, write_file_checked, Failure, Out};
use build::{display, tilde, TABS};
use others::Other;

/// Where the screens look: the project, the home directory paths are shown
/// under, the corpus whose ways get per-project toggles, and the places the
/// guided flows search.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The project the screens show: its root, or with none the directory
    /// they were opened at.
    pub project: PathBuf,
    /// Whether `project` is a project. Without one there is no project
    /// layer, and a key written to a project's file has nowhere to go.
    pub in_project: bool,
    /// Whether the screens were opened for a project (`--project`, or
    /// `CLAUDE_PROJECT_DIR`), never the home directory. Without one, keys the
    /// screens keep per project go to the user file.
    pub for_project: bool,
    pub home: PathBuf,
    /// The core ways, one toggle each on the ways tab.
    pub corpus: PathBuf,
    /// The user's own ways (ADR-143), toggled beside the core ones.
    pub user_ways: PathBuf,
    /// User themes; none means nothing saves.
    pub themes: Option<PathBuf>,
    pub xdg_config: PathBuf,
    pub claude_config_dir: Option<PathBuf>,
    /// The Claude config directory whose projects the setup flow offers.
    pub claude: PathBuf,
    /// What the gate tab's judge section shows, read at each build of the
    /// tree; `None` leaves the section out.
    pub judge: fn() -> Option<judge::JudgeState>,
    /// What the mod tab shows of each shipped mod, read at each build of the
    /// tree; empty leaves each mod's state unread.
    pub mods: fn() -> Vec<crate::cmd::mods::Overview>,
    /// Whether `mods` runs off the screen's thread: the live one calls
    /// Claude Code, which the screens do not wait on.
    pub mods_background: bool,
    /// What attend says of its sensors, read once for the sensors tab's
    /// cards.
    pub sensors: fn() -> crate::cmd::modules::sensors::Attend,
}

/// Whether the screens write `b` to the project's file: a project-scope key;
/// when the screens were opened for a project, one the ways schema names for
/// the screens to keep per project; or, `enabled` aside, a key of a sensor
/// a project's sensor file defines, as `layers` resolve it.
pub(super) fn writes_project(b: &agent_settings::Bound, ctx: &Ctx, layers: &[Layer]) -> bool {
    b.spec.scope == Scope::Project
        || (ctx.for_project && ways_core::settings::SCREENS_WRITE_PROJECT.contains(&b.spec.name))
        || (super::is_sensor_key(b) && b.bound.first().is_some_and(|n| sensors::writes_project(layers, n, b.spec.name)))
}

/// Whether `given` (the `--project`, else `CLAUDE_PROJECT_DIR`) opens the
/// screens for a project: one is named and it is not the home directory.
pub(super) fn opened_for_project(given: Option<&Path>, home: &Path) -> bool {
    given.is_some_and(|p| !p.as_os_str().is_empty() && !super::is_home(p, home))
}

impl Ctx {
    /// The screens' context for `--project` as given: the project enclosing
    /// it, read as `ways settings list --project` reads it. A path no project
    /// encloses opens the screens with no project layer.
    pub fn from_env(given: Option<&Path>) -> Ctx {
        let home = claude_sessions::home_dir();
        let xdg_config = ways_core::paths::xdg_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let env_project = std::env::var_os("CLAUDE_PROJECT_DIR").filter(|v| !v.is_empty()).map(PathBuf::from);
        let for_project = opened_for_project(given.or(env_project.as_deref()), &home);
        let root = super::read_project(given);
        let opened_at = || given.and_then(super::real_dir).unwrap_or_else(|| PathBuf::from(crate::util::project_dir()));
        Ctx {
            in_project: root.is_some(),
            project: root.unwrap_or_else(opened_at),
            for_project,
            corpus: ways_core::paths::shipped_ways_root(),
            user_ways: ways_core::paths::user_ways_root(),
            themes: agent_theme::user_dir(),
            claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from),
            claude: ways_core::paths::claude_dir().root().to_path_buf(),
            judge: judge::JudgeState::live,
            mods: crate::cmd::mods::overview,
            mods_background: true,
            sensors: crate::cmd::modules::sensors::described,
            xdg_config,
            home,
        }
    }
}

impl Ctx {
    /// The project whose layer the screens read; `None` with no project.
    pub fn project_layer(&self) -> Option<&Path> {
        self.in_project.then_some(self.project.as_path())
    }
}

/// How long an apply waits for another writer to let go of a file.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// The adapter between the registry and the shell.
pub struct Ways {
    pub ctx: Ctx,
    reg: Registry,
    /// The files the tree was read from, for the change watch: found when
    /// the tree is built, so the watch reads metadata only.
    watched: RefCell<Vec<PathBuf>>,
    /// Whether the ways tab lists every known project's ways, not only this
    /// project's.
    all_projects: Cell<bool>,
    /// The other projects with ways, found once: finding them probes the
    /// filesystem.
    others: RefCell<Option<Rc<Vec<Other>>>>,
    /// The files each tab writes, as its title names them, found once per
    /// tab: the title is asked at every draw.
    writes: RefCell<std::collections::BTreeMap<String, String>>,
    /// The mod tab's state, read in the background when the context says so.
    mods: RefCell<modread::ModRead>,
    /// The files' stamp when the tree was last read: a reload whose files
    /// moved owes a read of the mods.
    files: Cell<Option<u64>>,
    /// A queued command ran since the last reload, which may have moved a
    /// mod.
    ran: Cell<bool>,
    /// What attend says of its sensors, asked at the first build.
    described: std::cell::OnceCell<crate::cmd::modules::sensors::Attend>,
}

/// How soon after a read starts another may: a burst of requests reads once.
const MODS_FRESH: std::time::Duration = std::time::Duration::from_secs(5);

/// How long the first build waits for the read before it shows the tab
/// reading.
const MODS_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

impl Ways {
    pub fn new(ctx: Ctx) -> Ways {
        let mods = RefCell::new(modread::ModRead::new(ctx.mods, MODS_FRESH));
        Ways {
            ctx,
            reg: registry(),
            watched: RefCell::default(),
            all_projects: Cell::new(false),
            others: RefCell::default(),
            writes: RefCell::default(),
            mods,
            files: Cell::new(None),
            ran: Cell::new(false),
            described: std::cell::OnceCell::new(),
        }
    }

    /// The same screens with another window between mod reads, for tests.
    #[cfg(test)]
    pub(super) fn with_mods_fresh(mut self, fresh: std::time::Duration) -> Ways {
        self.mods = RefCell::new(modread::ModRead::new(self.ctx.mods, fresh));
        self
    }

    /// What the mod tab shows of each mod, and where the read stands. In
    /// the background the first build waits briefly, once, for the read.
    pub(super) fn mod_states(&self) -> (Vec<crate::cmd::mods::Overview>, mods::Read) {
        if !self.ctx.mods_background {
            return ((self.ctx.mods)(), mods::Read::Done);
        }
        let mut m = self.mods.borrow_mut();
        m.pump();
        // The shell takes the stamp once this tree is handed to it, so a
        // read landing later moves the stamp and reloads the tree.
        m.wait_first(MODS_WAIT);
        m.shown()
    }

    /// What attend says of its sensors, asked once.
    pub(super) fn described(&self) -> &crate::cmd::modules::sensors::Attend {
        self.described.get_or_init(|| (self.ctx.sensors)())
    }

    /// The stamp of the files the tree was read from: metadata only.
    fn files_stamp(&self) -> u64 {
        if self.watched.borrow().is_empty() {
            let _ = self.layers();
        }
        let mut seen = Vec::new();
        for p in self.watched.borrow().iter() {
            seen.extend_from_slice(p.to_string_lossy().as_bytes());
            seen.push(0);
            if let Ok(m) = std::fs::metadata(p) {
                seen.extend_from_slice(&m.len().to_le_bytes());
                let at = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
                seen.extend_from_slice(&at.to_le_bytes());
            }
            if p.is_dir() {
                let mut names: Vec<String> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                names.sort();
                for n in names {
                    seen.extend_from_slice(n.as_bytes());
                    seen.push(0);
                }
            }
        }
        agent_identity::identity::fnv1a_64(&seen)
    }

    pub fn layers(&self) -> Vec<Layer> {
        let layers = live_layers(self.ctx.project_layer());
        let mut paths: Vec<PathBuf> = layers.iter().filter_map(|l| l.path.clone()).collect();
        paths.push(ways_agent_core::profile::user_layer_path());
        paths.push(ways_core::paths::config_root().join("keys"));
        // The cached model lists the model keys offer: `ways agent models`
        // run while the screen is open reloads the picker.
        paths.extend(model_caches());
        paths.extend(sensors::dirs(self.ctx.project_layer()));
        if self.all_projects.get() {
            paths.extend(self.others().iter().map(|o| ways_core::settings::project_file(&o.project)));
        }
        *self.watched.borrow_mut() = paths;
        layers
    }

    pub fn roots(&self) -> Vec<Node> {
        self.build(&self.layers())
    }

    /// The value of a key as resolved now.
    pub(crate) fn value(&self, key: &str, layers: &[Layer]) -> Option<Value> {
        let b = self.reg.lookup(key)?;
        resolve(b.spec, &b.bound, layers).value
    }

    /// The flow environment: discovery roots, the recorded targets, and the
    /// real read-only plan.
    fn env(&self) -> flows::Env {
        let targets = self
            .value("install.targets", &self.layers())
            .and_then(|v| v.as_sequence().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|t| Some((t.get("path")?.as_str()?.to_string(), t.get("enabled").and_then(Value::as_bool).unwrap_or(true))))
            .collect();
        flows::Env {
            home: self.ctx.home.clone(),
            xdg_config: self.ctx.xdg_config.clone(),
            claude_config_dir: self.ctx.claude_config_dir.clone(),
            targets,
            claude: self.ctx.claude.clone(),
            project: self.ctx.project.clone(),
            plan: Rc::new(run_plan),
        }
    }

    /// Write `key = text` as `set` would: parsed by the schema, to the file
    /// the key is written to.
    fn set_one(&self, key: &str, text: &str) -> Result<(), String> {
        let b = lookup(&self.reg, key).map_err(|f| f.message)?;
        let v = b.spec.parse_cli(text, &live_layers(self.ctx.project_layer()), &b.bound).map_err(|m| format!("{key}: {m}"))?;
        let (path, _) = target_file(&b, None).map_err(|f| f.message)?;
        self.refuse_broken(&path)?;
        write_file(&path, super::is_sensor_key(&b), &[(super::file_key(&b), v)]).map(|_| ()).map_err(|f| self.short(&f.message))
    }

    /// A file that does not parse fails closed and takes no write from the
    /// screens; the writer would refuse it too, and this says why first.
    /// The file is read as it is now, not as the tree last saw it.
    fn refuse_broken(&self, path: &Path) -> Result<(), String> {
        let Ok(bytes) = std::fs::read(path) else { return Ok(()) };
        if agent_settings::load::parse_text(&String::from_utf8_lossy(&bytes), Some(path)).is_err() {
            return Err(format!(
                "{} does not parse, so it fails closed and takes no write; fix its syntax by hand",
                tilde(path, &self.ctx.home)
            ));
        }
        Ok(())
    }

    /// Check, under the writer's lock, that each key still resolves to the
    /// value the tree read before it was edited, with `file` as `doc` now
    /// holds it. A difference is an outside change, which a write would
    /// overwrite unseen; it is refused, naming the key and both values.
    fn unchanged(&self, file: &Path, project: &Path, doc: &agent_settings::yaml_edit::Doc, values: &[Write]) -> Result<(), String> {
        let text = doc.text();
        // Another project's switch was read from its own file alone.
        let mut layers = if project == self.ctx.project {
            live_layers(self.ctx.project_layer())
        } else {
            vec![Layer::read(&ways_core::settings::SCHEMA, "project", ways_core::settings::FILE, agent_settings::LayerScope::Project, file)]
        };
        for l in layers.iter_mut().filter(|l| l.path.as_deref() == Some(file)) {
            *l = match attend_config::sensors::file_at(file) {
                Some((name, origin, _)) if l.file == attend_config::schema::SENSOR_FILE => attend_config::sensors::file_layer(&name, origin, Some(file), &text),
                _ => Layer::from_text(super::schema_of(l.file), &l.name.clone(), l.file, l.scope, Some(file), &text),
            };
        }
        for w in values {
            let Some(b) = self.reg.lookup(&w.store.key) else { continue };
            let now = display(resolve(b.spec, &b.bound, &layers).value.as_ref(), b.spec.kind);
            if now != w.loaded {
                return Err(format!(
                    "{} is {now} on disk, not the {} it was read as: it changed since. Review shows it now; apply again to write {}",
                    w.store.key, w.loaded, w.value
                ));
            }
        }
        Ok(())
    }

    /// Write the engine `w` holds, and with it the judge mod's wanted state,
    /// through their one writer; then start the mod's repair. A file that
    /// does not parse, or an engine that changed on disk since it was read,
    /// takes no write.
    fn write_engine(&self, w: &Write) -> Result<(), String> {
        self.refuse_broken(&ways_agent_core::profile::user_layer_path())?;
        self.refuse_broken(&ways_core::paths::user_config())?;
        let b = lookup(&self.reg, "gate.engine").map_err(|f| f.message)?;
        let now = display(resolve(b.spec, &b.bound, &live_layers(self.ctx.project_layer())).value.as_ref(), b.spec.kind);
        if now != w.loaded {
            return Err(format!(
                "gate.engine is {now} on disk, not the {} it was read as: it changed since. Review shows it now; apply again to write {}",
                w.loaded, w.value
            ));
        }
        let engine = (!w.value.is_empty()).then_some(w.value);
        super::engine::write_engine_within(engine, Some(LOCK_WAIT)).map_err(|f| self.short(&f.message))?;
        self.repair_judge();
        Ok(())
    }

    /// `ways manage mod repair ways-judge --commit`, started in the
    /// background after an engine change, so the screen keeps drawing while
    /// Claude Code is called; the mod tab shows its result once the files
    /// change. Under the unit tests only a stand-in runner is started.
    fn repair_judge(&self) {
        if cfg!(test) && std::env::var_os("WAYS_SETTINGS_RUNNER").is_none() {
            return;
        }
        let Ok(exe) = Ways::runner() else { return };
        let child = Command::new(exe)
            .args(["manage", "mod", "repair", "ways-judge", "--commit"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut c) = child {
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
    }

    /// The binary queued commands run with: this one, or for tests the
    /// stand-in `WAYS_SETTINGS_RUNNER` names.
    fn runner() -> Result<PathBuf, String> {
        match std::env::var_os("WAYS_SETTINGS_RUNNER").filter(|v| !v.is_empty()) {
            Some(p) => Ok(PathBuf::from(p)),
            None => std::env::current_exe().map_err(|e| format!("locating ways: {e}")),
        }
    }

    /// A message with paths under home as `~`.
    fn short(&self, msg: &str) -> String {
        build::tilde_home(msg, &self.ctx.home)
    }
}

impl Adapter for Ways {
    fn validate(&self, store: &Store, text: &str) -> Option<Result<String, String>> {
        let b = self.reg.lookup(&store.key)?;
        Some(b.spec.parse_cli(text, &live_layers(self.ctx.project_layer()), &b.bound).map(|v| display(Some(&v), b.spec.kind)))
    }

    /// One locked edit of `file`, each value parsed by the schema exactly as
    /// `ways settings set` parses its argument. A file that does not parse,
    /// or a key that changed on disk since it was read, takes no write.
    fn write(&mut self, file: &Path, values: &[Write]) -> Result<(), String> {
        // gate.engine goes through the engine choice, which writes the judge
        // mod's wanted state with it (ADR-512 item 4).
        if let Some(engine) = values.iter().find(|w| w.store.key == "gate.engine") {
            let rest: Vec<Write> =
                values.iter().filter(|w| w.store.key != "gate.engine").map(|w| Write { store: w.store, value: w.value, loaded: w.loaded }).collect();
            if !rest.is_empty() {
                self.write(file, &rest)?;
            }
            return self.write_engine(engine);
        }
        self.refuse_broken(file)?;
        // A project-scope key may be another project's, from the all
        // projects view: one of the projects listed there, whose file it
        // names.
        let mut project = self.ctx.project.clone();
        let mut edits = Vec::new();
        let mut sensor = false;
        let layers = live_layers(self.ctx.project_layer());
        for w in values {
            let b = lookup(&self.reg, &w.store.key).map_err(|f| f.message)?;
            let v = b.spec.parse_cli(w.value, &layers, &b.bound).map_err(|m| format!("{}: {m}", w.store.key))?;
            let at = |p: &Path| target_file(&b, writes_project(&b, &self.ctx, &layers).then_some(p)).map(|t| t.0).map_err(|f| f.message);
            let mut path = at(&self.ctx.project)?;
            if path == file && !self.ctx.in_project && writes_project(&b, &self.ctx, &layers) {
                return Err(self.short(&format!(
                    "{} is set in a project's file, and no project encloses {}; nothing was written",
                    w.store.key,
                    self.ctx.project.display()
                )));
            }
            if path != file && b.spec.scope == Scope::Project {
                match self.other_owning(file) {
                    Some(other) => {
                        project = other;
                        path = file.to_path_buf();
                    }
                    None => {
                        return Err(self.short(&format!(
                            "{} is the ways.yaml of neither this project nor one Claude Code knows; nothing was written",
                            file.display()
                        )))
                    }
                }
            }
            if path != file {
                return Err(format!("{} is written to {}, not {}", w.store.key, path.display(), file.display()));
            }
            sensor |= super::is_sensor_key(&b);
            edits.push((super::file_key(&b), v));
        }
        // The wait for the lock is bounded: a writer that hangs holding it
        // must not freeze the screen's keys and signals.
        match write_file_checked(file, sensor, &edits, Some(LOCK_WAIT), |doc| self.unchanged(file, &project, doc, values)) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(conflict)) => Err(self.short(&conflict)),
            Err(f) => Err(self.short(&f.message)),
        }
    }

    /// The queued command, run to its end; [`Adapter::start`] is what the
    /// screens use.
    fn run(&mut self, q: &Queued) -> Result<(), String> {
        let mut job = self.start(q);
        loop {
            if let Some(r) = job.poll() {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// The queued command line, started as the CLI runs it: this binary
    /// with the line's arguments, a secret on stdin. It runs in the
    /// background; the screens poll it.
    fn start(&mut self, q: &Queued) -> Box<dyn Job> {
        // A command, such as the `ways init` the setup flow queues, may give
        // a project ways: the other projects are found again. It may also
        // move a mod, read again at the reload after it ends.
        self.forget_others();
        self.ran.set(true);
        let ended = |r: Result<(), String>| -> Box<dyn Job> { Box::new(Ended(Some(r))) };
        let line = q.command.strip_suffix(" < <stdin>").unwrap_or(&q.command).to_string();
        let argv = match split(&line) {
            Ok(a) => a,
            Err(e) => return ended(Err(e)),
        };
        let Some(("ways", args)) = argv.split_first().map(|(a, rest)| (a.as_str(), rest)) else {
            return ended(Err(format!("not a ways command: {}", q.command)));
        };
        let exe = match Ways::runner() {
            Ok(e) => e,
            Err(e) => return ended(Err(e)),
        };
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .env("NO_COLOR", "1")
            .stdin(if q.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A group of its own, so a stop ends every process it starts, not
        // only the first.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ended(Err(format!("{line}: {e}"))),
        };
        agent_tui::register_job_group(child.id());
        let read = |p: Option<Box<dyn Read + Send>>| {
            let (tx, rx) = std::sync::mpsc::channel();
            if let Some(mut r) = p {
                std::thread::spawn(move || {
                    let mut b = Vec::new();
                    let _ = r.read_to_end(&mut b);
                    let _ = tx.send(b);
                });
            }
            rx
        };
        let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let home = self.ctx.home.clone();
        let mut proc = Proc { child, out, err, home, printed: None };
        if let (Some(secret), Some(mut pipe)) = (&q.stdin, proc.child.stdin.take()) {
            // A command that exits unread gives an error here, so the screen
            // stops it and puts the terminal back rather than dying raw.
            if let Err(e) = agent_fmt::write_to_child(&mut pipe, secret.reveal().as_bytes()) {
                // Never leave the command, or anything it started, behind.
                proc.stop();
                return ended(Err(format!("{line}: writing its stdin: {e}")));
            }
        }
        Box::new(proc)
    }

    fn reload(&mut self) -> Option<Vec<Node>> {
        // A queued command or a file change may have moved a mod; a read
        // landing did not, and owes no other.
        let files = self.files_stamp();
        if self.ran.replace(false) || self.files.get().is_some_and(|f| f != files) {
            self.mods.borrow_mut().owe();
        }
        self.files.set(Some(files));
        Some(self.roots())
    }

    /// A stamp that moved with the files unchanged moved for a mod read.
    fn stamp_moved(&mut self) -> Option<String> {
        if !self.ctx.mods_background || self.files.get() != Some(self.files_stamp()) {
            return None;
        }
        Some(match self.mods.borrow().last_failed() {
            Some(why) => format!("mod state read failed: {why}"),
            None => "mod state updated".to_string(),
        })
    }

    /// The size and time of each file the tree was read from, and the names
    /// in the keys directory: metadata only, nothing parsed; and the mod
    /// reads that finished, so a landing read reloads the tree.
    fn stamp(&self) -> Option<u64> {
        let files = self.files_stamp();
        if self.files.get().is_none() {
            self.files.set(Some(files));
        }
        let mut m = self.mods.borrow_mut();
        if self.ctx.mods_background {
            m.pump();
        }
        Some(files ^ m.finished().wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }

    fn flow(&self, name: &str) -> Option<Flow> {
        flows::flow(&self.env(), name)
    }

    /// `projects` switches the ways tab between this project's ways and
    /// every known project's. Back to this project's is refused while an
    /// edit to another project's file is pending, which it would drop.
    /// Either way the other projects are found again.
    fn view(&mut self, name: &str, pending: &[&Store]) -> Result<Option<String>, String> {
        if name != "projects" {
            return Ok(None);
        }
        let all = !self.all_projects.get();
        if !all {
            if let Some(why) = self.narrow_refusal(pending) {
                return Err(why);
            }
        }
        self.forget_others();
        self.all_projects.set(all);
        Ok(Some(if all { "all projects shown" } else { "this project shown" }.into()))
    }

    /// Every tab names the files its keys write, by layer; the ways tab
    /// names the view it shows too.
    fn title(&self, tab: &str) -> Option<String> {
        let view = (tab == "ways").then_some(if self.all_projects.get() { "all projects" } else { "this project" });
        let writes = self.writes.borrow_mut().entry(tab.to_string()).or_insert_with(|| self.tab_writes(tab)).clone();
        Some(title(&self.ctx, view, &writes))
    }

    fn help(&self, tab: &str) -> Option<String> {
        super::help_text(Some(tab)).ok()
    }

    /// The critical mods that do not run, as the last read of the mods left
    /// them; asked at every draw, so it reads nothing.
    fn notice(&self) -> Vec<String> {
        let states = if self.ctx.mods_background { self.mods.borrow().shown().0 } else { (self.ctx.mods)() };
        mods::notice(&states)
    }

    fn choose_theme(&mut self, name: &str) -> Result<(), String> {
        self.set_one("theme.active", name)
    }

    fn choose_shape(&mut self, name: &str) -> Result<(), String> {
        self.set_one("theme.shape", name)
    }

    fn choose_waypoints(&mut self, name: &str) -> Result<(), String> {
        self.set_one("theme.waypoints", name)
    }

    /// The set the replay would draw for `theme` under `setting`, each glyph
    /// in its colour at the frame's depth.
    fn waypoints_preview(&self, theme: &agent_theme::Theme, setting: &str) -> Option<agent_tui::ratatui::text::Line<'static>> {
        use agent_tui::ratatui::style::Style;
        use agent_tui::ratatui::text::{Line, Span};
        let depth = agent_tui::theme::current().depth();
        let own = agent_theme::Painter::select(Some(theme), depth);
        let painter = crate::cmd::screen_host::waypoints_as(own, Some(setting));
        let set = crate::cmd::render::WaypointSet::new(painter.waypoints(), depth);
        let mut spans = Vec::new();
        for i in 0..set.len() {
            let style = set.color(i).map_or(Style::new(), |c| Style::new().fg(agent_theme::ratatui::color(c)));
            spans.push(Span::styled(set.glyph(i).to_string(), style));
            spans.push(Span::raw(" "));
        }
        Some(Line::from(spans))
    }

    fn theme_card(&self, theme: &agent_theme::Theme, source: agent_theme::Source, file: Option<&Path>, active: bool, shape: &str) -> Option<agent_tui::tree::Card> {
        let mut c = card::to_card(crate::cmd::modules::themes::item(theme, source, file, active));
        let saves_to = self.ctx.themes.as_deref().map_or("none: nothing saves".to_string(), |d| d.display().to_string());
        c.rows.extend([("saves to".into(), saves_to), ("shape".into(), format!("{shape} (a: shape cycles it)"))]);
        Some(card::shown(c, &self.ctx.home))
    }

    fn claude_code(&self) -> Option<agent_tui::ClaudeCode> {
        Some(themecc::section(self))
    }

    fn set_claude_code(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !themecc::owns(key) {
            return Err(format!("{key} is not a Claude Code render setting"));
        }
        self.set_one(key, value)
    }
}

/// A queued command running in the background, its output read on threads
/// so a full pipe never stalls it.
struct Proc {
    child: std::process::Child,
    out: std::sync::mpsc::Receiver<Vec<u8>>,
    err: std::sync::mpsc::Receiver<Vec<u8>>,
    /// Shown as `~` in the messages and in what it printed.
    home: PathBuf,
    /// What it printed, once it has ended.
    printed: Option<Printed>,
}

/// How long the output of an ended command is waited for. A process it
/// started may still hold its pipes; its output is then not waited for.
const OUTPUT_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

impl Proc {
    /// Take what the ended command printed, with paths under home as `~`.
    fn take(&mut self, code: Option<i32>) -> &Printed {
        let home = self.home.clone();
        let text = |r: &std::sync::mpsc::Receiver<Vec<u8>>| {
            let t = String::from_utf8_lossy(&r.recv_timeout(OUTPUT_WAIT).unwrap_or_default()).into_owned();
            build::tilde_home(&t, &home)
        };
        let (stdout, stderr) = (text(&self.out), text(&self.err));
        self.printed.insert(Printed { code, stdout, stderr })
    }

    /// The last line a failed command printed: on stderr, else on stdout,
    /// since some commands (`ways agent key add`) report a refusal there.
    fn reason(p: &Printed) -> String {
        let last = |t: &str| t.lines().rev().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string());
        last(&p.stderr).or_else(|| last(&p.stdout)).unwrap_or_else(|| "it printed nothing".into())
    }
}

impl Job for Proc {
    fn poll(&mut self) -> Option<Result<(), String>> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) if status.success() => {
                self.take(status.code());
                Some(Ok(()))
            }
            Ok(Some(status)) => {
                let why = Proc::reason(self.take(status.code()));
                Some(Err(match status.code() {
                    Some(c) => format!("exit {c}: {why}"),
                    None => format!("ended by a signal: {why}"),
                }))
            }
            Err(e) => Some(Err(format!("waiting on the command: {e}"))),
        }
    }

    fn printed(&mut self) -> Option<Printed> {
        self.printed.clone()
    }

    /// End the command and every process in its group, then reap it.
    fn stop(&mut self) {
        agent_tui::kill_group(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
        agent_tui::clear_job_group(self.child.id());
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        // A command still running when the screens close is ended, never
        // left behind.
        if matches!(self.child.try_wait(), Ok(None)) {
            self.stop();
        } else {
            // Ended on its own; whatever it started in its group ends too.
            agent_tui::kill_group(self.child.id());
            agent_tui::clear_job_group(self.child.id());
        }
    }
}

/// `ways manage target plan <dir>`, read-only, from this binary.
fn run_plan(dir: &Path) -> String {
    let Ok(exe) = std::env::current_exe() else { return "could not locate the ways binary".into() };
    match Command::new(exe).args(["manage", "target", "plan"]).arg(dir).env("NO_COLOR", "1").output() {
        // A blocked plan exits non-zero and still prints everything.
        Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
        Err(e) => format!("could not run `ways manage target plan`: {e}"),
    }
}

/// A command line as words, undoing the single quotes `quote` adds.
fn split(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => word.push(x),
                        None => return Err(format!("unbalanced quote in {line}")),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(x) = chars.next() {
                    word.push(x);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        out.push(word);
    }
    Ok(out)
}

/// How the screens are opened: on a terminal, or headless for a snapshot
/// and for tests of the real binary.
#[derive(Debug, Default, Clone)]
pub struct Open {
    pub tab: Option<String>,
    pub project: Option<PathBuf>,
    /// Key and mouse tokens to feed the real handlers, headless (`agent_tui::testkit::parse_events`).
    pub keys: Vec<String>,
    /// `WxH`: print the frame at that size, headless, in the test kit's format.
    pub snap: Option<String>,
    /// Colour depth to draw at: truecolor, 256, 16 or none; the terminal's by default.
    pub depth: Option<String>,
}

fn depth_of(s: Option<&str>) -> Result<ColorDepth, Failure> {
    Ok(match s {
        None => ColorDepth::detect(),
        Some("truecolor") => ColorDepth::TrueColor,
        Some("256") => ColorDepth::Ansi256,
        Some("16") => ColorDepth::Ansi16,
        Some("none") => ColorDepth::NoColor,
        Some(o) => return Err(fail(exit::USAGE, format!("--depth {o}: one of truecolor, 256, 16, none"))),
    })
}

/// The tree pane's title: the view a tab names, the layers its keys write
/// (`Ways::tab_writes`), then the project, so a narrow pane cuts the path
/// and keeps the rest: ` this project · writes project+user — ~/p `.
fn title(ctx: &Ctx, view: Option<&str>, writes: &str) -> String {
    let parts: Vec<&str> = [view.unwrap_or_default(), writes].into_iter().filter(|p| !p.is_empty()).collect();
    let head = if parts.is_empty() { "ways settings".to_string() } else { parts.join(" · ") };
    format!(" {head} — {} ", tilde(&ctx.project, &ctx.home))
}

/// The screens' tabs in order: the settings tabs, then theme. `ways
/// settings [TAB]` takes these names.
pub fn tab_names() -> Vec<&'static str> {
    TABS.iter().map(|t| t.name).chain(["theme"]).collect()
}

/// The themes on offer, with the active choice as `layers` resolve it: the
/// theme tab's state, and what `ways settings theme` acts on.
pub fn themes(ways: &Ways, layers: &[Layer], depth: ColorDepth) -> Themes {
    let active = ways.value("theme.active", layers).and_then(|v| v.as_str().map(str::to_string));
    Themes::new(ways.ctx.themes.clone(), depth, active).home(ways.ctx.home.clone())
}

pub fn app(ways: Ways, tab: Option<&str>, depth: ColorDepth) -> Result<App, Failure> {
    let layers = ways.layers();
    let roots = ways.build(&layers);
    let shape = ways.value("theme.shape", &layers).and_then(|v| v.as_str().map(Shape::named)).unwrap_or(Shape::PLAIN);
    let waypoints = ways.value("theme.waypoints", &layers).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "theme".into());
    let themes = themes(&ways, &layers, depth);
    let names = tab_names();
    let at = match tab {
        None => 0,
        Some(t) => names.iter().position(|n| *n == t).ok_or_else(|| fail(exit::USAGE, format!("no tab {t}; the tabs are {}", names.join(", "))))?,
    };
    let title = ways.title(names[at]).unwrap_or_else(|| title(&ways.ctx, None, ""));
    Ok(App::new(title, roots).shape(shape).waypoints(&waypoints).themes(themes).adapter(ways).on_tab(at))
}

/// Open the screens: on the terminal, or headless with `keys` and `snap`.
pub fn open(o: &Open) -> Out {
    let headless = !o.keys.is_empty() || o.snap.is_some();
    if !headless && !(std::io::stdout().is_terminal() && std::io::stdin().is_terminal()) {
        return Err(fail(exit::USAGE, "the settings screens need a terminal; `ways settings list` prints the values"));
    }
    let ways = Ways::new(Ctx::from_env(o.project.as_deref()));
    let depth = depth_of(o.depth.as_deref())?;
    let mut app = app(ways, o.tab.as_deref(), depth)?;
    if !headless {
        let session = agent_tui::run(app).map_err(|e| fail(exit::WRITE_FAILED, format!("terminal: {e}")))?;
        // The terminal may be gone (a hangup): a write that fails is let go,
        // where print! would panic.
        let left = session.summary();
        if left != "nothing pending\n" {
            let _ = std::io::stdout().write_all(left.as_bytes());
        }
        // The terminal is restored by now; a signal ends the process as it
        // would have, with 128 plus the signal.
        if let Some(sig) = session.signal {
            std::process::exit(128 + sig);
        }
        return Ok(());
    }
    let events = agent_tui::testkit::parse_events(o.keys.iter().flat_map(|k| k.split_whitespace())).map_err(|e| fail(exit::USAGE, format!("--keys: {e}")))?;
    let size = match &o.snap {
        Some(size) => Some(
            size.split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
                .filter(|(w, h)| *w > 0 && *h > 0)
                .ok_or_else(|| fail(exit::USAGE, format!("--snap {size}: WIDTHxHEIGHT, such as 100x30")))?,
        ),
        None => None,
    };
    use agent_tui::ratatui::crossterm::event::{Event, KeyCode};
    for e in events {
        // A frame before each event, as a terminal draws one before it
        // reads the next: what a key does can depend on what was drawn,
        // such as how far a modal's text scrolls, and a click lands on what
        // the frame put there, so a snapshot shows what the terminal would.
        if let Some((w, h)) = size {
            let _ = agent_tui::testkit::render(&mut app, w, h);
        }
        match e {
            Event::Key(k) => {
                // A secret never comes from an argument: it would sit in argv
                // and the process list. A key script stops at a masked entry.
                if app.masked() && matches!(k.code, KeyCode::Char(_)) {
                    return Err(fail(exit::USAGE, "--keys cannot type into a masked entry: a secret is never an argument"));
                }
                if !app.key(k) {
                    break;
                }
            }
            Event::Mouse(m) => app.mouse(m),
            _ => {}
        }
        agent_tui::testkit::finish_apply(&mut app);
    }
    match size {
        Some((w, h)) => print!("{}", agent_tui::testkit::frame(&agent_tui::testkit::render(&mut app, w, h))),
        None => print!("{}", app.summary()),
    }
    Ok(())
}

/// The cache file of each provider's model list.
fn model_caches() -> Vec<PathBuf> {
    let dir = ways_agent_core::models::cache_dir();
    ways_agent_core::profile::Provider::ALL.iter().map(|p| ways_agent_core::models::cache_path(&dir, *p)).collect()
}
