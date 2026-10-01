//! The ways adapter: every setting agent-ways reads, as one tree whose four
//! roots are tabs, with the layer each value came from and the file an edit
//! would write.
//!
//! Read-only. Key files are checked for existence and never opened. Actions
//! carry the real `ways` command lines; none is run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use serde_yaml::Value;

use crate::tree::{quote, Action, Arg, Kind, Node, Setting};
use crate::ui::flow::{Candidate, Flow, Out, PLine, Tone, Verb};

const SHIPPED_PROFILES: &str = include_str!("../../../ways-agent-core/profiles.yaml");

pub struct Paths {
    pub user: PathBuf,
    pub agent: PathBuf,
    pub keys: PathBuf,
    pub project: PathBuf,
    pub corpus: PathBuf,
}

impl Paths {
    pub fn resolve(project_dir: &Path) -> Paths {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let cfg = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".config")).join("agent-ways");
        Paths {
            user: cfg.join("config.yaml"),
            agent: cfg.join("agent.yaml"),
            keys: cfg.join("keys"),
            project: project_dir.join(".claude/ways.yaml"),
            corpus: home.join(".claude/hooks/ways"),
        }
    }
}

fn load(p: &Path) -> Value {
    fs::read_to_string(p).ok().and_then(|s| serde_yaml::from_str(&s).ok()).unwrap_or(Value::Null)
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// A user-scope config key: project overrides user overrides default.
/// Edits write the user file, as `ways settings set` would by default.
fn cfg(name: &str, doc: &str, kind: Kind, default: &str, user: &Value, project: &Value, p: &Paths) -> Node {
    let (value, source) = if let Some(v) = project.get(name).and_then(scalar) {
        (v, "project")
    } else if let Some(v) = user.get(name).and_then(scalar) {
        (v, "user")
    } else {
        (default.to_string(), "default")
    };
    Node::leaf(name, doc, Setting::new(kind, value, source).default(default).store(p.user.clone(), name))
}

fn prob() -> Kind {
    Kind::Float { min: 0.0, max: 1.0 }
}

pub fn build(p: &Paths, project_dir: &Path) -> Vec<Node> {
    let user = load(&p.user);
    let project = load(&p.project);
    let agent = load(&p.agent);

    vec![
        ways_tab(&user, &project, p, project_dir).opened(),
        matching(&user, &project, p).opened(),
        gate(&agent, p).opened(),
        install(&user, p).opened(),
    ]
}

fn matching(user: &Value, project: &Value, p: &Paths) -> Node {
    Node::group(
        "matching",
        "When a way fires on a prompt (ADR-156). Probabilities are calibrated, so one global bar serves every way.",
        vec![
            cfg("semantic_fire_probability", "τ_s: a way fires on its own relatedness when g(cos) reaches this.", prob(), "0.5", user, project, p),
            cfg("keyword_floor_probability", "τ_k: a pattern: hit fires only when g(cos) reaches this floor.", prob(), "0.15", user, project, p),
            cfg("parent_threshold_multiplier", "Once a parent way fired, a child's bar is multiplied by this. 1.0 disables the boost.", prob(), "0.8", user, project, p),
            cfg("parent_boost_floor", "The lowest a boosted child's bar may go.", prob(), "0.3", user, project, p),
            cfg("near_miss_margin", "Logging only: a way that missed by less than this is recorded as a near miss.", Kind::Float { min: 0.0, max: 0.5 }, "0.05", user, project, p),
            disclosure(user, project, p),
        ],
    )
}

fn disclosure(user: &Value, project: &Value, p: &Paths) -> Node {
    let presets = [("once", "1"), ("rare", "0.4"), ("normal", "0.15"), ("frequent", "0.05")];
    let up = user.get("refire_presets");
    let refire = presets
        .iter()
        .map(|(name, d)| {
            let (v, src) = match up.and_then(|m| m.get(*name)).and_then(scalar) {
                Some(v) => (v, "user"),
                None => (d.to_string(), "default"),
            };
            Node::leaf(
                *name,
                "Fraction of the session's context window before a way with this preset may fire again (ADR-126).",
                Setting::new(Kind::Float { min: 0.0, max: 2.0 }, v, src).default(*d).store(p.user.clone(), format!("refire_presets.{name}")),
            )
        })
        .collect();
    Node::group(
        "disclosure",
        "How ways reach the session once they match.",
        vec![
            cfg("default_scope", "Scope for ways that declare none.", Kind::Choice(vec!["agent".into(), "subagent".into(), "teammate".into()]), "agent", user, project, p),
            cfg("language", "Output language. auto and en keep the localization pipeline off (ADR-139).", Kind::Text, "auto", user, project, p),
            Node::group("refire_presets", "Named re-disclosure cadences a way's refire: field may use.", refire),
        ],
    )
}

/// Top-level directories of the corpus that hold ways.
fn domain_names(corpus: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(corpus)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn domains(user: &Value, p: &Paths) -> Node {
    let disabled: Vec<String> = user
        .get("disabled_domains")
        .and_then(Value::as_sequence)
        .map(|s| s.iter().filter_map(scalar).collect())
        .unwrap_or_default();
    let children = domain_names(&p.corpus)
        .into_iter()
        .map(|d| {
            let on = !disabled.contains(&d);
            let src = if on { "default" } else { "user" };
            Node::leaf(
                d.clone(),
                format!("Every way under {d}/, for every project. Today: disabled_domains in the user config."),
                Setting::new(Kind::Bool, on.to_string(), src).default("true").store(p.user.clone(), format!("disabled_domains[{d}]")),
            )
        })
        .collect();
    Node::group("domains", "Switch a whole domain of ways on or off. User-wide: it applies to every project.", children)
}

/// The `description:` line of a way file's frontmatter.
fn way_description(file: &Path) -> String {
    fs::read_to_string(file)
        .ok()
        .and_then(|s| s.lines().take(40).find_map(|l| l.strip_prefix("description:").map(|d| d.trim().trim_matches('"').to_string())))
        .unwrap_or_default()
}

/// One node per way directory: a dir holding `<name>.md`, or a dir of ways.
fn way_tree(dir: &Path, id: &str, disabled: &BTreeMap<String, bool>, p: &Paths) -> Option<Node> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let mut subdirs: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subdirs.sort();
    let children: Vec<Node> = subdirs
        .iter()
        .filter_map(|d| {
            let n = d.file_name()?.to_string_lossy().into_owned();
            way_tree(d, &format!("{id}/{n}"), disabled, p)
        })
        .collect();
    let file = dir.join(format!("{name}.md"));
    if file.is_file() {
        let off = disabled.get(id).copied() == Some(false);
        let mut n = Node::leaf(
            name,
            format!("{id}\n\n{}", way_description(&file)),
            Setting::new(Kind::Bool, (!off).to_string(), if off { "project" } else { "default" })
                .default("true")
                .store(p.project.clone(), format!("ways.{id}")),
        );
        n.children = children;
        Some(n)
    } else if children.is_empty() {
        None
    } else {
        Some(Node::group(name, format!("{id}/"), children))
    }
}

fn ways_tab(user: &Value, project: &Value, p: &Paths, project_dir: &Path) -> Node {
    let mut disabled = BTreeMap::new();
    if let Some(m) = project.get("ways").and_then(Value::as_mapping) {
        for (k, v) in m {
            let on = match v {
                Value::Bool(b) => *b,
                other => other.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            };
            if let Some(k) = k.as_str() {
                disabled.insert(k.to_string(), on);
            }
        }
    }
    let enabled = project.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let ways: Vec<Node> = domain_names(&p.corpus)
        .into_iter()
        .filter_map(|d| way_tree(&p.corpus.join(&d), &d, &disabled, p))
        .collect();
    Node::group(
        "ways",
        format!("Which ways are on: this project's switch, the user-wide domains, then each way here.\nProject: {}\nToday: `ways disable` / `ways enable` and .claude/ways.yaml.", project_dir.display()),
        vec![
            Node::leaf(
                "enabled",
                "false switches ways off in this project: the scan injects nothing (ADR-184).",
                Setting::new(Kind::Bool, enabled.to_string(), if project.get("enabled").is_some() { "project" } else { "default" })
                    .default("true")
                    .store(p.project.clone(), "enabled"),
            ),
            domains(user, p),
            Node::group(
                "project",
                "Each way, on or off for this project only (ADR-131). A toggle stays a value here; the real command maps to `ways disable <id>` and `ways enable <id>`.",
                ways,
            ),
        ],
    )
    .with_actions(vec![Action::new("set up", "guided: pick a project, preview what `ways init` writes there").arg(Arg::Flow("setup".into()))])
}

fn gate(agent: &Value, p: &Paths) -> Node {
    let shipped: BTreeMap<String, BTreeMap<String, Value>> = serde_yaml::from_str(SHIPPED_PROFILES).unwrap_or_default();
    let names: Vec<String> = ["anthropic", "openrouter"].iter().map(|s| s.to_string()).filter(|n| shipped.contains_key(n)).collect();
    let engine = agent.get("engine").and_then(scalar);
    let mode = agent.get("mode").and_then(scalar);
    let field_kind = |f: &str| match f {
        "threshold" => prob(),
        "timeout_ms" => Kind::Int { min: 100, max: 30_000 },
        "turns" => Kind::Int { min: 0, max: 20 },
        "max_turn_chars" => Kind::Int { min: 100, max: 20_000 },
        "concurrency" => Kind::Int { min: 1, max: 64 },
        "max_candidates" => Kind::Int { min: 1, max: 64 },
        "model" => Kind::Text,
        _ => Kind::ReadOnly,
    };
    let field_doc = |f: &str| match f {
        "threshold" => "P(yes) below this blocks a candidate in enforce mode.",
        "timeout_ms" => "The judge call's deadline. Past it the gate fails open.",
        "turns" => "Conversation turns sent as context, counted back from the last.",
        "max_turn_chars" => "Each turn is cut to its last this-many characters.",
        "concurrency" => "Provider calls the agent runs at once, across all sessions.",
        "max_candidates" => "Candidates judged per request; the rest pass unjudged (ADR-197).",
        "model" => "The model this profile calls. `ways agent models` lists them.",
        _ => "Fixed by the shipped profile.",
    };
    let profiles = names
        .iter()
        .map(|name| {
            let user_patch = agent.get("profiles").and_then(|m| m.get(name.as_str()));
            let fields = shipped[name]
                .iter()
                .map(|(f, v)| {
                    let d = scalar(v).unwrap_or_default();
                    let (val, src) = match user_patch.and_then(|m| m.get(f.as_str())).and_then(scalar) {
                        Some(u) => (u, "user"),
                        None => (d.clone(), "shipped"),
                    };
                    Node::leaf(
                        f.clone(),
                        field_doc(f),
                        Setting::new(field_kind(f), val, src).default(d).store(p.agent.clone(), format!("profiles.{name}.{f}")),
                    )
                })
                .collect();
            Node::group(name.clone(), format!("Engine profile {name} (ADR-196 §5)."), fields)
        })
        .collect();
    let keys = names
        .iter()
        .map(|name| {
            // Existence only: the key file is never opened.
            let present = p.keys.join(name).exists();
            let key = |verb: &str| format!("ways agent key {verb} --provider {name}");
            let mut actions = vec![if present {
                Action::new("rotate", key("rotate")).arg(Arg::Secret)
            } else {
                Action::new("set", key("add")).arg(Arg::Secret)
            }];
            if present {
                actions.push(Action::new("remove", key("remove")).confirm());
            }
            actions.push(Action::new("check", key("check")));
            Node::leaf(
                name.clone(),
                "Enter types the key masked and queues `ways agent key add|rotate`, which reads it from stdin: it never reaches argv or the screen, and the settings view never shows key material.",
                Setting::new(Kind::Secret, if present { "present" } else { "absent" }, "keys/"),
            )
            .with_actions(actions)
        })
        .collect();
    let mut choices = vec!["(auto)".to_string()];
    choices.extend(names.iter().cloned());
    Node::group(
        "gate",
        "The relevance gate the ways agent runs (ADR-196). Today: `ways agent use|mode` and agent.yaml.",
        vec![
            Node::leaf(
                "engine",
                "The profile the agent uses. (auto): the first shipped profile with a key.",
                Setting::new(Kind::Choice(choices), engine.clone().unwrap_or("(auto)".into()), if engine.is_some() { "user" } else { "default" })
                    .default("(auto)")
                    .store(p.agent.clone(), "engine"),
            ),
            Node::leaf(
                "mode",
                "enforce blocks, shadow only logs, off skips the judge.",
                Setting::new(Kind::Choice(vec!["enforce".into(), "shadow".into(), "off".into()]), mode.clone().unwrap_or("enforce".into()), if mode.is_some() { "user" } else { "default" })
                    .default("enforce")
                    .store(p.agent.clone(), "mode"),
            ),
            Node::group("profiles", "Per-engine tuning. A user layer overrides any field.", profiles),
            Node::group("keys", "Provider API keys, by presence. Entry is masked.", keys),
        ],
    )
}

fn install(user: &Value, p: &Paths) -> Node {
    let targets: Vec<Node> = user
        .get("targets")
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(|t| {
                    let path = t.get("path").and_then(scalar)?;
                    let on = t.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                    let target = |verb: &str| format!("ways config target {verb} {}", quote(&path));
                    let toggle = if on { Action::new("disable", target("disable")).confirm() } else { Action::new("enable", target("enable")) };
                    Some(
                        Node::leaf(
                            path.clone(),
                            "A Claude Code config directory agent-ways projects into (ADR-184). The value is read-only: enabling, disabling and removing reconcile, so they are actions.",
                            Setting::new(Kind::ReadOnly, if on { "enabled" } else { "disabled" }, "user"),
                        )
                        .with_actions(vec![toggle, Action::new("remove", target("remove")).confirm()]),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let deny_user = user.get("secret_path_deny").and_then(Value::as_bool);
    let reconcile = Action::new("reconcile", "ways reconcile").confirm();
    let activate = || Action::new("activate", "guided: pick Claude config directories, preview the plan").arg(Arg::Flow("activate".into()));
    // With nothing recorded the group would be empty, so it says how to start.
    let none = targets.is_empty();
    let mut targets = targets;
    if none {
        targets.push(
            Node::leaf("(none recorded)", "No target is recorded, so the default ~/.claude applies. `a` guides activating agent-ways in a Claude instance.", Setting::new(Kind::ReadOnly, "a: activate in a Claude instance", "default"))
                .with_actions(vec![activate()]),
        );
    }
    Node::group(
        "install",
        "Where agent-ways is active and what the projection writes.",
        vec![
            Node::group("targets", "Projection targets. Changing one is an action, so the tree shows them and the command changes them.", targets)
                .with_actions(vec![
                    activate(),
                    Action::new("add", "ways config target add {}").arg(Arg::Text("directory".into())).confirm(),
                    Action::new("plan", "ways config target plan {}").arg(Arg::Text("directory".into())),
                ])
                .opened_if(none),
            Node::leaf(
                "secret_path_deny",
                "Project the secret-path permissions.deny baseline into settings.json (ADR-152). Takes effect at the next `ways reconcile`.",
                Setting::new(Kind::Bool, deny_user.unwrap_or(true).to_string(), if deny_user.is_some() { "user" } else { "default" })
                    .default("true")
                    .store(p.user.clone(), "secret_path_deny"),
            ),
        ],
    )
    .with_actions(vec![activate(), reconcile])
}

// ---- guided flows -----------------------------------------------------------
//
// Two helpers, both read-only until their commands are applied: activating
// agent-ways in Claude instances, and setting it up in a project. Discovery
// reads directory listings and file existence. The activation preview runs
// `ways config target plan`, which touches nothing.

/// Where discovery looks and how a plan is fetched, so tests can point both
/// at a fixture.
pub struct Env {
    pub home: PathBuf,
    pub xdg_config: PathBuf,
    pub claude_config_dir: Option<PathBuf>,
    pub user_config: PathBuf,
    /// Where Claude Code keeps one directory per project with sessions.
    pub projects: PathBuf,
    pub project: PathBuf,
    /// The text `ways config target plan <dir>` prints.
    pub plan: Rc<dyn Fn(&Path) -> String>,
}

impl Env {
    pub fn resolve(p: &Paths, project: &Path) -> Env {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let xdg_config = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".config"));
        Env {
            projects: home.join(".claude/projects"),
            claude_config_dir: std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|s| !s.is_empty()).map(PathBuf::from),
            home,
            xdg_config,
            user_config: p.user.clone(),
            project: project.to_path_buf(),
            plan: Rc::new(run_plan),
        }
    }
}

/// The flow an `Arg::Flow` action names.
pub fn helpers(env: Env) -> impl Fn(&str) -> Option<Flow> {
    move |name| match name {
        "activate" => Some(activate_flow(&env)),
        "setup" => Some(setup_flow(&env)),
        _ => None,
    }
}

/// `ways config target plan <dir>`, read-only. Tries `$WAYS_BIN`, `ways` on
/// the path, then this checkout's debug build.
fn run_plan(dir: &Path) -> String {
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/ways");
    let bins = [std::env::var_os("WAYS_BIN").map(PathBuf::from), Some(PathBuf::from("ways")), Some(built)];
    for bin in bins.into_iter().flatten() {
        match Command::new(&bin).args(["config", "target", "plan"]).arg(dir).env("NO_COLOR", "1").output() {
            // A blocked plan exits non-zero and still prints everything.
            Ok(o) if !o.stdout.is_empty() => return String::from_utf8_lossy(&o.stdout).into_owned(),
            Ok(o) => return String::from_utf8_lossy(&o.stderr).into_owned(),
            Err(_) => continue,
        }
    }
    "could not run `ways config target plan`: no ways binary found (set WAYS_BIN)".into()
}

fn tilde(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(r) if r.as_os_str().is_empty() => "~".into(),
        Ok(r) if !home.as_os_str().is_empty() => format!("~/{}", r.display()),
        _ => p.display().to_string(),
    }
}

