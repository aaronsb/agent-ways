//! `ways projects`: list, search, show, summarize, clean up and relocate
//! Claude Code's projects (ADR-504). The port of `tools/claude-projects`.
//!
//! Reading goes through `claude-sessions`. `cleanup`, `hygiene` and
//! `relocate` are the only writers into `~/.claude/projects`: `cleanup` and
//! `hygiene` ask before removing anything, move what they remove into a
//! `.trash-<stamp>` dir, and take `--dry-run`; `relocate` previews unless
//! given `--execute`.
//!
//! Output is plain text. Colour waits for agent-theme's ANSI output (#694).
//! [`screen`] shows `list` and `show` together on a terminal (#748).

mod fsio;
mod relocate;
mod rewrite;
pub(crate) mod screen;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::Result;
use clap::Subcommand;
use claude_sessions::ClaudeDir;
use serde_json::Value;

pub use relocate::RelocateArgs;

#[derive(Subcommand)]
pub enum ProjectsCommand {
    /// List projects, most recently active first (the default)
    #[command(visible_alias = "ls")]
    List(ListArgs),
    /// Search project paths, session summaries and first prompts
    #[command(visible_aliases = ["s", "find"])]
    Search {
        /// Search term
        query: String,
        /// Also search transcript content (slower)
        #[arg(long)]
        deep: bool,
        /// Machine-readable JSON output: every match, best first
        #[arg(long)]
        json: bool,
    },
    /// Show one project in detail
    #[command(visible_alias = "info")]
    Show {
        /// Project path or name fragment
        project: String,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Aggregate statistics
    Stats,
    /// Remove empty project directories (asks first)
    Cleanup {
        /// Show what would be removed, remove nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Large transcripts and empty session dirs (asks before moving dirs to a trash dir)
    Hygiene {
        /// Report only, remove nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Move a project's session history to a new working directory (previews unless --execute)
    #[command(visible_aliases = ["mv", "move"])]
    Relocate(RelocateArgs),
}

#[derive(clap::Args, Default)]
pub struct ListArgs {
    /// Only projects with sessions or transcripts
    #[arg(long)]
    active: bool,
    /// Only projects with memory
    #[arg(long)]
    memory: bool,
    /// Only empty projects (no sessions, no transcripts)
    #[arg(long)]
    stale: bool,
    /// Show clickable file:// URLs
    #[arg(long)]
    urls: bool,
    /// Machine-readable JSON output
    #[arg(long, conflicts_with = "urls")]
    json: bool,
}

/// Where the commands read and write: Claude Code's config dir and
/// `~/.claude.json`. Split out so tests run against a fixture tree.
pub struct Env {
    pub claude: ClaudeDir,
    pub claude_json: PathBuf,
    pub home: String,
    pub now: u64,
}

impl Env {
    fn user() -> Self {
        let home = ways_core::util::home_dir();
        Self {
            claude: ways_core::paths::claude_dir(),
            claude_json: home.join(".claude.json"),
            home: home.to_string_lossy().into_owned(),
            now: agent_fmt::when::now_secs(),
        }
    }

    fn projects(&self) -> PathBuf {
        self.claude.projects_dir()
    }

    /// `path` with a leading `~` spelled out as the home directory.
    fn untilde(&self, path: &str) -> String {
        match path.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", self.home),
            _ => path.to_string(),
        }
    }

    /// `path` with the home directory shown as `~`.
    fn tilde(&self, path: &str) -> String {
        match path.strip_prefix(&self.home) {
            Some(rest) if !self.home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
                format!("~{rest}")
            }
            _ => path.to_string(),
        }
    }
}

/// Run a `ways projects` command. `None` is `list` with no filters.
pub fn run(command: Option<ProjectsCommand>) -> Result<()> {
    let env = Env::user();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let stdin = std::io::stdin();
    let mut confirm = |prompt: &str| -> bool {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        stdin.lock().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
    };
    let ok = dispatch(&env, command, &mut out, &mut confirm)?;
    out.flush()?;
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

/// The commands against an explicit environment. `Ok(false)` means the
/// command failed in a way the operator must see in the exit status.
pub fn dispatch(
    env: &Env,
    command: Option<ProjectsCommand>,
    out: &mut dyn Write,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<bool> {
    match command.unwrap_or(ProjectsCommand::List(ListArgs::default())) {
        ProjectsCommand::List(args) => list(env, &args, out)?,
        ProjectsCommand::Search { query, deep, json } => search(env, &query, deep, json, out)?,
        ProjectsCommand::Show { project, json } => show(env, &project, json, out)?,
        ProjectsCommand::Stats => stats(env, out)?,
        ProjectsCommand::Cleanup { dry_run } => cleanup(env, dry_run, out, confirm)?,
        ProjectsCommand::Hygiene { dry_run } => hygiene(env, dry_run, out, confirm)?,
        ProjectsCommand::Relocate(args) => return relocate::relocate(env, &args, out),
    }
    Ok(true)
}

// ── Scanning ────────────────────────────────────────────────────

/// What `ways projects` knows about one project directory.
struct Project {
    dirname: String,
    /// The project path, `~`-abbreviated.
    path: String,
    /// Entries in `sessions-index.json`, which older Claude Code versions wrote.
    sessions: usize,
    transcripts: usize,
    transcript_bytes: u64,
    memory_files: usize,
    first_active: Option<u64>,
    last_active: Option<u64>,
    last_summary: String,
    last_branch: String,
    recent_prompts: Vec<String>,
    /// The index entries, newest first.
    entries: Vec<Value>,
    /// Whether the project has a `sessions-index.json`.
    indexed: bool,
}

impl Project {
    fn is_empty(&self) -> bool {
        self.sessions == 0 && self.transcripts == 0
    }

    /// The project as `list --json` gives it; `show --json` adds its
    /// sessions with `sessions` set. A value the project lacks is `null`.
    fn json(&self, env: &Env, sessions: bool) -> Value {
        let time = |e: Option<u64>| e.map(agent_fmt::when::utc_iso);
        let text = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let mut v = serde_json::json!({
            "path": self.path,
            "absolute_path": env.untilde(&self.path),
            "dir": self.dirname,
            "sessions": self.sessions,
            "transcripts": self.transcripts,
            "transcript_bytes": self.transcript_bytes,
            "memory_files": self.memory_files,
            "first_active": time(self.first_active),
            "last_active": time(self.last_active),
            "last_branch": text(&self.last_branch),
            "last_summary": text(&self.last_summary),
            "recent_prompts": self.recent_prompts,
        });
        if sessions {
            v["session_list"] = self
                .entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "modified": text(str_field(e, "modified")),
                        "messages": e.get("messageCount"),
                        "branch": text(str_field(e, "gitBranch")),
                        "sidechain": e.get("isSidechain").and_then(Value::as_bool).unwrap_or(false),
                        "summary": text(str_field(e, "summary")),
                    })
                })
                .collect();
        }
        v
    }
}

