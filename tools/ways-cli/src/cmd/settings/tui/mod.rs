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
pub mod flows;
pub mod others;
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

use super::{fail, live_layers, lookup, project_dir, registry, target_file, write_file, write_file_checked, Failure, Out};
use build::{display, tilde, TABS};
use others::Other;

/// Where the screens look: the project, the home directory paths are shown
/// under, the corpus whose ways get per-project toggles, and the places the
/// guided flows search.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub project: PathBuf,
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
}

impl Ctx {
    pub fn from_env(project: Option<&Path>) -> Ctx {
        let home = claude_sessions::home_dir();
        let xdg_config = ways_core::paths::xdg_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        Ctx {
            project: project_dir(project),
            corpus: ways_core::paths::core_ways_root(),
            user_ways: ways_core::paths::user_ways_root(),
            themes: agent_theme::user_dir(),
            claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from),
            claude: ways_core::paths::claude_dir().root().to_path_buf(),
            xdg_config,
            home,
        }
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
}

impl Ways {
    pub fn new(ctx: Ctx) -> Ways {
        Ways { ctx, reg: registry(), watched: RefCell::default(), all_projects: Cell::new(false), others: RefCell::default() }
    }

    pub fn layers(&self) -> Vec<Layer> {
        let layers = live_layers(&self.ctx.project);
        let mut paths: Vec<PathBuf> = layers.iter().filter_map(|l| l.path.clone()).collect();
        paths.push(ways_agent_core::profile::user_layer_path());
        paths.push(ways_core::paths::config_root().join("keys"));
        // The cached model lists the model keys offer: `ways agent models`
        // run while the screen is open reloads the picker.
        paths.extend(model_caches());
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
    fn value(&self, key: &str, layers: &[Layer]) -> Option<Value> {
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
        let v = b.spec.parse_cli(text, &live_layers(&self.ctx.project), &b.bound).map_err(|m| format!("{key}: {m}"))?;
        let (path, _) = target_file(&b, None).map_err(|f| f.message)?;
        self.refuse_broken(&path)?;
        write_file(&path, &[(b.path(), v)]).map(|_| ()).map_err(|f| self.short(&f.message))
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
            live_layers(project)
        } else {
            vec![Layer::read(&ways_core::settings::SCHEMA, "project", ways_core::settings::FILE, agent_settings::LayerScope::Project, file)]
        };
        for l in layers.iter_mut().filter(|l| l.path.as_deref() == Some(file)) {
            *l = Layer::from_text(super::schema_of(l.file), &l.name.clone(), l.file, l.scope, Some(file), &text);
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
        Some(b.spec.parse_cli(text, &live_layers(&self.ctx.project), &b.bound).map(|v| display(Some(&v), b.spec.kind)))
    }

    /// One locked edit of `file`, each value parsed by the schema exactly as
    /// `ways settings set` parses its argument. A file that does not parse,
    /// or a key that changed on disk since it was read, takes no write.
    fn write(&mut self, file: &Path, values: &[Write]) -> Result<(), String> {
        self.refuse_broken(file)?;
        // A project-scope key may be another project's, from the all
        // projects view: one of the projects listed there, whose file it
        // names.
        let mut project = self.ctx.project.clone();
        let mut edits = Vec::new();
        let layers = live_layers(&self.ctx.project);
        for w in values {
            let b = lookup(&self.reg, &w.store.key).map_err(|f| f.message)?;
            let v = b.spec.parse_cli(w.value, &layers, &b.bound).map_err(|m| format!("{}: {m}", w.store.key))?;
            let at = |p: &Path| target_file(&b, (b.spec.scope == Scope::Project).then_some(p)).map(|t| t.0).map_err(|f| f.message);
            let mut path = at(&self.ctx.project)?;
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
            edits.push((b.path(), v));
        }
        // The wait for the lock is bounded: a writer that hangs holding it
        // must not freeze the screen's keys and signals.
        match write_file_checked(file, &edits, Some(LOCK_WAIT), |doc| self.unchanged(file, &project, doc, values)) {
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
        // a project ways: the other projects are found again.
        self.forget_others();
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
            if let Err(e) = pipe.write_all(secret.reveal().as_bytes()) {
                // Never leave the command, or anything it started, behind.
                proc.stop();
                return ended(Err(format!("{line}: writing its stdin: {e}")));
            }
        }
        Box::new(proc)
    }

    fn reload(&mut self) -> Option<Vec<Node>> {
        Some(self.roots())
    }

    /// The size and time of each file the tree was read from, and the names
    /// in the keys directory: metadata only, nothing parsed.
    fn stamp(&self) -> Option<u64> {
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
        Some(agent_identity::identity::fnv1a_64(&seen))
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

    /// The ways tab names the view it shows.
    fn title(&self, tab: &str) -> Option<String> {
        (tab == "ways").then(|| title(&self.ctx, Some(if self.all_projects.get() { "all projects" } else { "this project" })))
    }

    fn help(&self, tab: &str) -> Option<String> {
        super::help_text(Some(tab)).ok()
    }

    fn choose_theme(&mut self, name: &str) -> Result<(), String> {
        self.set_one("theme.active", name)
    }

    fn choose_shape(&mut self, name: &str) -> Result<(), String> {
        self.set_one("theme.shape", name)
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

/// `ways target plan <dir>`, read-only, from this binary.
fn run_plan(dir: &Path) -> String {
    let Ok(exe) = std::env::current_exe() else { return "could not locate the ways binary".into() };
    match Command::new(exe).args(["target", "plan"]).arg(dir).env("NO_COLOR", "1").output() {
        // A blocked plan exits non-zero and still prints everything.
        Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
        Err(e) => format!("could not run `ways target plan`: {e}"),
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
    /// Key tokens to feed the real key handler, headless (`agent_tui::testkit::parse_keys`).
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

/// The app over the live files, opened on `tab`.
/// The tree pane's title: the project, and the view a tab names.
fn title(ctx: &Ctx, view: Option<&str>) -> String {
    let view = view.map(|v| format!(" · {v}")).unwrap_or_default();
    format!(" ways settings — {}{view} ", tilde(&ctx.project, &ctx.home))
}

pub fn app(ways: Ways, tab: Option<&str>, depth: ColorDepth) -> Result<App, Failure> {
    let layers = ways.layers();
    let roots = ways.build(&layers);
    let active = ways.value("theme.active", &layers).and_then(|v| v.as_str().map(str::to_string));
    let shape = ways.value("theme.shape", &layers).and_then(|v| v.as_str().map(Shape::named)).unwrap_or(Shape::PLAIN);
    let themes = Themes::new(ways.ctx.themes.clone(), depth, active).home(ways.ctx.home.clone());
    let title = title(&ways.ctx, None);
    let mut names: Vec<&str> = TABS.iter().map(|t| t.name).collect();
    names.push("theme");
    let at = match tab {
        None => 0,
        Some(t) => names.iter().position(|n| *n == t).ok_or_else(|| fail(exit::USAGE, format!("no tab {t}; the tabs are {}", names.join(", "))))?,
    };
    Ok(App::new(title, roots).shape(shape).themes(themes).adapter(ways).on_tab(at))
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
    let keys = agent_tui::testkit::parse_keys(o.keys.iter().flat_map(|k| k.split_whitespace())).map_err(|e| fail(exit::USAGE, format!("--keys: {e}")))?;
    let size = match &o.snap {
        Some(size) => Some(
            size.split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
                .filter(|(w, h)| *w > 0 && *h > 0)
                .ok_or_else(|| fail(exit::USAGE, format!("--snap {size}: WIDTHxHEIGHT, such as 100x30")))?,
        ),
        None => None,
    };
    for k in keys {
        // A frame before each key, as a terminal draws one before it reads
        // the next: what a key does can depend on what was drawn, such as
        // how far a modal's text scrolls, so a snapshot shows what the
        // terminal would.
        if let Some((w, h)) = size {
            let _ = agent_tui::testkit::render(&mut app, w, h);
        }
        // A secret never comes from an argument: it would sit in argv and
        // the process list. A key script stops at a masked entry.
        if app.masked() && matches!(k.code, agent_tui::ratatui::crossterm::event::KeyCode::Char(_)) {
            return Err(fail(exit::USAGE, "--keys cannot type into a masked entry: a secret is never an argument"));
        }
        if !app.key(k) {
            break;
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