/// A typed path: `~` and `~/x` expand, anything else is taken as given.
fn expand(text: &str, home: &Path) -> PathBuf {
    match text.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(text),
    }
}

/// The targets recorded in the user config, and whether any are: with none,
/// `~/.claude` is the one default target.
fn recorded_targets(user_config: &Path) -> Vec<(String, bool)> {
    load(user_config)
        .get("targets")
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(|t| Some((t.get("path").and_then(scalar)?, t.get("enabled").and_then(Value::as_bool).unwrap_or(true))))
                .collect()
        })
        .unwrap_or_default()
}

/// What makes a directory a Claude config dir.
fn looks_like_claude(dir: &Path) -> bool {
    dir.join("settings.json").is_file() || dir.join("projects").is_dir()
}

/// One candidate for the activation picker, badged by what is known of `dir`.
fn claude_candidate(dir: &Path, detail: &str, recorded: &[(String, bool)], env: &Env) -> Candidate {
    let id = dir.display().to_string();
    let label = tilde(dir, &env.home);
    let same = |a: &str| a == id || fs::canonicalize(a).ok().zip(fs::canonicalize(dir).ok()).is_some_and(|(x, y)| x == y);
    match recorded.iter().find(|(p, _)| same(p)) {
        Some((_, true)) => Candidate::new(id, label, "recorded target, enabled", "active", Tone::Ok).unpickable(),
        Some((_, false)) => Candidate::new(id, label, "recorded target, off: finishing enables it", "disabled", Tone::Warn),
        None if recorded.is_empty() && dir == env.home.join(".claude") => {
            Candidate::new(id, label, "the default target while none is recorded", "active", Tone::Ok).unpickable()
        }
        None if looks_like_claude(dir) => Candidate::new(id, label, detail, "available", Tone::Accent),
        None => Candidate::new(id, label, "no settings.json or projects/ inside", "not a Claude dir", Tone::Muted).unpickable(),
    }
}