fn read_index(dir: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("sessions-index.json")).ok()?).ok()
}

fn index_entries(index: &Option<Value>) -> Vec<Value> {
    index
        .as_ref()
        .and_then(|i| i.get("entries"))
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default()
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|s| s.as_str()).unwrap_or("")
}

/// The display path when nothing names the real one: `-home-u-a-b` reads as
/// `~/a/b`, as `claude-projects` showed it.
fn heuristic_path(dirname: &str) -> String {
    let name = dirname.trim_start_matches('-');
    if let Some(rest) = name.strip_prefix("home-") {
        let parts: Vec<&str> = rest.split('-').skip(1).collect();
        return if parts.is_empty() { "~".to_string() } else { format!("~/{}", parts.join("/")) };
    }
    dirname.to_string()
}

fn scan_project(env: &Env, dir: &Path) -> Project {
    let dirname = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = claude_sessions::resolve_project_path(&env.projects(), &dirname)
        .map(|p| env.tilde(&p))
        .unwrap_or_else(|| heuristic_path(&dirname));

    let mut entries = index_entries(&read_index(dir));
    entries.sort_by(|a, b| str_field(b, "modified").cmp(str_field(a, "modified")));

    let transcripts = claude_sessions::transcripts_in(dir);
    let metas: Vec<std::fs::Metadata> =
        transcripts.iter().filter_map(|t| std::fs::metadata(t).ok()).collect();
    let transcript_bytes = metas.iter().map(|m| m.len()).sum();

    let mut p = Project {
        dirname,
        path,
        sessions: entries.len(),
        transcripts: transcripts.len(),
        transcript_bytes,
        memory_files: 0,
        first_active: None,
        last_active: None,
        last_summary: String::new(),
        last_branch: String::new(),
        recent_prompts: Vec::new(),
        entries: Vec::new(),
        indexed: dir.join("sessions-index.json").exists(),
    };

    if let (Some(latest), Some(oldest)) = (entries.first(), entries.last()) {
        p.last_active = date_epoch(str_field(latest, "modified"));
        p.first_active = date_epoch(str_field(oldest, "created"));
        p.last_summary = str_field(latest, "summary").to_string();
        p.last_branch = str_field(latest, "gitBranch").to_string();
        p.recent_prompts = entries
            .iter()
            .take(3)
            .map(|e| str_field(e, "firstPrompt"))
            .filter(|fp| !fp.is_empty())
            .map(|fp| take_chars(fp, 80))
            .collect();
    }
    if p.last_active.is_none() {
        p.last_active = metas
            .iter()
            .filter_map(|m| m.modified().ok())
            .max()
            .map(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    }
    p.entries = entries;

    if let Ok(rd) = std::fs::read_dir(dir.join("memory")) {
        p.memory_files = rd
            .flatten()
            .filter(|e| e.path().is_file() && e.file_name() != ".gitkeep")
            .count();
    }
    p
}

/// Every project, most recently active first.
fn scan_all(env: &Env) -> Vec<Project> {
    let mut projects: Vec<Project> = claude_sessions::project_dirs_in(&env.projects())
        .iter()
        .map(|d| scan_project(env, d))
        .collect();
    projects.sort_by_key(|p| std::cmp::Reverse(p.last_active));
    projects
}

// ── Formatting ──────────────────────────────────────────────────

/// The first ten characters of an ISO timestamp (its date) as epoch seconds
/// at midnight UTC.
fn date_epoch(ts: &str) -> Option<u64> {
    let date = ts.get(..10)?;
    agent_fmt::when::parse_utc_iso(&format!("{date}T00:00:00Z"))
}

fn fmt_date(epoch: u64) -> String {
    agent_fmt::when::utc_date(epoch)
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}G", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1e3)
    } else {
        format!("{n}B")
    }
}

