//! The guided flows of the settings screens: activating agent-ways in Claude
//! instances (install tab) and setting it up in a project (ways tab). Both
//! read only until their commands are applied: discovery reads directory
//! listings and file existence, and the activation preview runs `ways config
//! target plan`, which touches nothing. Finishing queues the commands on the
//! launching tab, so they meet its review like any other pending item.

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_tui::flow::{Candidate, Flow, Out, PLine, Tone, Verb};
use agent_tui::tree::quote;

use super::build::tilde;

/// Where discovery looks and how a plan is fetched, so tests can point both
/// at a fixture.
pub struct Env {
    pub home: PathBuf,
    pub xdg_config: PathBuf,
    pub claude_config_dir: Option<PathBuf>,
    /// The targets the user config records, as (path, enabled).
    pub targets: Vec<(String, bool)>,
    /// The Claude config directory whose `projects/` lists sessions.
    pub claude: PathBuf,
    pub project: PathBuf,
    /// The text `ways config target plan <dir>` prints.
    pub plan: Rc<dyn Fn(&Path) -> String>,
}

impl Env {
    fn copy(&self) -> Env {
        Env {
            home: self.home.clone(),
            xdg_config: self.xdg_config.clone(),
            claude_config_dir: self.claude_config_dir.clone(),
            targets: self.targets.clone(),
            claude: self.claude.clone(),
            project: self.project.clone(),
            plan: self.plan.clone(),
        }
    }
}

/// The flow an `Arg::Flow` action names.
pub fn flow(env: &Env, name: &str) -> Option<Flow> {
    match name {
        "activate" => Some(activate_flow(env)),
        "setup" => Some(setup_flow(env)),
        _ => None,
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

/// What makes a directory a Claude config dir.
fn looks_like_claude(dir: &Path) -> bool {
    dir.join("settings.json").is_file() || dir.join("projects").is_dir()
}

/// One candidate for the activation picker, badged by what is known of `dir`.
fn claude_candidate(dir: &Path, detail: &str, env: &Env) -> Candidate {
    let id = dir.display().to_string();
    let label = tilde(dir, &env.home);
    let same = |a: &str| a == id || fs::canonicalize(a).ok().zip(fs::canonicalize(dir).ok()).is_some_and(|(x, y)| x == y);
    match env.targets.iter().find(|(p, _)| same(p)) {
        Some((_, true)) => Candidate::new(id, label, "recorded target, enabled", "active", Tone::Ok).unpickable(),
        Some((_, false)) => Candidate::new(id, label, "recorded target, off: finishing enables it", "disabled", Tone::Warn),
        None if env.targets.is_empty() && dir == env.home.join(".claude") => {
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
    let mut found: Vec<(PathBuf, String)> = env.targets.iter().map(|(p, _)| (expand(p, &env.home), "recorded target".to_string())).collect();
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
        if seen.contains(&canon) || !(dir.is_dir() || env.targets.iter().any(|(p, _)| expand(p, &env.home) == dir)) {
            continue;
        }
        seen.push(canon);
        out.push(claude_candidate(&dir, &detail, env));
    }
    out
}

/// Activate agent-ways in Claude instances.
pub fn activate_flow(env: &Env) -> Flow {
    let (plan, home, targets) = (env.plan.clone(), env.home.clone(), env.targets.clone());
    let other = env.copy();
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
                    let off = targets.iter().any(|(p, on)| !on && expand(p, &home).display().to_string() == c.id);
                    let verb = if off { "enable" } else { "add" };
                    Out::new(verb, format!("ways config target {verb} {}", quote(&c.id)), true)
                })
                .collect()
        },
    )
    .other(move |text| {
        let dir = expand(text.trim(), &other.home);
        claude_candidate(&dir, "typed path", &other)
    })
    .notes(
        "Claude config directories to activate; active ones are listed for reference",
        "the real plan from `ways config target plan`; nothing has run",
        "finishing queues these on the install tab; its review applies them",
    )
}

/// Strip colour sequences, which the table printer adds on a terminal.
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

