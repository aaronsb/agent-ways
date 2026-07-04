//! Regression tests for project-local way embedding + matching.
//!
//! These lock the three defects behind "project-local ways never fire":
//!   - Bug A: `ways corpus` must embed the current project from CLAUDE_PROJECT_DIR
//!     (Windows-safe — no lossy decode of the ~/.claude/projects/ dir name).
//!   - Bug B: the namespaced corpus id must equal what `ways scan` computes for
//!     the same project, so the embedding lookup actually finds project ways.
//!   - Bug C: `ways corpus --ways-dir` must not silently clobber the canonical
//!     user corpus; `--output` redirects an isolated build.
//!
//! Cross-platform and model-free by default. The semantic leg runs only when a
//! model is provided via `WAYS_TEST_MODEL_DIR` (the embedding `.gguf` + the
//! `way-embed` binary), so CI without the model stays green.

use std::path::{Path, PathBuf};
use std::process::Command;

fn ways_bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // test binary name
    path.pop(); // deps/
    path.push(if cfg!(windows) { "ways.exe" } else { "ways" });
    if !path.exists() {
        path = PathBuf::from(env!("CARGO_BIN_EXE_ways"));
    }
    path
}

fn fixture_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}

/// A faithful copy of `util::encode_project_key`. Integration tests cannot call
/// crate internals, so the contract is pinned here: if the encoding rule
/// changes, BOTH the implementation and this copy must change together — that is
/// the point. Corpus and scan share this rule, so a drift on one side is exactly
/// the regression we are guarding against.
fn encode_project_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let normalized: PathBuf = resolved.components().collect();
    let mut s = normalized.to_string_lossy().into_owned();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    s.chars()
        .map(|c| if c == '\\' || c == '/' || c == ':' { '-' } else { c })
        .collect()
}

struct Env {
    base: PathBuf,
    home: PathBuf,
    xdg_cache: PathBuf,
    xdg_config: PathBuf,
    xdg_runtime: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("ways-proj-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let xdg_cache = base.join("cache");
        let xdg_config = base.join("config");
        let xdg_runtime = base.join("runtime");
        for d in [&home, &xdg_cache, &xdg_config, &xdg_runtime] {
            std::fs::create_dir_all(d).unwrap();
        }
        Env { base, home, xdg_cache, xdg_config, xdg_runtime }
    }