/// Claude config directories to offer: the recorded targets, `~/.claude`,
/// `$CLAUDE_CONFIG_DIR`, then directories directly under `$HOME` and
/// `$XDG_CONFIG_HOME` named like Claude that look like one. Files such as
/// `.claude.json` are skipped.
pub fn claude_dirs(env: &Env) -> Vec<Candidate> {
    let recorded = recorded_targets(&env.user_config);
    let mut found: Vec<(PathBuf, String)> = recorded.iter().map(|(p, _)| (expand(p, &env.home), "recorded target".to_string())).collect();
    found.push((env.home.join(".claude"), "the usual place".into()));
    if let Some(d) = &env.claude_config_dir {
        found.push((d.clone(), "$CLAUDE_CONFIG_DIR".into()));
    }
    for root in [&env.home, &env.xdg_config] {
        let mut names: Vec<PathBuf> = fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().to_lowercase().contains("claude"))
            .map(|e| e.path())
            .filter(|p| p.is_dir() && looks_like_claude(p))
            .collect();
        names.sort();
        found.extend(names.into_iter().map(|p| (p, format!("found under {}", tilde(root, &env.home)))));
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for (dir, detail) in found {
        let canon = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if seen.contains(&canon) || !(dir.is_dir() || recorded.iter().any(|(p, _)| expand(p, &env.home) == dir)) {
            continue;
        }
        seen.push(canon);
        out.push(claude_candidate(&dir, &detail, &recorded, env));
    }
    out
}