/// Projects to offer: the current directory, then those with Claude
/// sessions, the most recently used first, keeping paths that still exist.
/// Claude's project directories resolve to their paths through
/// `claude-sessions` (ADR-504 §6).
pub fn projects(env: &Env) -> Vec<Candidate> {
    let mut out = vec![project_candidate(&env.project, "current directory".into())];
    let root = claude_sessions::ClaudeDir::at(&env.claude).projects_dir();
    let mut dirs: Vec<(std::time::SystemTime, PathBuf, usize)> = claude_sessions::project_dirs_in(&root)
        .into_iter()
        .filter_map(|d| {
            let name = d.file_name()?.to_string_lossy().into_owned();
            let path = PathBuf::from(claude_sessions::resolve_project_path(&root, &name)?);
            let modified = fs::metadata(&d).ok()?.modified().ok()?;
            let sessions = claude_sessions::transcripts_in(&d).len();
            (path.is_dir() && path != env.project).then_some((modified, path, sessions))
        })
        .collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.0));
    out.extend(dirs.into_iter().take(60).map(|(_, p, n)| project_candidate(&p, format!("{n} Claude session{}", if n == 1 { "" } else { "s" }))));
    out
}

/// What `ways init --project <dir>` writes, as `cmd/init.rs` does it:
/// nothing without `.claude/` or `.git/`; otherwise `.claude/ways/`,
/// `.claude/.gitignore`, the way template, and the MEMORY.md seed in the
/// project's Claude directory.
pub fn init_preview(dir: &Path, env: &Env) -> Vec<PLine> {
    let claude = dir.join(".claude");
    let mut out = vec![PLine::new(Verb::Heading, format!("ways init --project {}", dir.display()))];
    if !claude.is_dir() && !dir.join(".git").is_dir() {
        out.push(PLine::new(Verb::Refused, "neither .claude/ nor .git/ here: init does nothing"));
        return out;
    }
    let memory = claude_sessions::ClaudeDir::at(&env.claude).project_dir(&dir.display().to_string()).join("memory").join("MEMORY.md");
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
    out.push(item(&memory, "new file: the seed that routes memory (ADR-128)", tilde(&memory, &env.home)));
    if claude.join("ways.yaml").is_file() {
        out.push(PLine::new(Verb::Kept, format!("{:<44} init leaves it alone", ".claude/ways.yaml")));
    }
    out
}