    /// Write a semantic way file at `<root>/<id>/<leaf>.md`.
    fn write_way(root: &Path, id: &str, desc: &str, vocab: &str) {
        let leaf = id.rsplit('/').next().unwrap_or(id);
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{leaf}.md")),
            format!("---\ndescription: {desc}\nvocabulary: {vocab}\n---\n# {leaf}\n"),
        )
        .unwrap();
    }

    /// Apply the isolated environment so neither the real `~/.claude/projects`
    /// nor the real corpus is read or written.
    fn apply(&self, cmd: &mut Command) {
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CACHE_HOME", &self.xdg_cache)
            .env("XDG_CONFIG_HOME", &self.xdg_config)
            .env("XDG_RUNTIME_DIR", &self.xdg_runtime)
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("PWD");
    }

    fn corpus_jsonl(&self) -> PathBuf {
        self.xdg_cache.join("agent-ways/user/ways-corpus.jsonl")
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Run a configured `ways corpus` invocation, asserting it exits 0 and — on
/// failure — surfacing the exit code plus the child's stdout/stderr. On the
/// windows-latest CI leg these tests fail with `success() == false` while
/// `.status()` (inherited) left the streams empty, hiding the cause. Capturing
/// the exit *code* is the tell (e.g. 0xC0000005 access violation / a large
/// unsigned code vs 101 for a Rust panic), and callers run corpus WITHOUT
/// `--quiet` so its own progress log shows how far it got before dying.
fn assert_corpus_ok(cmd: &mut Command, ctx: &str) {
    let out = cmd.output().expect("spawn ways corpus");
    assert!(
        out.status.success(),
        "ways corpus failed [{ctx}]: exit={:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn corpus_ids(jsonl: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(jsonl).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect()
}

#[test]
fn project_way_embeds_with_namespaced_id() {
    let env = Env::new("embed");
    let project = fixture_project();

    let mut cmd = Command::new(ways_bin());
    env.apply(&mut cmd);
    assert_corpus_ok(
        cmd.args(["corpus"]).env("CLAUDE_PROJECT_DIR", &project),
        "embed",
    );

    let ids = corpus_ids(&env.corpus_jsonl());
    let expected = format!("{}/projdomain/projway", encode_project_key(&project));

    assert!(
        ids.iter().any(|id| id == &expected),
        "Bug A/B: expected namespaced project id '{expected}' in corpus.\n\
         The prefix must equal encode_project_key(project) so `ways scan` finds it.\n\
         Got ids: {ids:?}"
    );
}

#[test]
fn project_way_fires_via_keyword() {
    let env = Env::new("keyword");
    let project = fixture_project();

    // Embed first (keyword matching reads files from disk, but this also proves
    // the end-to-end path the reporter exercised).
    let mut gen = Command::new(ways_bin());
    env.apply(&mut gen);
    assert_corpus_ok(
        gen.args(["corpus"]).env("CLAUDE_PROJECT_DIR", &project),
        "keyword",
    );

    let mut scan = Command::new(ways_bin());
    env.apply(&mut scan);
    let out = scan
        .args([
            "scan", "prompt",
            "--query", "trigger zqxplugintest now",
            "--session", "proj-kw",
        ])
        .arg("--project")
        .arg(&project)
        .env("CLAUDE_PROJECT_DIR", &project)
        .output()
        .expect("run ways scan prompt");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Plugin First enforcement"),
        "keyword channel did not fire the project way.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn project_way_fires_via_semantic_when_model_present() {
    let Some(model_dir) = std::env::var_os("WAYS_TEST_MODEL_DIR").map(PathBuf::from) else {
        eprintln!("SKIP: set WAYS_TEST_MODEL_DIR (way-embed + minilm-l6-v2.gguf) to run the semantic leg");
        return;
    };

    let env = Env::new("semantic");
    let project = fixture_project();

    // Stage the engine (binary + model) into the isolated cache so auto-embed
    // can generate vectors without touching the real corpus.
    let engine_dir = env.xdg_cache.join("agent-ways/user");
    std::fs::create_dir_all(&engine_dir).unwrap();
    for entry in std::fs::read_dir(&model_dir).expect("read WAYS_TEST_MODEL_DIR") {
        let entry = entry.unwrap();
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            std::fs::copy(entry.path(), engine_dir.join(entry.file_name())).unwrap();
        }
    }

    let mut gen = Command::new(ways_bin());
    env.apply(&mut gen);
    assert_corpus_ok(
        gen.args(["corpus"]).env("CLAUDE_PROJECT_DIR", &project),
        "semantic",
    );

    // A semantic-only query: shares vocabulary with the way but does NOT match
    // its `(?i)zqxplugintest` keyword pattern, so a hit proves the embedding
    // lookup found the namespaced project entry (Bug B).
    let mut scan = Command::new(ways_bin());
    env.apply(&mut scan);
    let out = scan
        .args([
            "scan", "prompt",
            "--query", "build this feature as a registered extension module",
            "--session", "proj-sem",
        ])
        .arg("--project")
        .arg(&project)
        .env("CLAUDE_PROJECT_DIR", &project)
        .output()
        .expect("run ways scan prompt");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Plugin First enforcement"),
        "semantic channel did not fire the project way.\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn crlf_authored_project_way_still_fires() {
    // A way authored on Windows has CRLF line endings. The scan-side collector
    // must recognize its frontmatter (line-based, not `starts_with("---\n")`),
    // or the way is silently skipped and never fires — keyword OR semantic.
    let env = Env::new("crlf");
    let project = env.base.join("crlf-project");
    let way_dir = project.join(".claude/ways/projdomain/crlfway");
    std::fs::create_dir_all(&way_dir).unwrap();
    let body = "---\r\ndescription: crlf authored way\r\nvocabulary: crlf windows\r\n\
                pattern: (?i)zqxcrlftest\r\nscope: agent\r\nrefire: 0.15\r\n---\r\n\
                # CRLF Way\r\n\r\nThis way was authored with Windows line endings.\r\n";
    std::fs::write(way_dir.join("crlfway.md"), body).unwrap();

    let mut scan = Command::new(ways_bin());
    env.apply(&mut scan);
    let out = scan
        .args([
            "scan", "prompt",
            "--query", "please zqxcrlftest now",
            "--session", "proj-crlf",
        ])
        .arg("--project")
        .arg(&project)
        .env("CLAUDE_PROJECT_DIR", &project)
        .output()
        .expect("run ways scan prompt");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("CRLF Way"),
        "CRLF-authored way was skipped by the collector (scan exit={:?}).\nstdout: {stdout}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ways_dir_with_output_does_not_clobber_canonical() {
    let env = Env::new("clobber");

    // Seed a sentinel "canonical" corpus.
    let canonical = env.corpus_jsonl();
    std::fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    let sentinel = "{\"id\":\"sentinel/global\",\"description\":\"x\",\"vocabulary\":\"x\"}\n";
    std::fs::write(&canonical, sentinel).unwrap();

    // An isolated build from an arbitrary ways-dir, redirected via --output.
    let empty_ways = env.base.join("ad-hoc-ways");
    std::fs::create_dir_all(&empty_ways).unwrap();
    let out_dir = env.base.join("isolated-out");

    let mut cmd = Command::new(ways_bin());
    env.apply(&mut cmd);
    assert_corpus_ok(
        cmd.args(["corpus", "--ways-dir"])
            .arg(&empty_ways)
            .arg("--output")
            .arg(&out_dir),
        "clobber/--output",
    );

    // Canonical corpus must be untouched; the isolated output must be written.
    assert_eq!(
        std::fs::read_to_string(&canonical).unwrap(),
        sentinel,
        "Bug C: --output build must not overwrite the canonical corpus"
    );
    assert!(
        out_dir.join("ways-corpus.jsonl").is_file(),
        "Bug C: --output corpus was not written to the requested dir"
    );
}

#[test]
fn ways_dir_without_output_warns_about_canonical() {
    let env = Env::new("warn");
    let empty_ways = env.base.join("ad-hoc-ways");
    std::fs::create_dir_all(&empty_ways).unwrap();

    let mut cmd = Command::new(ways_bin());
    env.apply(&mut cmd);
    let out = cmd
        .args(["corpus", "--quiet", "--ways-dir"])
        .arg(&empty_ways)
        .output()
        .expect("run ways corpus --ways-dir");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("WARNING") && stderr.contains("--output"),
        "Bug C: expected a footgun warning steering to --output (corpus exit={:?}).\nstdout: {}\nstderr: {stderr}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
}

/// ADR-143: a user way in $XDG_CONFIG shadows a same-named core way (dedup-by-name),
/// and a unique user way is embedded alongside core ways.
#[test]
fn user_way_shadows_core_way() {
    let env = Env::new("shadow");
    // core ways at ~/.claude/hooks/ways (the default core root).
    let core = env.home.join(".claude/hooks/ways");
    Env::write_way(&core, "meta/foo", "CORE foo about deployment", "deploy ci release");
    Env::write_way(&core, "meta/bar", "core bar about migrations", "migrate schema sql");
    // user ways at $XDG_CONFIG/agent-ways/ways — foo shadows core foo, baz is unique.
    let user = env.xdg_config.join("agent-ways/ways");
    Env::write_way(&user, "meta/foo", "USER foo override about kubernetes", "k8s pod cluster");
    Env::write_way(&user, "meta/baz", "user baz personal notes", "note journal todo");

    let mut cmd = Command::new(ways_bin());
    env.apply(&mut cmd);
    assert_corpus_ok(cmd.args(["corpus"]), "shadow");

    let ids = corpus_ids(&env.corpus_jsonl());
    assert_eq!(
        ids.iter().filter(|i| *i == "meta/foo").count(),
        1,
        "meta/foo must appear exactly once (user shadows core, not double-embedded): {ids:?}"
    );
    assert!(ids.iter().any(|i| i == "meta/bar"), "core-only bar present: {ids:?}");
    assert!(ids.iter().any(|i| i == "meta/baz"), "user-only baz present: {ids:?}");

    let content = std::fs::read_to_string(env.corpus_jsonl()).unwrap();
    assert!(content.contains("USER foo override"), "the surviving foo must be the user's");
    assert!(!content.contains("CORE foo about"), "core foo must be shadowed out of the corpus");
}