/// Flow 1: activate agent-ways in Claude instances.
pub fn activate_flow(env: &Env) -> Flow {
    let recorded = recorded_targets(&env.user_config);
    let (plan, home) = (env.plan.clone(), env.home.clone());
    let (other_env, other_rec) = (Env { plan: env.plan.clone(), home: env.home.clone(), xdg_config: env.xdg_config.clone(), claude_config_dir: None, user_config: env.user_config.clone(), projects: env.projects.clone(), project: env.project.clone() }, recorded.clone());
    Flow::new(
        "activate agent-ways in Claude instances",
        true,
        claude_dirs(env),
        move |picks| {
            let mut lines = Vec::new();
            for c in picks {
                if !lines.is_empty() {
                    lines.push(PLine::new(Verb::Plain, ""));
                }
                lines.extend(parse_plan(&plan(Path::new(&c.id))));
            }
            lines
        },
        move |picks, _| {
            picks
                .iter()
                .map(|c| {
                    let off = recorded.iter().any(|(p, on)| !on && expand(p, &home).display().to_string() == c.id);
                    let verb = if off { "enable" } else { "add" };
                    Out::new(verb, format!("ways config target {verb} {}", quote(&c.id)), true)
                })
                .collect()
        },
    )
    .other(move |text| {
        let dir = expand(text.trim(), &other_env.home);
        claude_candidate(&dir, "typed path", &other_rec, &other_env)
    })
    .notes(
        "Claude config directories to activate; active ones are listed for reference",
        "the real plan from `ways config target plan`; nothing has run",
        "finishing queues these on the install tab; its review applies them",
    )
}

