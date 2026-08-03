use anyhow::Result;
use clap::{Parser, Subcommand};

// The shared engine now lives in the `ways-core` library crate (ADR-151).
// Re-export it at the crate root so existing `crate::util::…`, `crate::paths::…`,
// etc. paths across the command modules keep resolving unchanged.
pub use ways_core::{agents, config, frontmatter, paths, scanner, util};

mod cmd;
pub mod session;

/// Full version string for `--version`: the semver plus the baked `git describe`
/// build provenance (ADR-150), e.g. `1.0.0 (ways-v1.0.0-78-gc595437)`. This makes
/// a dev build visibly and machine-readably distinct from a release build — the
/// signal `ways update`'s downgrade guard and `download-ways.sh` rely on.
const LONG_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("WAYS_BUILD"), ")");

#[derive(Parser)]
#[command(
    name = "ways",
    version,
    long_version = LONG_VERSION,
    about = "Unified CLI for ways knowledge guidance"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Context window usage — accurate token counts from transcript
    Context {
        /// Project directory (default: detect from cwd or CLAUDE_PROJECT_DIR)
        #[arg(long)]
        project: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Validate way frontmatter against the schema
    Lint {
        /// Path to scan (default: project ways if in project, else global)
        path: Option<String>,
        /// Show the frontmatter schema reference
        #[arg(long)]
        schema: bool,
        /// Exit non-zero on errors (for CI)
        #[arg(long)]
        check: bool,
        /// Auto-fix what can be fixed, in the file or directory given as `path`
        ///
        /// Scope comes from `path`, not from this flag. Without a path, `--fix`
        /// refuses to run rather than rewriting the whole corpus by surprise;
        /// pass `--all` to ask for that deliberately.
        #[arg(long)]
        fix: bool,
        /// Allow `--fix` to write across the entire resolved corpus
        #[arg(long)]
        all: bool,
        /// Scan global ways (ignore CLAUDE_PROJECT_DIR)
        #[arg(long)]
        global: bool,
    },
    /// Detect (or repair) hard-wrapped markdown prose
    ///
    /// Exits 0 when clean and 1 when wrapped prose is found, matching lint
    /// convention. Reads stdin when no path is given.
    Reflow {
        /// Markdown file to inspect (default: read stdin)
        path: Option<String>,
        /// Rewrite the file, backing the original up first
        #[arg(long)]
        fix: bool,
        /// Emit findings as JSON
        #[arg(long)]
        json: bool,
        /// Suppress human-readable output (exit code only)
        #[arg(long)]
        quiet: bool,
    },
    /// Generate the ways corpus for matching engines
    Corpus {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Output directory for corpus artifacts (default: canonical XDG cache).
        /// Use with --ways-dir for an isolated build that won't clobber the
        /// canonical user corpus.
        #[arg(long)]
        output: Option<String>,
        /// Suppress progress output
        #[arg(long, short)]
        quiet: bool,
        /// Trace every phase (paths, per-project scans, embed passes, calibration
        /// lanes) and stream way-embed's per-way progress. Overrides --quiet.
        /// Reach for this when a build appears to hang: the last line printed
        /// names the step it stalled in.
        #[arg(long, short)]
        verbose: bool,
        /// Only regenerate if corpus is stale (newer way files exist)
        #[arg(long)]
        if_stale: bool,
    },
    /// Diagnose how a query matches ways under the live late-interaction matcher
    /// (ADR-160): peak · share · body-confirm · fired, per candidate — the tool for
    /// authoring a way against how it actually fires. `--cosine` shows the legacy
    /// single-vector view (which no longer reflects the fire path).
    Match {
        /// The query string to match
        query: String,
        /// Path to corpus JSONL (single-vector `--cosine` view only)
        #[arg(long)]
        corpus: Option<String>,
        /// Show the legacy single-vector cosine view instead of late-interaction
        #[arg(long)]
        cosine: bool,
        /// Project directory (for project-local ways; default: current)
        #[arg(long)]
        project: Option<String>,
    },
    /// Score a query against ways using embedding similarity
    Embed {
        /// The query string to match
        query: String,
        /// Path to corpus JSONL
        #[arg(long)]
        corpus: Option<String>,
        /// Path to GGUF model file
        #[arg(long)]
        model: Option<String>,
    },
    /// Score way-vs-way cosine similarity
    Siblings {
        /// Way ID to compare (or "all" for full matrix)
        id: String,
        /// Minimum similarity threshold to display
        #[arg(long, default_value = "0.3")]
        threshold: f64,
        /// Path to corpus JSONL
        #[arg(long)]
        corpus: Option<String>,
        /// Path to GGUF model file
        #[arg(long)]
        model: Option<String>,
    },
    /// Export ways as a JSONL graph (nodes + edges)
    Graph {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Output file (default: stdout)
        #[arg(long, short)]
        output: Option<String>,
    },
    /// Analyze progressive disclosure tree structure
    Tree {
        /// Way path or short name (e.g., "supplychain" or full path)
        path: String,
        /// Show Jaccard similarity between siblings
        #[arg(long)]
        jaccard: bool,
    },
    /// Display a way, check, or core guidance (session-aware)
    Show {
        #[command(subcommand)]
        what: ShowCommand,
    },
    /// Analyze a way file and suggest vocabulary improvements
    Suggest {
        /// Path to a way file
        file: String,
        /// Minimum term frequency for suggestions
        #[arg(long, default_value = "2")]
        min_freq: u32,
    },
    /// Initialize project .claude/ways/ structure and MEMORY.md seed (ADR-128)
    Init {
        /// Project directory (default: CLAUDE_PROJECT_DIR or cwd)
        #[arg(long)]
        project: Option<String>,
    },
    /// Scaffold a new way with frontmatter, body template, and locale stubs
    Template {
        /// Way path relative to ways root (e.g., "softwaredev/code/newway")
        path: String,
        /// Description — what this way covers, in natural language
        #[arg(long, short)]
        description: String,
        /// Vocabulary — space-separated domain keywords users would say
        #[arg(long, short = 'V')]
        vocabulary: Option<String>,
        /// Scope: agent, subagent, teammate (comma-separated)
        #[arg(long, default_value = "agent")]
        scope: String,
        /// Create in global ways (~/.claude/hooks/ways/) instead of project-local
        #[arg(long)]
        global: bool,
    },
    /// Language coverage report — models, stubs, and per-way embed routing
    Language {
        /// Filter to ways supporting this language (code or name)
        #[arg(long)]
        filter: Option<String>,
        /// Show full per-way coverage detail (default shows uncovered summary)
        #[arg(long)]
        audit: bool,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Usage statistics from event log
    Stats {
        /// Last N days only
        #[arg(long)]
        days: Option<u32>,
        /// Filter to specific project path (default: CLAUDE_PROJECT_DIR)
        #[arg(long)]
        project: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
        /// Show stats across all projects (ignore CLAUDE_PROJECT_DIR)
        #[arg(long)]
        global: bool,
    },
    /// List ways triggered in the current session with epoch and disclosure state
    List {
        /// Session ID (if omitted, auto-detects current session)
        #[arg(long)]
        session: Option<String>,
        /// Sort order: epoch (default, conversation order), name, distance
        #[arg(long, default_value = "epoch")]
        sort: String,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Print the projection manifest — the desired state of ~/.claude derived
    /// from `git ls-files` over the projection allowlist (ADR-144). The
    /// reconciler converges ~/.claude toward this.
    Manifest {
        /// Source checkout to derive from (default: $XDG_DATA/agent-ways)
        #[arg(long)]
        source: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Converge ~/.claude toward the projection manifest (ADR-144). Symlink
    /// mode (default) links each projection root into the source checkout, so a
    /// `git pull` in $XDG_DATA is live with no further step. Idempotent;
    /// silent when already up to date.
    Reconcile {
        /// Source checkout (default: $XDG_DATA/agent-ways)
        #[arg(long)]
        source: Option<String>,
        /// Projection target (default: ~/.claude)
        #[arg(long)]
        dest: Option<String>,
        /// Materialization: symlink (default) or copy
        #[arg(long)]
        mode: Option<String>,
        /// Show what would change without touching the filesystem
        #[arg(long)]
        dry_run: bool,
        /// Suppress the summary line (still prints any changes)
        #[arg(long)]
        quiet: bool,
    },
    /// Preview (or perform) migration of a legacy in-place ~/.claude clone to
    /// the XDG application layout (ADR-144 §5). Default is a read-only plan;
    /// --execute is gated and backed up.
    Migrate {
        /// Show the read-only migration plan (the default; accepted explicitly)
        #[arg(long)]
        plan: bool,
        /// Dry-run the executor: assert every phase's contract without mutating
        #[arg(long)]
        what_if: bool,
        /// Perform the migration (gated; backs up first, resumable)
        #[arg(long)]
        execute: bool,
        /// Target install dir (default: ~/.claude)
        #[arg(long)]
        dest: Option<String>,
    },
    /// Replay a session's way-firing history as an interactive animation
    Rethink {
        /// Session ID to replay directly (skip picker)
        #[arg(long)]
        session: Option<String>,
        /// Filter to sessions from this project path
        #[arg(long)]
        project: Option<String>,
        /// Replay across every project, not just the current one (default is the
        /// current project; detection failure is an error, never a silent
        /// globalize)
        #[arg(long)]
        all: bool,
        /// Initial frame speed in milliseconds (default: 1000)
        #[arg(long)]
        speed: Option<u64>,
        /// List sessions (non-interactive)
        #[arg(long)]
        list: bool,
        /// Dump the reconstructed timeline as JSON instead of animating it
        /// (non-interactive; includes a session summary and near-miss events)
        #[arg(long)]
        json: bool,
    },
    /// Introspect a session over the SessionIntrospection model: which ways
    /// fired, on which turn, and why (ADR-153/154). Currently: the `dump` mode.
    Introspect {
        #[command(subcommand)]
        mode: IntrospectCommand,
    },
    /// Engine health dashboard — binary, model, corpus, project status
    Status {
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Scan ways and output matched content (replaces hook scan loops)
    Scan {
        #[command(subcommand)]
        mode: ScanCommand,
    },
    /// Reset session state when ways stop firing or fire incorrectly.
    ///
    /// Clears markers, epoch counters, and check fire counts from /tmp.
    /// Use when: a way should fire but doesn't (stale marker), checks
    /// fire too aggressively (inflated epoch), or after debugging the
    /// way tree. Default is dry run — add --confirm to actually delete.
    Reset {
        /// Target a specific session ID
        #[arg(long)]
        session: Option<String>,
        /// Clear all sessions (not just the current one)
        #[arg(long)]
        all: bool,
        /// Actually delete (default is dry run that shows what would be cleared)
        #[arg(long)]
        confirm: bool,
    },
    /// Print the per-session response-topics state file path (single source
    /// of truth for the Stop hook writer, the UserPromptSubmit consumer, and
    /// `ways reset`'s side-file cleanup).
    ResponseTopicsPath {
        /// Session ID
        session: String,
    },
    /// Print the per-user sessions root directory. Lets the hook scripts (and
    /// CI) verify they resolve the same path as the binary on every platform —
    /// the contract documented in `hooks/ways/sessions-root.sh`.
    SessionsRoot,
    /// Print the canonical events-log path. The single source of truth for every
    /// telemetry writer — the Rust `session::log_event` and the shell hooks
    /// (`clear-markers.sh`, `inject-subagent.sh` via `events-log.sh`) — and every
    /// reader (`firing::load_events`). Resolving through the binary keeps the
    /// hooks from hardcoding a path that drifts from `paths::events_log()` after
    /// the ADR-142 XDG migration (ADR-153 §1).
    EventsLogPath,
    /// Manage configuration (init/show/path)
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Disable a way in this project (ADR-131 — writes .claude/ways.yaml)
    Disable {
        /// Way ID to disable (e.g., "itops/incident"). Omit when using --list.
        #[arg(required_unless_present = "list", conflicts_with = "list")]
        name: Option<String>,
        /// List currently disabled ways in this project
        #[arg(long, conflicts_with = "name")]
        list: bool,
        /// With --list, emit bare names one-per-line (machine-readable, no decoration)
        #[arg(long, requires = "list")]
        names_only: bool,
    },
    /// Re-enable a way previously disabled in this project (ADR-131)
    Enable {
        /// Way ID to enable (e.g., "itops/incident")
        name: String,
    },
    /// Audit locale alias fidelity + discrimination (ADR-125 — flags stubs to re-author)
    Tune {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Filter to ways matching this substring (e.g., "security", "ea/")
        #[arg(long)]
        way: Option<String>,
        /// Audit only this language code (default: the active localized language)
        #[arg(long)]
        lang: Option<String>,
        /// Minimum cross-lingual cosine to accept for fidelity (default: 0.60)
        #[arg(long, default_value = "0.60")]
        fidelity_threshold: f64,
        /// Minimum discrimination gap (min_peer − top_confuser.score);
        /// entries below this are flagged as being outranked by another way.
        /// Default 0.03 — small positive margin required.
        #[arg(long, default_value = "0.03")]
        discrimination_threshold: f64,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Audit fire relevance — flag ways landing in off-domain sessions (ADR-134 Decision 3)
    TunePrecision {
        /// Minimum sessions a way must have fired in before it's flagged
        #[arg(long, default_value = "5")]
        min_sessions: usize,
        /// Off-class rate at or above which a way is flagged (0.0–1.0)
        #[arg(long, default_value = "0.5")]
        flag_threshold: f64,
        /// Filter to events whose project path contains this substring
        #[arg(long)]
        project: Option<String>,
        /// Filter to ways whose id contains this substring
        #[arg(long)]
        way: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Permission audit — diff requires: fields against settings.json grants (ADR-116)
    Permissions {
        #[command(subcommand)]
        action: PermissionsCommand,
        /// Scan global ways (ignore project-local)
        #[arg(long, global = true)]
        global: bool,
    },
    /// Update the agent-ways install — pull, refresh binaries (pre-built first), regenerate corpus, reproject
    Update {
        /// Show what would run without executing it
        #[arg(long)]
        dry_run: bool,
        /// Pin the install to a specific branch, tag, or commit and build the
        /// whole suite from source, instead of pulling the latest release.
        /// Leaves the release channel until `ways update --ref main`.
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
    },
}

#[derive(Subcommand)]
enum IntrospectCommand {
    /// Interactively replay a session's way firings (the animated TUI).
    Replay {
        /// Session ID to replay directly (default: pick interactively)
        #[arg(long)]
        session: Option<String>,
        /// Scope to this project path (default: current project)
        #[arg(long)]
        project: Option<String>,
        /// Consider sessions from every project, not just the current one
        #[arg(long)]
        all: bool,
        /// Initial frame speed in milliseconds (default: 1000)
        #[arg(long)]
        speed: Option<u64>,
    },
    /// List candidate sessions in scope (table, or `--json` for an agent).
    List {
        /// Scope to this project path (default: current project)
        #[arg(long)]
        project: Option<String>,
        /// List sessions from every project, not just the current one
        #[arg(long)]
        all: bool,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Dump a session's reconstructed introspection as JSON (agent-facing):
    /// turns, fired ways, their criteria, keyed transcript join, and matched
    /// spans. Defaults to the most recent session in the current project.
    Dump {
        /// Session ID to dump (default: most recent in scope)
        #[arg(long)]
        session: Option<String>,
        /// Scope to this project path (default: current project)
        #[arg(long)]
        project: Option<String>,
        /// Pick the session across every project, not just the current one
        #[arg(long)]
        all: bool,
    },
    /// Live-monitor the current session's way firings, following the newest frame
    /// as ways fire (the replay TUI, refreshing on a tick). Defaults to the most
    /// recent session in the current project.
    Live {
        /// Session ID to monitor (default: most recent in the current project)
        #[arg(long)]
        session: Option<String>,
        /// Scope to this project path (default: current project)
        #[arg(long)]
        project: Option<String>,
    },
    /// List semantic way fires as `score · surface · way`, read straight from
    /// events.jsonl (no introspection model) — the read-side precision instrument:
    /// eyeball whether each fire matched the surface it fired on. Defaults to the
    /// most recent session in the current project, borderline (lowest-score) first.
    Fires {
        /// Session ID to read (default: most recent in scope)
        #[arg(long)]
        session: Option<String>,
        /// Scope to this project path (default: current project)
        #[arg(long)]
        project: Option<String>,
        /// Pick the session across every project, not just the current one
        #[arg(long)]
        all: bool,
        /// Only show fires at or below this score (surface the suspect tail)
        #[arg(long)]
        max_score: Option<f64>,
        /// Cap the number of rows shown (default: all)
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum ScanCommand {
    /// Scan ways against a user prompt (keyword + semantic matching)
    Prompt {
        /// User prompt text (lowercase)
        #[arg(long)]
        query: String,
        /// Session ID
        #[arg(long)]
        session: String,
        /// Project directory
        #[arg(long)]
        project: Option<String>,
        /// Claude's last response (raw), from the Stop hook (ADR-155 §3).
        /// Feeds only the embed lane — never the keyword regex lane.
        #[arg(long)]
        response_context: Option<String>,
    },
    /// Scan queued mid-turn operator messages from the transcript (ADR-161).
    /// Aggregates every `queue-operation`/`enqueue` newer than the per-session
    /// scan mark into one surface and matches it like a prompt. Runs on
    /// PostToolUse, where UserPromptSubmit never fired for these messages.
    Messages {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Project directory
        #[arg(long)]
        project: Option<String>,
        /// Transcript path (from the PostToolUse hook input)
        #[arg(long)]
        transcript: Option<String>,
    },
    /// Scan ways against a bash command
    Command {
        /// Command string
        #[arg(long)]
        command: String,
        /// Tool description
        #[arg(long)]
        description: Option<String>,
        /// Session ID
        #[arg(long)]
        session: String,
        /// Project directory
        #[arg(long)]
        project: Option<String>,
    },
    /// Scan ways against a file path
    File {
        /// File path being edited
        #[arg(long)]
        path: String,
        /// Session ID
        #[arg(long)]
        session: String,
        /// Project directory
        #[arg(long)]
        project: Option<String>,
    },
    /// Scan ways for subagent/teammate injection (writes stash for SubagentStart)
    Task {
        /// Task prompt text (lowercase)
        #[arg(long)]
        query: String,
        /// Session ID
        #[arg(long)]
        session: String,
        /// Project directory
        #[arg(long)]
        project: Option<String>,
        /// Team name (if teammate spawn)
        #[arg(long)]
        team: Option<String>,
    },
    /// Evaluate state-based triggers (context-threshold, file-exists, session-start)
    State {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Project directory
        #[arg(long)]
        project: Option<String>,
        /// Transcript path (for context-threshold)
        #[arg(long)]
        transcript: Option<String>,
        /// Hook event that invoked this scan (drives output envelope shape).
        /// `hookSpecificOutput` is canonical for all events; `PreToolUse`
        /// is the only event still taking the legacy top-level
        /// `additionalContext` shape. When omitted, falls back to
        /// `SessionStart` and emits a stderr trace — `check-state.sh` already
        /// applies the same fallback via jq, so a missing value here means
        /// neither layer received `hook_event_name` and the envelope shape
        /// may be wrong for the actual invoking event.
        #[arg(long)]
        hook_event: Option<String>,
    },
}

#[derive(Subcommand)]
enum ShowCommand {
    /// Display a way (session-aware, idempotent)
    Way {
        /// Way ID (e.g., "softwaredev/code/quality")
        id: String,
        /// Session ID
        #[arg(long)]
        session: String,
        /// Trigger channel (keyword, semantic:embedding)
        #[arg(long, default_value = "unknown")]
        trigger: String,
    },
    /// Display a check (with scoring curve)
    Check {
        /// Way ID containing the check
        id: String,
        /// Session ID
        #[arg(long)]
        session: String,
        /// Trigger channel
        #[arg(long, default_value = "unknown")]
        trigger: String,
        /// Match score from the matching engine
        #[arg(long, default_value = "0")]
        score: f64,
    },
    /// Display core guidance (session start)
    Core {
        /// Session ID
        #[arg(long)]
        session: String,
    },
    /// Display guidance for an attend signal (ADR-114)
    Attend {
        /// Signal type (e.g., "context-pressure", "build-complete")
        signal: String,
        /// Session ID
        #[arg(long)]
        session: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Initialize user config at XDG path
    Init,
    /// Show resolved configuration
    Show,
    /// Show config file paths
    Path,
}

#[derive(Subcommand)]
enum PermissionsCommand {
    /// Audit requires: fields against settings.json grants
    Audit,
}

fn main() -> Result<()> {
    // Windows' default main-thread stack is 1 MB; Linux's is 8 MB. Some commands
    // (e.g. `corpus`, `scan`) use a large-enough startup frame to overflow 1 MB,
    // crashing the spawned release binary immediately with STATUS_STACK_OVERFLOW
    // (0xC00000FD) and no output — while in-process unit tests, which run on the
    // test harness's larger-stack threads, pass. Run the real work on a thread
    // with an explicit, generous stack (the same shape rustc uses for its own
    // main thread).
    //
    // Outcome is preserved on both panic profiles: a returned `Err` flows straight
    // out of `main` (Termination prints `Error: …`, exit 1). A panic under release
    // `panic = "abort"` (tools/Cargo.toml) aborts at the site — thread-agnostic, so
    // the same single message + abort exit as before; the `resume_unwind` arm is
    // only live under unwind (dev/test), where it re-propagates the panic out of
    // `main` without re-invoking the hook (no double "panicked at" line).
    let worker = std::thread::Builder::new()
        .name("ways-main".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .expect("spawn ways-main worker thread");
    match worker.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn run() -> Result<()> {
    // Show banner + help when invoked with no args or "help"
    let args: Vec<String> = std::env::args().collect();
    let bare = args.len() == 1;
    let help = args.len() == 2 && (args[1] == "help" || args[1] == "--help" || args[1] == "-h");
    if bare || help {
        cmd::banner::run()?;
        if bare {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            return Ok(());
        }
    }

    let cli = Cli::parse();

    let command = match cli.command {
        Some(cmd) => cmd,
        None => return Ok(()), // already handled above
    };

    match command {
        Commands::Context { project, json } => cmd::context::run(project.as_deref(), json),
        Commands::Lint { path, schema, check, fix, all, global } => cmd::lint::run(path, schema, check, fix, all, global),
        Commands::Reflow { path, fix, json, quiet } => cmd::reflow::run(path, fix, json, quiet),
        Commands::Corpus { ways_dir, output, quiet, verbose, if_stale } => cmd::corpus::run(ways_dir, output, quiet, verbose, if_stale),
        Commands::Match { query, corpus, cosine, project } => {
            if cosine {
                cmd::match_cmd::run(query, corpus)
            } else {
                cmd::match_cmd::run_late(query, project.as_deref())
            }
        }
        Commands::Embed { query, corpus, model } => cmd::embed::run(query, corpus, model),
        Commands::Siblings { id, threshold, corpus, model } => {
            cmd::siblings::run(id, threshold, corpus, model)
        }
        Commands::Graph { ways_dir, output } => cmd::graph::run(ways_dir, output),
        Commands::Tree { path, jaccard } => cmd::tree::run(path, jaccard),
        Commands::Init { project } => cmd::init::run(project.as_deref()),
        Commands::Template { path, description, vocabulary, scope, global } => {
            cmd::template::run(path, description, vocabulary, scope, global)
        }
        Commands::Language { filter, audit, json } => cmd::language::run(filter.as_deref(), audit, json),
        Commands::Stats { days, project, json, global } => {
            cmd::stats::run(days, project.as_deref(), json, global)
        }
        Commands::List { session, sort, json } => cmd::list::run(session.as_deref(), &sort, json),
        Commands::Manifest { source, json } => cmd::manifest::run(json, source),
        Commands::Migrate { plan: _, what_if, execute, dest } => {
            cmd::migrate::run(execute, what_if, dest)
        }
        Commands::Reconcile { source, dest, mode, dry_run, quiet } => {
            cmd::reconcile::run(source, dest, mode, dry_run, quiet, false)
        }
        Commands::Rethink { session, project, all, speed, list, json } => {
            eprintln!(
                "note: `ways rethink` is deprecated — use `ways introspect replay` \
                 (or `introspect list` / `introspect dump`). It still works for now."
            );
            match (list, json) {
                // `--list --json`: machine-listable session enumeration (ADR-154 §4).
                (true, true) => cmd::rethink_dump::run_list_json(project.as_deref(), all),
                // `--json`: dump one session's reconstructed timeline.
                (false, true) => cmd::rethink_dump::run_json(session.as_deref(), project.as_deref(), all),
                // interactive replay, or `--list` text.
                _ => cmd::rethink::run(session.as_deref(), project.as_deref(), speed, list, all),
            }
        }
        Commands::Introspect { mode } => match mode {
            IntrospectCommand::Replay { session, project, all, speed } => {
                cmd::introspect::replay(session.as_deref(), project.as_deref(), all, speed)
            }
            IntrospectCommand::List { project, all, json } => {
                cmd::introspect::list(project.as_deref(), all, json)
            }
            IntrospectCommand::Dump { session, project, all } => {
                cmd::introspect::dump(session.as_deref(), project.as_deref(), all)
            }
            IntrospectCommand::Live { session, project } => {
                cmd::introspect::live(session.as_deref(), project.as_deref())
            }
            IntrospectCommand::Fires { session, project, all, max_score, limit } => {
                cmd::introspect::fires(session.as_deref(), project.as_deref(), all, max_score, limit)
            }
        },
        Commands::Status { json } => cmd::status::run(json),
        Commands::Scan { mode } => match mode {
            ScanCommand::Prompt { query, session, project, response_context } => {
                cmd::scan::prompt(&query, &session, project.as_deref(), response_context.as_deref())
            }
            ScanCommand::Messages { session, project, transcript } => {
                cmd::scan::messages(&session, project.as_deref(), transcript.as_deref())
            }
            ScanCommand::Command { command, description, session, project } => {
                cmd::scan::command(&command, description.as_deref(), &session, project.as_deref())
            }
            ScanCommand::File { path, session, project } => {
                cmd::scan::file(&path, &session, project.as_deref())
            }
            ScanCommand::Task { query, session, project, team } => {
                cmd::scan::task(&query, &session, project.as_deref(), team.as_deref())
            }
            ScanCommand::State { session, project, transcript, hook_event } => {
                let event = hook_event.unwrap_or_else(|| {
                    eprintln!(
                        "[ways] scan state invoked without --hook-event; defaulting to SessionStart. \
                         If the invoking hook is not SessionStart/PreToolUse, the output envelope \
                         shape will be wrong and the agent will silently receive nothing."
                    );
                    "SessionStart".to_string()
                });
                cmd::scan::state(&session, project.as_deref(), transcript.as_deref(), &event)
            }
        },
        Commands::Show { what } => match what {
            ShowCommand::Way { id, session, trigger } => {
                let out = cmd::show::way(&id, &session, &trigger)?;
                if !out.is_empty() { print!("{out}"); }
                Ok(())
            }
            ShowCommand::Check { id, session, trigger, score } => {
                let out = cmd::show::check(&id, &session, &trigger, score)?;
                if !out.is_empty() { print!("{out}"); }
                Ok(())
            }
            ShowCommand::Core { session } => {
                let out = cmd::show::core(&session)?;
                if !out.is_empty() { print!("{out}"); }
                Ok(())
            }
            ShowCommand::Attend { signal, session } => {
                let out = cmd::show::attend(&signal, &session)?;
                if !out.is_empty() { print!("{out}"); }
                Ok(())
            }
        },
        Commands::Config { action } => match action {
            ConfigCommand::Init => {
                let path = config::Config::init_user_config();
                println!("wrote config to {}", path.display());
                Ok(())
            }
            ConfigCommand::Show => {
                // Intentionally loads fresh from disk (not config::global()) —
                // diagnostic command should always reflect current file state
                let project_dir = std::env::var("CLAUDE_PROJECT_DIR")
                    .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()));
                let cfg = config::Config::load(&project_dir);
                println!("{:#?}", cfg);
                Ok(())
            }
            ConfigCommand::Path => {
                println!("{}", config::Config::config_path());
                Ok(())
            }
        },
        Commands::Disable { name, list, names_only } => {
            if list {
                cmd::disable::list(names_only)
            } else {
                // clap guarantees name is Some here via required_unless_present
                cmd::disable::disable(&name.expect("clap enforces name when --list absent"))
            }
        }
        Commands::Enable { name } => cmd::disable::enable(&name),
        Commands::Suggest { file, min_freq } => cmd::suggest::run(file, min_freq),
        Commands::Tune { ways_dir, way, lang, fidelity_threshold, discrimination_threshold, json } => {
            cmd::tune::run(ways_dir, way, lang, fidelity_threshold, discrimination_threshold, json)
        }
        Commands::TunePrecision { min_sessions, flag_threshold, project, way, json } => {
            cmd::tune_precision::run(min_sessions, flag_threshold, project, way, json)
        }
        Commands::Reset { session, all, confirm } => {
            cmd::reset::run(session.as_deref(), all, confirm)
        }
        Commands::SessionsRoot => {
            println!("{}", session::sessions_root());
            Ok(())
        }
        Commands::EventsLogPath => {
            println!("{}", paths::events_log().display());
            Ok(())
        }
        Commands::ResponseTopicsPath { session } => {
            println!("{}", session::response_topics_path(&session).display());
            Ok(())
        }
        Commands::Permissions { action, global } => {
            match action {
                PermissionsCommand::Audit => cmd::permissions::audit(global),
            }
        }
        Commands::Update { dry_run, git_ref } => cmd::update::run(dry_run, git_ref),
    }
}