/// A relative age: `today`, `3d`, `2w`, `5mo`, `1y`.
fn age(now: u64, then: Option<u64>) -> String {
    let Some(then) = then else { return String::new() };
    let days = now.saturating_sub(then) / 86_400;
    match days {
        0 => "today".to_string(),
        1..=6 => format!("{days}d"),
        7..=29 => format!("{}w", days / 7),
        30..=364 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn file_url(env: &Env, display: &str) -> String {
    match display.strip_prefix('~') {
        Some(rest) => format!("file://{}{rest}", env.home),
        None => format!("file://{display}"),
    }
}

fn pad_left(s: &str, w: usize) -> String {
    format!("{s:>w$}")
}

/// `path` cut from the left to `w` characters, `…` marking the cut.
fn ellipsize_left(path: &str, w: usize) -> String {
    let n = path.chars().count();
    if n > w {
        format!("…{}", path.chars().skip(n - (w - 1)).collect::<String>())
    } else {
        path.to_string()
    }
}

/// The `list` cells after the path: sessions, size, last active, memory.
fn list_cells(env: &Env, p: &Project) -> [String; 4] {
    let size = if p.transcript_bytes > 0 { fmt_bytes(p.transcript_bytes) } else { "–".to_string() };
    let last = match age(env.now, p.last_active) {
        a if a.is_empty() => "–".to_string(),
        a => a,
    };
    let mem = if p.memory_files > 0 { "●" } else { "·" };
    [session_cell(p), size, last, mem.to_string()]
}

/// The session column: index sessions, else transcripts as `Nt`, else `–`.
fn session_cell(p: &Project) -> String {
    if p.sessions > 0 {
        p.sessions.to_string()
    } else if p.transcripts > 0 {
        format!("{}t", p.transcripts)
    } else {
        "–".to_string()
    }
}

// ── Commands ────────────────────────────────────────────────────

fn list(env: &Env, args: &ListArgs, out: &mut dyn Write) -> Result<()> {
    let projects: Vec<Project> = scan_all(env)
        .into_iter()
        .filter(|p| {
            if args.active {
                !p.is_empty()
            } else if args.memory {
                p.memory_files > 0
            } else if args.stale {
                p.is_empty()
            } else {
                true
            }
        })
        .collect();
    if args.json {
        let all: Vec<Value> = projects.iter().map(|p| p.json(env, false)).collect();
        writeln!(out, "{}", serde_json::to_string_pretty(&all)?)?;
        return Ok(());
    }
    if projects.is_empty() {
        writeln!(out, "No matching projects found.")?;
        return Ok(());
    }

    let path_w = agent_fmt::terminal_width().saturating_sub(36).max(20);
    writeln!(out, "\nClaude Code Projects  ({} shown)\n", projects.len())?;
    writeln!(out, "  {:<path_w$} {:>8} {:>6} {:>7} {:>6}", "Project", "Sessions", "Size", "Last", "Memory")?;
    writeln!(out, "  {} {} {} {} {}", "─".repeat(path_w), "─".repeat(8), "─".repeat(6), "─".repeat(7), "─".repeat(6))?;

    for p in &projects {
        let [sess, size, last, mem] = list_cells(env, p);
        let cells = format!("{} {} {} {}", pad_left(&sess, 8), pad_left(&size, 6), pad_left(&last, 7), pad_left(&mem, 6));
        if args.urls {
            writeln!(out, "  {}", file_url(env, &p.path))?;
            writeln!(out, "    {cells}")?;
        } else {
            let path = ellipsize_left(&p.path, path_w);
            let pad = path_w.saturating_sub(path.chars().count());
            writeln!(out, "  {path}{} {cells}", " ".repeat(pad))?;
        }
    }
    writeln!(out)?;
    Ok(())
}

fn search(env: &Env, query: &str, deep: bool, json: bool, out: &mut dyn Write) -> Result<()> {
    let q = query.to_lowercase();
    let mut matches: Vec<(u32, Project, Vec<String>)> = Vec::new();

    for p in scan_all(env) {
        let (mut score, mut snippets) = shallow_match(&p, &q);
        if deep && score == 0 {
            let (hits, snippet) = deep_search(&env.projects().join(&p.dirname), &q);
            score += hits.min(5);
            snippets.extend(snippet);
        }
        if score > 0 {
            matches.push((score, p, snippets));
        }
    }

    // Stable: equal scores keep the most-recent-first order.
    matches.sort_by_key(|m| std::cmp::Reverse(m.0));
    if json {
        let all: Vec<Value> = matches
            .iter()
            .map(|(score, p, snippets)| {
                let mut v = p.json(env, false);
                v["score"] = (*score).into();
                v["snippets"] = snippets.clone().into();
                v
            })
            .collect();
        writeln!(out, "{}", serde_json::to_string_pretty(&all)?)?;
        return Ok(());
    }
    if matches.is_empty() {
        let hint = if deep { "" } else { " (try --deep to search transcript content)" };
        writeln!(out, "No projects matching '{query}'{hint}")?;
        return Ok(());
    }

    let mode = if deep { "Deep search" } else { "Search" };
    writeln!(out, "\n{mode}: {query}  ({} matches)\n", matches.len())?;
    for (_, p, snippets) in matches.iter().take(15) {
        let last = match age(env.now, p.last_active) {
            a if a.is_empty() => "–".to_string(),
            a => a,
        };
        let sess = if p.sessions > 0 { format!("{}s", p.sessions) } else { session_cell(p) };
        writeln!(out, "  {}", file_url(env, &p.path))?;
        writeln!(out, "    {sess} · {last}")?;
        for s in snippets.iter().take(2) {
            writeln!(out, "    › {s}")?;
        }
    }
    writeln!(out)?;
    Ok(())
}

/// How well `p` matches `q` (lowercased) on its path, session summaries
/// and first prompts, and the snippets that matched. Zero is no match.
fn shallow_match(p: &Project, q: &str) -> (u32, Vec<String>) {
    let mut score = 0u32;
    let mut snippets: Vec<String> = Vec::new();
    let path_l = p.path.to_lowercase();
    if path_l.contains(q) || p.dirname.to_lowercase().contains(q) {
        score += 10;
    }
    for word in q.split_whitespace() {
        if path_l.contains(word) {
            score += 5;
        }
    }
    if p.indexed {
        for e in &p.entries {
            let summary = str_field(e, "summary");
            let first = str_field(e, "firstPrompt");
            if summary.to_lowercase().contains(q) {
                score += 5;
                snippets.push(take_chars(summary, 80));
            } else if first.to_lowercase().contains(q) {
                score += 3;
                snippets.push(take_chars(first, 80));
            }
        }
    } else if let Some(prompt) = p.recent_prompts.iter().find(|pr| pr.to_lowercase().contains(q)) {
        score += 2;
        snippets.push(take_chars(prompt, 80));
    }
    (score, snippets)
}

/// Case-insensitive search of every file under a project directory: the
/// number of files holding `q` (lowercased), and a snippet from the first
/// matching transcript.
fn deep_search(dir: &Path, q: &str) -> (u32, Option<String>) {
    let q = q.as_bytes().to_ascii_lowercase();
    if q.is_empty() {
        return (0, None);
    }
    let mut hits = 0u32;
    let mut snippet = None;
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(file) = std::fs::File::open(entry.path()) else { continue };
        let mut reader = std::io::BufReader::new(file);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
            let lower = line.to_ascii_lowercase();
            if let Some(at) = lower.windows(q.len()).position(|w| w == q.as_slice()) {
                hits += 1;
                let is_transcript = entry.path().extension().and_then(|e| e.to_str()) == Some("jsonl");
                if snippet.is_none() && is_transcript {
                    let start = at.saturating_sub(40);
                    let end = (at + q.len() + 40).min(line.len());
                    let text = String::from_utf8_lossy(&line[start..end]);
                    snippet = Some(format!("…{}…", take_chars(text.trim(), 80)));
                }
                break;
            }
            line.clear();
        }
    }
    (hits, snippet)
}