/// Strip ANSI colour sequences, which the table printer adds on a terminal.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for e in chars.by_ref() {
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `ways config target plan` output as styled lines: each root by its verb
/// (linked is kept, link adds, relink replaces, refused is refused), then the
/// settings merge by its verb (kept, add, replace, REMOVE). A refused root
/// carries the note that `--force` moves the real path aside.
pub fn parse_plan(text: &str) -> Vec<PLine> {
    let mut out: Vec<PLine> = Vec::new();
    let mut table = false;
    for raw in text.lines() {
        let line = strip_ansi(raw);
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        if t.starts_with("plan for ") {
            out.push(PLine::new(Verb::Heading, t));
            table = true;
            continue;
        }
        if table {
            if words[0] == "Root" || t.starts_with('─') {
                continue;
            }
            let verb = match words.get(1).copied() {
                Some("linked") => Some(Verb::Kept),
                Some("link") => Some(Verb::Added),
                Some("relink") => Some(Verb::Replaced),
                Some("refused") => Some(Verb::Refused),
                _ => None,
            };
            if let Some(v) = verb {
                out.push(PLine::new(v, format!("{:<30}{:<8}{}", words[0], words[1], words[2..].join(" "))));
                continue;
            }
            table = false;
        }
        let verb = match words[0] {
            w if w.starts_with("settings.json") && t.contains("already merged") => Verb::Kept,
            w if w.starts_with("settings.json") && t.ends_with(':') => Verb::Heading,
            w if w.starts_with("settings.json") => Verb::Plain,
            "kept" => Verb::Kept,
            "add" | "allow" | "deny" => Verb::Added,
            "refresh" | "replace" => Verb::Replaced,
            "REMOVE" => Verb::Removed,
            "blocked:" | "--force" | "move" => Verb::Refused,
            _ => Verb::Plain,
        };
        out.push(PLine::new(verb, t));
    }
    let refused = out.iter().any(|l| l.verb == Verb::Refused);
    if refused && !out.iter().any(|l| l.text.contains("--force")) {
        out.push(PLine::new(Verb::Refused, "--force moves each real path aside to <name>.ways-backup-<seconds>"));
    }
    out
}

/// The path a Claude project directory name stands for. Claude Code writes a
/// path with `/` and `.` as `-`, so a name is ambiguous and is resolved
/// against the file system: `--x` is `/.x`, and a dash inside a segment may
/// be `-`, `.` or `_`.
fn decode_project(name: &str) -> Option<PathBuf> {
    let toks: Vec<&str> = name.strip_prefix('-')?.split('-').collect();
    walk_project(Path::new("/"), &toks)
}

