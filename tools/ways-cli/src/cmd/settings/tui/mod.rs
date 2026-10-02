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
#[cfg(test)]
mod tests;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use agent_settings::load::{resolve, Layer};
use agent_settings::{exit, Registry, Scope};
use agent_theme::ColorDepth;
use agent_tui::flow::Flow;
use agent_tui::theme::Shape;
use agent_tui::tree::{Node, Queued, Store};
use agent_tui::{Adapter, App, Themes, Write};
use serde_yaml::Value;

use super::{fail, live_layers, lookup, project_dir, registry, target_file, write_file, Failure, Out};
use build::{display, tilde, TABS};

/// Where the screens look: the project, the home directory paths are shown
/// under, the corpus whose ways get per-project toggles, and the places the
/// guided flows search.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub project: PathBuf,
    pub home: PathBuf,
    /// The core ways, one toggle each on the ways tab.
    pub corpus: PathBuf,
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
        let xdg_config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".config"));
        Ctx {
            project: project_dir(project),
            corpus: ways_core::paths::core_ways_root(),
            themes: agent_theme::user_dir(),
            claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from),
            claude: ways_core::paths::claude_dir().root().to_path_buf(),
            xdg_config,
            home,
        }
    }
}

/// The adapter between the registry and the shell.
pub struct Ways {
    pub ctx: Ctx,
    reg: Registry,
}

impl Ways {
    pub fn new(ctx: Ctx) -> Ways {
        Ways { ctx, reg: registry() }
    }

    pub fn layers(&self) -> Vec<Layer> {
        live_layers(&self.ctx.project)
    }

    pub fn roots(&self) -> Vec<Node> {
        self.build(&self.layers())
    }

    /// The value of a key as resolved now.
    fn value(&self, key: &str, layers: &[Layer]) -> Option<Value> {
        let b = self.reg.lookup(key)?;
        resolve(b.spec, &b.bound, layers).value
    }

    /// The files the tree reads, for the change watch.
    fn watched(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.layers().into_iter().filter_map(|l| l.path).collect();
        out.push(ways_agent_core::profile::user_layer_path());
        out.push(ways_core::paths::config_root().join("keys"));
        out
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
        let v = b.spec.parse_cli(text).map_err(|m| format!("{key}: {m}"))?;
        let (path, _) = target_file(&b, None).map_err(|f| f.message)?;
        self.refuse_broken(&path)?;
        write_file(&path, &[(b.path(), v)]).map(|_| ()).map_err(|f| self.short(&f.message))
    }

    /// A file that does not parse fails closed and takes no write from the
    /// screens; the writer would refuse it too, and this says why first.
    fn refuse_broken(&self, path: &Path) -> Result<(), String> {
        if build::broken(&self.layers()).iter().any(|(f, _)| f == path) {
            return Err(format!(
                "{} does not parse, so it fails closed and takes no write; fix its syntax by hand",
                tilde(path, &self.ctx.home)
            ));
        }
        Ok(())
    }

    /// A message with paths under home as `~`.
    fn short(&self, msg: &str) -> String {
        let h = self.ctx.home.display().to_string();
        if h.is_empty() || h == "/" {
            return msg.to_string();
        }
        msg.replace(&h, "~")
    }
}

impl Adapter for Ways {
    fn validate(&self, store: &Store, text: &str) -> Option<Result<String, String>> {
        let b = self.reg.lookup(&store.key)?;
        Some(b.spec.parse_cli(text).map(|v| display(Some(&v), b.spec.kind)))
    }

    /// One locked edit of `file`, each value parsed by the schema exactly as
    /// `ways settings set` parses its argument.
    fn write(&mut self, file: &Path, values: &[Write]) -> Result<(), String> {
        let mut edits = Vec::new();
        for w in values {
            let b = lookup(&self.reg, &w.store.key).map_err(|f| f.message)?;
            let v = b.spec.parse_cli(w.value).map_err(|m| format!("{}: {m}", w.store.key))?;
            let project = (b.spec.scope == Scope::Project).then_some(self.ctx.project.as_path());
            let (path, _) = target_file(&b, project).map_err(|f| f.message)?;
            if path != file {
                return Err(format!("{} is written to {}, not {}", w.store.key, path.display(), file.display()));
            }
            edits.push((b.path(), v));
        }
        self.refuse_broken(file)?;
        write_file(file, &edits).map(|_| ()).map_err(|f| self.short(&f.message))
    }

