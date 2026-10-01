//! The ways adapter: every setting agent-ways reads, as one tree, with the
//! layer each value came from and the file an edit would write.
//!
//! Read-only. Key files are checked for existence and never opened. Actions
//! carry the real `ways` command lines; none is run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::tree::{quote, Action, Arg, Kind, Node, Setting};

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
        matching(&user, &project, p).opened(),
        disclosure(&user, &project, p),
        domains(&user, p),
        project_node(&project, p, project_dir),
        gate(&agent, p),
        install(&user, p),
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
    Node::group("domains", "Switch a whole domain of ways on or off, user-wide.", children)
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

fn project_node(project: &Value, p: &Paths, project_dir: &Path) -> Node {
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
        "project",
        format!("This project: {}\nToday: `ways disable` / `ways enable` and .claude/ways.yaml.", project_dir.display()),
        vec![
            Node::leaf(
                "enabled",
                "false switches ways off in this project: the scan injects nothing (ADR-184).",
                Setting::new(Kind::Bool, enabled.to_string(), if project.get("enabled").is_some() { "project" } else { "default" })
                    .default("true")
                    .store(p.project.clone(), "enabled"),
            ),
            Node::group(
                "ways",
                "Each way, on or off for this project only (ADR-131). A toggle stays a value here; the real command maps to `ways disable <id>` and `ways enable <id>`.",
                ways,
            ),
        ],
    )
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
    Node::group(
        "install",
        "Where agent-ways is active and what the projection writes.",
        vec![
            Node::group("targets", "Projection targets. Changing one is an action, so the tree shows them and the command changes them.", targets).with_actions(vec![
                Action::new("add", "ways config target add {}").arg(Arg::Text("directory".into())).confirm(),
                Action::new("plan", "ways config target plan {}").arg(Arg::Text("directory".into())),
            ]),
            Node::leaf(
                "secret_path_deny",
                "Project the secret-path permissions.deny baseline into settings.json (ADR-152). Takes effect at the next `ways reconcile`.",
                Setting::new(Kind::Bool, deny_user.unwrap_or(true).to_string(), if deny_user.is_some() { "user" } else { "default" })
                    .default("true")
                    .store(p.user.clone(), "secret_path_deny"),
            ),
        ],
    )
    .with_actions(vec![reconcile])
}