fn walk_project(dir: &Path, toks: &[&str]) -> Option<PathBuf> {
    if toks.is_empty() {
        return Some(dir.to_path_buf());
    }
    let (dot, toks) = if toks[0].is_empty() && toks.len() > 1 { (".", &toks[1..]) } else { ("", toks) };
    for take in 1..=toks.len().min(6) {
        let mut segs = vec![toks[0].to_string()];
        for t in &toks[1..take] {
            segs = segs.iter().flat_map(|s| ["-", ".", "_"].map(|sep| format!("{s}{sep}{t}"))).collect();
        }
        for seg in segs {
            let path = dir.join(format!("{dot}{seg}"));
            if path.exists() {
                if let Some(found) = walk_project(&path, &toks[take..]) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn project_candidate(dir: &Path, detail: String) -> Candidate {
    let claude = dir.join(".claude");
    let (badge, tone) = match (claude.join("ways.yaml").is_file(), claude.join("ways").is_dir()) {
        (true, true) => ("ways.yaml + ways/", Tone::Ok),
        (true, false) => ("ways.yaml", Tone::Ok),
        (false, true) => ("ways/", Tone::Accent),
        (false, false) => ("none", Tone::Muted),
    };
    Candidate::new(dir.display().to_string(), dir.display().to_string(), detail, badge, tone)
}

/// Projects to offer: the current directory, then those with Claude sessions,
/// the most recently used first, keeping paths that still exist.
pub fn projects(env: &Env) -> Vec<Candidate> {
    let mut out = vec![project_candidate(&env.project, "current directory".into())];
    let mut dirs: Vec<(std::time::SystemTime, PathBuf, usize)> = fs::read_dir(&env.projects)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = decode_project(&e.file_name().to_string_lossy())?;
            let modified = e.metadata().ok()?.modified().ok()?;
            let sessions = fs::read_dir(e.path()).into_iter().flatten().flatten().filter(|f| f.path().extension().is_some_and(|x| x == "jsonl")).count();
            (path.is_dir() && path != env.project).then_some((modified, path, sessions))
        })
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.0));
    out.extend(dirs.into_iter().take(60).map(|(_, p, n)| project_candidate(&p, format!("{n} Claude session{}", if n == 1 { "" } else { "s" }))));
    out
}

/// What `ways init --project <dir>` writes, as the code in
/// `ways-cli/src/cmd/init.rs` does it: nothing without `.claude/` or `.git/`;
/// otherwise `.claude/ways/`, `.claude/.gitignore`, the way template, and the
/// MEMORY.md seed under `~/.claude/projects/<dir with / and . as ->/memory/`.
pub fn init_preview(dir: &Path, home: &Path) -> Vec<PLine> {
    let claude = dir.join(".claude");
    let mut out = vec![PLine::new(Verb::Heading, format!("ways init --project {}", dir.display()))];
    if !claude.is_dir() && !dir.join(".git").is_dir() {
        out.push(PLine::new(Verb::Refused, "neither .claude/ nor .git/ here: init does nothing"));
        return out;
    }
    let normalized: String = dir.display().to_string().chars().map(|c| if c == '/' || c == '.' { '-' } else { c }).collect();
    let memory = home.join(".claude/projects").join(normalized).join("memory/MEMORY.md");
    let item = |path: &Path, what: &str, label: String| {
        if path.exists() {
            PLine::new(Verb::Kept, format!("{label:<44} already there"))
        } else {
            PLine::new(Verb::Added, format!("{label:<44} {what}"))
        }
    };
    out.push(item(&claude.join("ways"), "new directory", ".claude/ways/".into()));
    out.push(item(&claude.join(".gitignore"), "new file: local-only files stay out of git", ".claude/.gitignore".into()));
    out.push(item(&claude.join("ways/_template.md"), "new file: a starting point for a way", ".claude/ways/_template.md".into()));
    out.push(item(&memory, "new file: the seed that routes memory (ADR-128)", tilde(&memory, home)));
    if claude.join("ways.yaml").is_file() {
        out.push(PLine::new(Verb::Kept, format!("{:<44} init leaves it alone", ".claude/ways.yaml")));
    }
    out
}

/// Flow 2: set up agent-ways in a project.
pub fn setup_flow(env: &Env) -> Flow {
    let home = env.home.clone();
    let preview_home = env.home.clone();
    Flow::new(
        "set up agent-ways in a project",
        false,
        projects(env),
        move |picks| picks.iter().flat_map(|c| init_preview(Path::new(&c.id), &preview_home)).collect(),
        |picks, on| {
            let Some(c) = picks.first() else { return Vec::new() };
            let dir = Path::new(&c.id);
            // `ways init` does nothing without .claude/ or .git/, so there is nothing to queue.
            if !dir.join(".claude").is_dir() && !dir.join(".git").is_dir() {
                return Vec::new();
            }
            let mut outs = vec![Out::new("init", format!("ways init --project {}", quote(&c.id)), false)];
            if on.first() == Some(&true) {
                outs.push(Out::new("enable", format!("ways settings set ways.enabled true --project {}", quote(&c.id)), false));
            }
            outs
        },
    )
    .other(move |text| {
        let dir = expand(text.trim(), &home);
        if dir.is_dir() {
            project_candidate(&dir, "typed path".into())
        } else {
            Candidate::new(dir.display().to_string(), dir.display().to_string(), "no such directory", "not found", Tone::Err).unpickable()
        }
    })
    .check("also set ways.enabled = true for this project", false)
    .notes(
        "a project to set up: this one, then projects with Claude sessions",
        "what `ways init` writes there; existing files are kept",
        "finishing queues these on the ways tab; its review applies them",
    )
}

/// A fake home in a temp dir, for discovery tests. Dropped, it removes itself.
#[cfg(test)]
pub mod testkit {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct Fixture {
        pub root: PathBuf,
    }