fn show(env: &Env, query: &str, json: bool, out: &mut dyn Write) -> Result<()> {
    // The query's best match, most recently active first at each step: the
    // project whose path or directory is the query, then the one it names
    // (its last path component), then the first whose path contains it. A
    // path under the home directory is compared in the `~` form projects
    // are shown in, so the one a shell expanded matches.
    let q = env.tilde(query).to_lowercase();
    let all = scan_all(env);
    let name = |p: &Project| p.path.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    let found = all
        .iter()
        .find(|p| p.path.to_lowercase() == q || p.dirname.to_lowercase() == q)
        .or_else(|| all.iter().find(|p| name(p) == q))
        .or_else(|| all.iter().find(|p| p.path.to_lowercase().contains(&q) || p.dirname.to_lowercase().contains(&q)));
    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(&found.map(|p| p.json(env, true)))?)?;
        return Ok(());
    }
    let Some(p) = found else {
        writeln!(out, "No project matching '{query}'")?;
        return Ok(());
    };
    out.write_all(b"\n")?;
    show_project(env, p, out)
}

/// What `show` prints for one project, after its leading blank line.
fn show_project(env: &Env, p: &Project, out: &mut dyn Write) -> Result<()> {
    writeln!(out, "{}", file_url(env, &p.path))?;
    writeln!(out, "  Dir: {}\n", p.dirname)?;
    if let Some(first) = p.first_active {
        writeln!(out, "  First session: {}", fmt_date(first))?;
    }
    if let Some(last) = p.last_active {
        writeln!(out, "  Last active:  {} ({})", fmt_date(last), age(env.now, Some(last)))?;
    }
    if !p.last_branch.is_empty() {
        writeln!(out, "  Last branch:  {}", p.last_branch)?;
    }
    writeln!(out)?;
    writeln!(out, "  Sessions:     {}", p.sessions)?;
    writeln!(out, "  Transcripts:  {}  ({})", p.transcripts, fmt_bytes(p.transcript_bytes))?;
    let files = if p.memory_files > 0 { format!(" ({} files)", p.memory_files) } else { String::new() };
    writeln!(out, "  Memory:       {}{files}\n", if p.memory_files > 0 { "yes" } else { "no" })?;

    if !p.last_summary.is_empty() {
        writeln!(out, "  Last summary: {}\n", p.last_summary)?;
    }
    if !p.recent_prompts.is_empty() {
        writeln!(out, "  Recent first prompts:")?;
        for prompt in &p.recent_prompts {
            writeln!(out, "    › {prompt}")?;
        }
        writeln!(out)?;
    }
    if !p.entries.is_empty() {
        writeln!(out, "  Sessions:")?;
        for e in p.entries.iter().take(10) {
            let date = take_chars(str_field(e, "modified"), 10);
            let msgs = e.get("messageCount").map(|m| m.to_string()).unwrap_or_else(|| "?".to_string());
            let branch = str_field(e, "gitBranch");
            let side = if e.get("isSidechain").and_then(|s| s.as_bool()) == Some(true) { " ⑂" } else { "" };
            writeln!(out, "    {date}  {msgs:>3} msgs  {branch:<20}{side}")?;
            let summary = take_chars(str_field(e, "summary"), 50);
            if !summary.is_empty() {
                writeln!(out, "      {summary}")?;
            }
        }
        if p.entries.len() > 10 {
            writeln!(out, "    … +{} more", p.entries.len() - 10)?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn stats(env: &Env, out: &mut dyn Write) -> Result<()> {
    let projects = scan_all(env);
    let count = |f: &dyn Fn(&Project) -> bool| projects.iter().filter(|p| f(p)).count();
    writeln!(out, "\nClaude Code Project Statistics\n")?;
    writeln!(out, "  Projects:          {}", projects.len())?;
    writeln!(out, "    with sessions:   {}", count(&|p| p.sessions > 0))?;
    writeln!(out, "    with transcripts:{}", count(&|p| p.transcripts > 0))?;
    writeln!(out, "    with memory:     {}", count(&|p| p.memory_files > 0))?;
    writeln!(out, "    empty:           {}\n", count(&|p| p.is_empty()))?;
    writeln!(out, "  Total sessions:    {}", projects.iter().map(|p| p.sessions).sum::<usize>())?;
    writeln!(out, "  Total transcripts: {}", projects.iter().map(|p| p.transcripts).sum::<usize>())?;
    writeln!(out, "  Total disk:        {}\n", fmt_bytes(projects.iter().map(|p| p.transcript_bytes).sum()))?;

    let mut by_size: Vec<&Project> = projects.iter().collect();
    by_size.sort_by_key(|p| std::cmp::Reverse(p.transcript_bytes));
    writeln!(out, "  Top by disk usage:")?;
    for p in by_size.iter().take(10).take_while(|p| p.transcript_bytes > 0) {
        writeln!(out, "    {:>6}  {}", fmt_bytes(p.transcript_bytes), p.path)?;
    }
    writeln!(out)?;

    let mut by_sessions: Vec<&Project> = projects.iter().collect();
    by_sessions.sort_by_key(|p| std::cmp::Reverse(p.sessions));
    writeln!(out, "  Top by session count:")?;
    for p in by_sessions.iter().take(10).take_while(|p| p.sessions > 0) {
        writeln!(out, "    {:>4}  {}", p.sessions, p.path)?;
    }
    writeln!(out)?;
    Ok(())
}

/// Does `dir` hold any file at any depth?
fn holds_files(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file()).count()
}

fn cleanup(
    env: &Env,
    dry_run: bool,
    out: &mut dyn Write,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<()> {
    let empty: Vec<Project> = scan_all(env).into_iter().filter(Project::is_empty).collect();
    if empty.is_empty() {
        writeln!(out, "No empty projects to clean up.")?;
        return Ok(());
    }
    let projects = env.projects();
    writeln!(out, "\nEmpty projects ({} found)", empty.len())?;
    writeln!(
        out,
        "  These are metadata entries in {}/, NOT your actual project directories.\n",
        env.tilde(&projects.to_string_lossy())
    )?;
    for p in &empty {
        writeln!(out, "  {}  →  {}", p.path, projects.join(&p.dirname).display())?;
    }
    if dry_run {
        writeln!(out, "\n  (dry run — pass without --dry-run to remove)")?;
        return Ok(());
    }
    writeln!(out)?;
    out.flush()?;
    let prompt = format!(
        "  Remove {} empty entries from {}/? [y/N] ",
        empty.len(),
        env.tilde(&projects.to_string_lossy())
    );
    if !confirm(&prompt) {
        writeln!(out, "  Cancelled.")?;
        return Ok(());
    }

    // Removed entries go to a trash dir, so a mistaken `y` is undone by
    // moving them back.
    let trash = fsio::trash_dir(&projects);
    let mut removed = 0;
    for p in &empty {
        let dir = projects.join(&p.dirname);
        // Claude Code creates a project's dir as a session starts, before
        // its first transcript: a dir touched this recently may be one.
        if recently_modified(&dir, env.now) {
            writeln!(out, "  skipped {} (modified in the last {} minutes)", p.path, RECENT_SECS / 60)?;
            continue;
        }
        // Only a directory with no file at any depth is removed.
        match holds_files(&dir) {
            0 => match fsio::move_to_trash(&dir, &trash, Path::new(&p.dirname)) {
                Ok(_) => {
                    removed += 1;
                    writeln!(out, "  removed {}", p.path)?;
                }
                Err(e) => writeln!(out, "  error {}: {e}", p.path)?,
            },
            n => writeln!(out, "  skipped {} (has {n} files)", p.path)?,
        }
    }
    writeln!(out, "\n  Removed {removed} directories.")?;
    if removed > 0 {
        writeln!(out, "  They are in {}; move them back to restore.", trash.display())?;
    }
    Ok(())
}

/// A project dir modified this recently may belong to a starting session.
const RECENT_SECS: u64 = 300;

fn recently_modified(dir: &Path, now: u64) -> bool {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .map(|t| now.saturating_sub(t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)) < RECENT_SECS)
        .unwrap_or(false)
}

/// One finding of `hygiene`.
struct Issue {
    project: String,
    file: String,
    size: u64,
    path: PathBuf,
}

/// Large transcripts and empty session dirs. Transcripts missing from
/// `sessions-index.json` are not reported: Claude Code stopped writing that
/// index, so every current transcript is missing from it. Empty dirs are
/// moved to a trash dir after confirmation; transcripts are never touched.
fn hygiene(
    env: &Env,
    dry_run: bool,
    out: &mut dyn Write,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<()> {
    let projects = scan_all(env);
    let total_disk: u64 = projects.iter().map(|p| p.transcript_bytes).sum();
    let (mut large, mut empty_dirs) = (Vec::new(), Vec::new());

    for p in &projects {
        let dir = env.projects().join(&p.dirname);
        for t in claude_sessions::transcripts_in(&dir) {
            let size = std::fs::metadata(&t).map(|m| m.len()).unwrap_or(0);
            let file = t.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if size > 10_000_000 {
                large.push(Issue { project: p.path.clone(), file, size, path: t });
            }
        }
        for sub in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = sub.path();
            if path.is_dir()
                && sub.file_name() != "memory"
                && std::fs::read_dir(&path).map(|mut r| r.next().is_none()).unwrap_or(false)
                // A live session creates its tool-results and subagents dirs
                // empty; one made moments ago may be about to fill.
                && !recently_modified(&path, env.now)
            {
                let file = format!("{}/{}", p.dirname, sub.file_name().to_string_lossy());
                empty_dirs.push(Issue { project: p.path.clone(), file, size: 0, path });
            }
        }
    }

    if large.is_empty() && empty_dirs.is_empty() {
        writeln!(out, "\nAll clean! No hygiene issues found across {} projects.\n", projects.len())?;
        return Ok(());
    }

    writeln!(out, "\nSession Hygiene Report\n")?;
    writeln!(out, "  Total disk usage: {}", fmt_bytes(total_disk))?;
    writeln!(out, "  Projects scanned: {}\n", projects.len())?;

    if !large.is_empty() {
        large.sort_by_key(|i| std::cmp::Reverse(i.size));
        let total: u64 = large.iter().map(|i| i.size).sum();
        writeln!(out, "  Large transcripts (>10MB): {}  ({} total)", large.len(), fmt_bytes(total))?;
        for i in large.iter().take(10) {
            writeln!(out, "    {:>6}  {}", fmt_bytes(i.size), i.project)?;
            writeln!(out, "           {}", i.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())?;
        }
        if large.len() > 10 {
            writeln!(out, "    … +{} more", large.len() - 10)?;
        }
        writeln!(out)?;
    }
    if !empty_dirs.is_empty() {
        writeln!(out, "  Empty session dirs: {}\n", empty_dirs.len())?;
    }

    if dry_run {
        writeln!(out, "  (pass without --dry-run to clean up)")?;
        return Ok(());
    }

    out.flush()?;
    if !empty_dirs.is_empty()
        && confirm(&format!("  Move {} empty session directories to the trash? [y/N] ", empty_dirs.len()))
    {
        let trash = fsio::trash_dir(&env.projects());
        let mut restore: Vec<String> = Vec::new();
        for i in &empty_dirs {
            match fsio::move_to_trash(&i.path, &trash, Path::new(&i.file)) {
                // Each goes back into its project dir, which still exists.
                Ok(dest) => restore.push(format!(
                    "    mv {} {}/",
                    relocate::shell_quote(&dest.to_string_lossy()),
                    relocate::shell_quote(&i.path.parent().unwrap_or(&i.path).to_string_lossy())
                )),
                Err(e) => writeln!(out, "  error {}: {e}", i.path.display())?,
            }
        }
        writeln!(out, "  Moved {} directories to {}. To restore:", restore.len(), trash.display())?;
        for line in &restore {
            writeln!(out, "{line}")?;
        }
    }
    writeln!(out)?;
    Ok(())
}

#[cfg(test)]
mod tests;