    /// The queued command line, run as the CLI runs it: this binary with
    /// the line's arguments, a secret on stdin.
    fn run(&mut self, q: &Queued) -> Result<(), String> {
        let line = q.command.strip_suffix(" < <stdin>").unwrap_or(&q.command);
        let argv = split(line)?;
        let Some(("ways", args)) = argv.split_first().map(|(a, rest)| (a.as_str(), rest)) else {
            return Err(format!("not a ways command: {}", q.command));
        };
        let exe = std::env::current_exe().map_err(|e| format!("locating ways: {e}"))?;
        let mut child = Command::new(exe)
            .args(args)
            .env("NO_COLOR", "1")
            .stdin(if q.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{line}: {e}"))?;
        if let (Some(secret), Some(mut pipe)) = (&q.stdin, child.stdin.take()) {
            pipe.write_all(secret.reveal().as_bytes()).map_err(|e| format!("{line}: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("{line}: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        Err(self.short(&format!("exit {}: {last}", out.status.code().unwrap_or(-1))))
    }

    fn reload(&mut self) -> Option<Vec<Node>> {
        Some(self.roots())
    }

    fn stamp(&self) -> Option<u64> {
        let mut h = DefaultHasher::new();
        for p in self.watched() {
            p.hash(&mut h);
            if let Ok(m) = std::fs::metadata(&p) {
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
            }
            if p.is_dir() {
                let mut names: Vec<String> = std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                names.sort();
                names.hash(&mut h);
            }
        }
        Some(h.finish())
    }

    fn flow(&self, name: &str) -> Option<Flow> {
        flows::flow(&self.env(), name)
    }

    fn help(&self, tab: &str) -> Option<String> {
        super::help_text(Some(tab)).ok()
    }

    fn choose_theme(&mut self, name: &str) -> Result<(), String> {
        self.set_one("theme.active", name)
    }
}

/// `ways config target plan <dir>`, read-only, from this binary.
fn run_plan(dir: &Path) -> String {
    let Ok(exe) = std::env::current_exe() else { return "could not locate the ways binary".into() };
    match Command::new(exe).args(["config", "target", "plan"]).arg(dir).env("NO_COLOR", "1").output() {
        // A blocked plan exits non-zero and still prints everything.
        Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
        Err(e) => format!("could not run `ways config target plan`: {e}"),
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
pub fn app(ways: Ways, tab: Option<&str>, depth: ColorDepth) -> Result<App, Failure> {
    let layers = ways.layers();
    let roots = ways.build(&layers);
    let active = ways.value("theme.active", &layers).and_then(|v| v.as_str().map(str::to_string));
    let shape = ways.value("theme.shape", &layers).and_then(|v| v.as_str().map(Shape::named)).unwrap_or(Shape::ROUND);
    let themes = Themes::new(ways.ctx.themes.clone(), depth, active).home(ways.ctx.home.clone());
    let title = format!(" ways settings — {} ", tilde(&ways.ctx.project, &ways.ctx.home));
    let mut names: Vec<&str> = TABS.to_vec();
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
        let left = session.summary();
        if left != "nothing pending\n" {
            print!("{left}");
        }
        return Ok(());
    }
    let keys = agent_tui::testkit::parse_keys(o.keys.iter().flat_map(|k| k.split_whitespace())).map_err(|e| fail(exit::USAGE, format!("--keys: {e}")))?;
    for k in keys {
        if !app.key(k) {
            break;
        }
        agent_tui::testkit::finish_apply(&mut app);
    }
    match &o.snap {
        Some(size) => {
            let (w, h) = size
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
                .filter(|(w, h)| *w > 0 && *h > 0)
                .ok_or_else(|| fail(exit::USAGE, format!("--snap {size}: WIDTHxHEIGHT, such as 100x30")))?;
            print!("{}", agent_tui::testkit::frame(&agent_tui::testkit::render(&mut app, w, h)));
        }
        None => print!("{}", app.summary()),
    }
    Ok(())
}