    impl Fixture {
        /// `.claude/` with `projects/`, `.claude-work/` with `settings.json`,
        /// a `.claude.json` file, `claude-empty/` and `notes/`, and one
        /// Claude directory under `.config`.
        pub fn home() -> Fixture {
            static N: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("ways-flow-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let f = Fixture { root };
            f.dir(".claude/projects");
            f.dir(".claude-work");
            f.file(".claude-work/settings.json", "{}");
            f.file(".claude.json", "{}");
            f.file(".claude.json.bak", "{}");
            f.dir("claude-empty");
            f.dir("notes");
            f.dir(".config/claude-alt/projects");
            f
        }
        pub fn dir(&self, rel: &str) -> PathBuf {
            let p = self.root.join(rel);
            fs::create_dir_all(&p).unwrap();
            p
        }
        pub fn file(&self, rel: &str, text: &str) {
            let p = self.root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        }
        /// An environment over the fixture; its plan is a fixed sample, never a run.
        pub fn env(&self) -> Env {
            Env {
                home: self.root.clone(),
                xdg_config: self.root.join(".config"),
                claude_config_dir: None,
                user_config: self.root.join(".config/agent-ways/config.yaml"),
                projects: self.root.join(".claude/projects"),
                project: self.root.join("work/current"),
                plan: Rc::new(|dir| SAMPLE_PLAN.replace("{dir}", &dir.display().to_string())),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// What `ways config target plan` printed, with the table's ANSI left in.
    pub const SAMPLE_PLAN: &str = "plan for {dir}
  \u{1b}[1mRoot                          Action Detail      \u{1b}[0m
  \u{1b}[2m─────────────────────────────────────────────────\u{1b}[0m
  skills                        linked already ours
  agents                        link   absent
  commands                      relink points elsewhere
  hooks/ways                    refused real directory
  bin/ways                      link   absent
settings.json:
  kept      2 hook entries of yours
  add       SessionStart: \"${HOME}/.claude/hooks/ways/check-setup.sh\"
  refresh   Stop: \"${HOME}/.claude/hooks/ways/check-response.sh\"  (ours, from a prior version)
  replace   PreToolUse: \"x\"  (reads as an agent-ways hook)
  REMOVE    PostToolUse: \"y\"
  allow     +3 entries
  deny      +9 entries (secret paths, ADR-152)

blocked: a real path sits at a projection root, or an entry of yours would be removed.
  --force renames each real path to <name>.ways-backup-<seconds> and proceeds;
  move a hook out of .claude/hooks/ first if it is listed under REMOVE or replace.
";
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    fn labels(cs: &[Candidate]) -> Vec<(&str, &str)> {
        cs.iter().map(|c| (c.label.as_str(), c.badge.as_str())).collect()
    }

    #[test]
    fn discovery_lists_claude_dirs_under_home_and_config_and_skips_files_and_strangers() {
        let fx = Fixture::home();
        let cs = claude_dirs(&fx.env());
        assert_eq!(labels(&cs), [("~/.claude", "active"), ("~/.claude-work", "available"), ("~/.config/claude-alt", "available")]);
        assert!(!cs[0].pickable && cs[1].pickable, "the default target is already active");
        let all: String = cs.iter().map(|c| c.label.clone()).collect();
        assert!(!all.contains(".claude.json") && !all.contains("notes") && !all.contains("claude-empty"), "{all}");
    }

    #[test]
    fn recorded_targets_come_first_with_their_state_and_claude_config_dir_is_offered() {
        let fx = Fixture::home();
        let off = fx.dir("elsewhere/claude-off");
        fx.file(".config/agent-ways/config.yaml", &format!("targets:\n  - path: {}\n    enabled: false\n  - path: {}/.claude-work\n", off.display(), fx.root.display()));
        let from_env = fx.dir("custom");
        fx.file("custom/settings.json", "{}");
        let mut env = fx.env();
        env.claude_config_dir = Some(from_env);
        let cs = claude_dirs(&env);
        let got: Vec<_> = cs.iter().map(|c| (c.label.rsplit('/').next().unwrap(), c.badge.as_str(), c.pickable)).collect();
        assert_eq!(
            got,
            [("claude-off", "disabled", true), (".claude-work", "active", false), (".claude", "available", true), ("custom", "available", true), ("claude-alt", "available", true)]
        );
        assert_eq!(cs[2].detail, "the usual place");
    }

    #[test]
    fn a_typed_path_is_badged_like_a_candidate() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let fx = Fixture::home();
        let mut flow = activate_flow(&fx.env());
        let mut type_path = |text: &str| {
            for k in [KeyCode::End, KeyCode::Enter].into_iter().chain(text.chars().map(KeyCode::Char)).chain([KeyCode::Enter]) {
                flow.key(KeyEvent::new(k, KeyModifiers::NONE));
            }
        };
        type_path("~/notes");
        type_path("~/.claude-work");
        let got: Vec<_> = flow.candidates().iter().skip(3).map(|c| (c.label.as_str(), c.badge.as_str(), c.pickable)).collect();
        assert_eq!(got, [("~/notes", "not a Claude dir", false)]);
        assert_eq!(flow.picked_ids().len(), 1, "an existing candidate is picked, not added twice");
    }

    #[test]
    fn plan_output_becomes_lines_styled_by_verb() {
        let lines = parse_plan(SAMPLE_PLAN);
        let verbs: Vec<(Verb, &str)> = lines.iter().map(|l| (l.verb, l.text.split_whitespace().next().unwrap())).collect();
        assert_eq!(
            verbs,
            [
                (Verb::Heading, "plan"),
                (Verb::Kept, "skills"),
                (Verb::Added, "agents"),
                (Verb::Replaced, "commands"),
                (Verb::Refused, "hooks/ways"),
                (Verb::Added, "bin/ways"),
                (Verb::Heading, "settings.json:"),
                (Verb::Kept, "kept"),
                (Verb::Added, "add"),
                (Verb::Replaced, "refresh"),
                (Verb::Replaced, "replace"),
                (Verb::Removed, "REMOVE"),
                (Verb::Added, "allow"),
                (Verb::Added, "deny"),
                (Verb::Refused, "blocked:"),
                (Verb::Refused, "--force"),
                (Verb::Refused, "move"),
            ]
        );
        assert!(lines.iter().all(|l| !l.text.contains('\u{1b}')), "ANSI is stripped");
        assert_eq!(lines[1].text, "skills                        linked  already ours");
    }

    #[test]
    fn a_clean_plan_is_all_kept_and_a_refused_root_without_the_note_gets_it() {
        let clean = parse_plan("plan for /d\n  Root Action Detail\n  ────\n  skills linked already ours\nsettings.json: already merged, nothing changes\n");
        assert!(clean.iter().all(|l| matches!(l.verb, Verb::Heading | Verb::Kept)), "{clean:?}");
        let refused = parse_plan("plan for /d\n  skills refused real directory\n");
        assert_eq!(refused.last().unwrap().verb, Verb::Refused);
        assert!(refused.last().unwrap().text.contains("--force moves each real path aside"));
    }

    #[test]
    fn project_names_decode_against_the_file_system() {
        let fx = Fixture::home();
        fx.dir("Projects/ai/kg/knowledge-graph-system");
        fx.dir(".dotfiles/sub_dir");
        let enc = |p: &Path| p.display().to_string().replace(['/', '.'], "-");
        let a = fx.root.join("Projects/ai/kg/knowledge-graph-system");
        assert_eq!(decode_project(&enc(&a)), Some(a));
        let b = fx.root.join(".dotfiles/sub_dir");
        assert_eq!(decode_project(&enc(&b)), Some(b));
        assert_eq!(decode_project("-no-such-place-xyz"), None);
    }

    #[test]
    fn projects_lead_with_the_current_dir_and_badge_what_each_has() {
        let fx = Fixture::home();
        let cur = fx.dir("work/current");
        let yaml = fx.dir("work/has-yaml");
        fx.file("work/has-yaml/.claude/ways.yaml", "enabled: true\n");
        let dirs = fx.dir("work/has-dir");
        fx.dir("work/has-dir/.claude/ways");
        let bare = fx.dir("work/bare");
        for (i, p) in [&cur, &yaml, &dirs, &bare].into_iter().enumerate() {
            fx.file(&format!(".claude/projects/{}/s{i}.jsonl", p.display().to_string().replace(['/', '.'], "-")), "{}");
        }
        fx.file(".claude/projects/-gone-away-nowhere/s.jsonl", "{}");
        let cs = projects(&fx.env());
        let got: Vec<_> = cs.iter().map(|c| (c.id.rsplit('/').next().unwrap(), c.badge.as_str())).collect();
        assert_eq!(got[0], ("current", "none"), "the current directory leads and is not repeated");
        assert_eq!(got.len(), 4, "a project whose path is gone is dropped: {got:?}");
        for want in [("has-yaml", "ways.yaml"), ("has-dir", "ways/"), ("bare", "none")] {
            assert!(got.contains(&want), "{want:?} in {got:?}");
        }
        assert!(cs[0].detail == "current directory" && cs[1].detail.ends_with("Claude session"));
    }

    #[test]
    fn init_preview_lists_what_init_writes_and_keeps_what_exists() {
        let fx = Fixture::home();
        let p = fx.dir("work/p");
        assert_eq!(init_preview(&p, &fx.root)[1].verb, Verb::Refused, "no .claude/ or .git/: nothing");
        fx.dir("work/p/.git");
        fx.dir("work/p/.claude/ways");
        fx.file("work/p/.claude/ways.yaml", "enabled: true\n");
        let lines = init_preview(&p, &fx.root);
        let got: Vec<_> = lines.iter().map(|l| (l.verb, l.text.split_whitespace().next().unwrap().to_string())).collect();
        assert_eq!(
            got[1..],
            [
                (Verb::Kept, ".claude/ways/".to_string()),
                (Verb::Added, ".claude/.gitignore".into()),
                (Verb::Added, ".claude/ways/_template.md".into()),
                (Verb::Added, format!("~/.claude/projects/{}/memory/MEMORY.md", p.display().to_string().replace(['/', '.'], "-"))),
                (Verb::Kept, ".claude/ways.yaml".into()),
            ]
        );
    }

    #[test]
    fn the_install_tab_says_how_to_start_when_no_target_is_recorded() {
        let fx = Fixture::home();
        let paths = Paths { user: fx.root.join("c.yaml"), agent: fx.root.join("a.yaml"), keys: fx.root.join("k"), project: fx.root.join("p.yaml"), corpus: fx.root.join("corpus") };
        let roots = build(&paths, &fx.root);
        let targets = &roots[3].children[0];
        assert!(targets.open && targets.children[0].name == "(none recorded)");
        assert!(matches!(&targets.children[0].actions[0].arg, Arg::Flow(n) if n == "activate"));
        assert!(matches!(&roots[0].actions[0].arg, Arg::Flow(n) if n == "setup"));
    }
}