/// Set up agent-ways in a project.
pub fn setup_flow(env: &Env) -> Flow {
    let home = env.home.clone();
    let preview = env.copy();
    Flow::new(
        "set up agent-ways in a project",
        false,
        projects(env),
        move |picks| picks.iter().flat_map(|c| init_preview(Path::new(&c.id), &preview)).collect(),
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

#[cfg(test)]
pub mod testkit {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fake home in a temp dir, for discovery tests. Dropped, it removes
    /// itself.
    pub struct Fixture {
        pub root: PathBuf,
    }

    impl Fixture {
        /// `.claude/` with `projects/`, `.claude-work/` with `settings.json`,
        /// a `.claude.json` file, `claude-empty/` and `notes/`, and one
        /// Claude directory under `.config`.
        pub fn home() -> Fixture {
            static N: AtomicUsize = AtomicUsize::new(0);
            // A short root, so paths in a frame are not cut short: under
            // /tmp on Unix (macOS's temp dir is long), the temp dir elsewhere.
            let name = format!("wf-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
            #[cfg(unix)]
            let root = PathBuf::from("/tmp").join(name);
            #[cfg(not(unix))]
            let root = std::env::temp_dir().join(name);
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
                targets: Vec::new(),
                claude: self.root.join(".claude"),
                project: self.root.join("work/current"),
                plan: Rc::new(|dir| sample_plan(&dir.display().to_string())),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// What `ways config target plan` printed on a terminal, with the
    /// table's colour sequences in.
    pub fn sample_plan(dir: &str) -> String {
        let esc = '\u{1b}';
        format!(
            "plan for {dir}
  {esc}[1mRoot                          Action Detail      {esc}[0m
  {esc}[2m─────────────────────────────────────────────────{esc}[0m
  skills                        linked already ours
  agents                        link   absent
  commands                      relink points elsewhere
  hooks/ways                    refused real directory
  bin/ways                      link   absent
settings.json:
  kept      2 hook entries of yours
  add       SessionStart: \"${{HOME}}/.claude/hooks/ways/check-setup.sh\"
  refresh   Stop: \"${{HOME}}/.claude/hooks/ways/check-response.sh\"  (ours, from a prior version)
  replace   PreToolUse: \"x\"  (reads as an agent-ways hook)
  REMOVE    PostToolUse: \"y\"
  allow     +3 entries
  deny      +9 entries (secret paths, ADR-152)

blocked: a real path sits at a projection root, or an entry of yours would be removed.
  --force renames each real path to <name>.ways-backup-<seconds> and proceeds;
  move a hook out of .claude/hooks/ first if it is listed under REMOVE or replace.
"
        )
    }
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
        let from_env = fx.dir("custom");
        fx.file("custom/settings.json", "{}");
        let mut env = fx.env();
        env.targets = vec![(off.display().to_string(), false), (format!("{}/.claude-work", fx.root.display()), true)];
        env.claude_config_dir = Some(from_env);
        let cs = claude_dirs(&env);
        let got: Vec<_> = cs.iter().map(|c| (c.label.rsplit(['/', '\\']).next().unwrap(), c.badge.as_str(), c.pickable)).collect();
        assert_eq!(
            got,
            [("claude-off", "disabled", true), (".claude-work", "active", false), (".claude", "available", true), ("custom", "available", true), ("claude-alt", "available", true)]
        );
        assert_eq!(cs[2].detail, "the usual place");
    }

    #[test]
    fn a_typed_path_is_badged_like_a_candidate() {
        use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        let lines = parse_plan(&sample_plan("/d"));
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
        assert!(lines.iter().all(|l| !l.text.contains('\u{1b}')), "colour sequences are stripped");
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
    fn projects_lead_with_the_current_dir_and_badge_what_each_has() {
        let fx = Fixture::home();
        let cur = fx.dir("work/current");
        let yaml = fx.dir("work/has-yaml");
        fx.file("work/has-yaml/.claude/ways.yaml", "enabled: true\n");
        let dirs = fx.dir("work/has-dir");
        fx.dir("work/has-dir/.claude/ways");
        let bare = fx.dir("work/bare");
        for (i, p) in [&cur, &yaml, &dirs, &bare].into_iter().enumerate() {
            let slug = claude_sessions::project_slug(&p.display().to_string());
            fx.file(&format!(".claude/projects/{slug}/s{i}.jsonl"), &format!("{}\n", serde_json::json!({ "cwd": p.display().to_string() })));
        }
        fx.file(".claude/projects/-gone-away-nowhere/s.jsonl", "{}");
        let cs = projects(&fx.env());
        let got: Vec<_> = cs.iter().map(|c| (c.id.rsplit(['/', '\\']).next().unwrap(), c.badge.as_str())).collect();
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
        assert_eq!(init_preview(&p, &fx.env())[1].verb, Verb::Refused, "no .claude/ or .git/: nothing");
        fx.dir("work/p/.git");
        fx.dir("work/p/.claude/ways");
        fx.file("work/p/.claude/ways.yaml", "enabled: true\n");
        let lines = init_preview(&p, &fx.env());
        let got: Vec<_> = lines.iter().map(|l| (l.verb, l.text.split_whitespace().next().unwrap().to_string())).collect();
        let slug = claude_sessions::project_slug(&p.display().to_string());
        assert_eq!(
            got[1..],
            [
                (Verb::Kept, ".claude/ways/".to_string()),
                (Verb::Added, ".claude/.gitignore".into()),
                (Verb::Added, ".claude/ways/_template.md".into()),
                (Verb::Added, format!("~/.claude/projects/{slug}/memory/MEMORY.md")),
                (Verb::Kept, ".claude/ways.yaml".into()),
            ]
        );
    }
}
