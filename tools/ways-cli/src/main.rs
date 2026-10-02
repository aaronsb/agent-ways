use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// Pin to one session id instead of guessing the transcript from cwd
        #[arg(long)]
        session: Option<String>,
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
        /// Ways root directory (default: ~/.claude/hooks/ways, or $XDG_DATA_HOME/agent-ways/hooks/ways before the projection exists)
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
        /// Only regenerate if corpus is stale (newer way files exist, or a
        /// failed build is due a retry). The SessionStart hook runs this:
        /// a failed embedding pass is reported on stderr and exits 0.
        #[arg(long)]
        if_stale: bool,
    },
    /// Diagnose how a query matches ways under the live late-interaction matcher
    /// (ADR-160): peak · share · body-confirm · fired, per candidate — the tool for
    /// authoring a way against how it actually fires.
    Match {
        /// The query string to match
        query: String,
        /// Project directory (for project-local ways; default: current)
        #[arg(long)]
        project: Option<String>,
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
    /// Scaffold a new way file (frontmatter and a body template). Ways are authored English-only; locale stubs come from ways-localize (ADR-139)
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
        /// When a projection root is already a real directory or file, rename
        /// it to a timestamped sibling (<name>.ways-backup-<seconds>) instead
        /// of stopping. Never deletes.
        #[arg(long)]
        force: bool,
    },
    /// Introspect a session: which ways fired, on which turn, and why
    /// (ADR-153/154). Replay or follow it live on screen, list sessions, or
    /// dump them as JSON.
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
    /// Serve one Claude Code hook: read its JSON payload on stdin and print
    /// what the hook returns (ADR-504 §11). The scripts under hooks/ways call
    /// this and nothing else.
    Hook {
        #[arg(value_enum)]
        event: cmd::hook::HookEvent,
    },
    /// Print the per-user sessions root directory, for scripts the binary does
    /// not run (macros and postchecks get it as WAYS_SESSIONS_ROOT).
    SessionsRoot,
    /// Print the canonical events-log path (`paths::events_log()`), which
    /// every telemetry writer and reader resolves.
    EventsLogPath,
    /// Print the directory name Claude Code gives a project under its projects
    /// dir (`claude_sessions::project_slug`). Defaults to `CLAUDE_PROJECT_DIR`,
    /// else the working directory. Lets shell macros read per-project state
    /// without re-deriving the rule.
    ProjectSlug {
        /// Project path
        path: Option<String>,
    },
    /// Claude Code's projects: list, search, show, stats, cleanup, hygiene and
    /// relocate (ADR-504). With no subcommand, lists them.
    Projects {
        #[command(subcommand)]
        command: Option<cmd::projects::ProjectsCommand>,
    },
    /// The ways agent: API keys, the judge's engine and mode (ADR-196, ADR-502).
    /// Runs `ways-agent`; `ways agent --help` lists its commands.
    #[command(disable_help_flag = true)]
    Agent {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Read and change settings through their files (ADR-503); alone on a terminal, the settings screens. Exit codes: 0 done, 2 usage or unknown key, 3 rejected, 4 overridden, 5 write failed
    #[command(disable_help_subcommand = true, args_conflicts_with_subcommands = true)]
    Settings {
        /// Open the screens on this tab: ways, matching, gate, install or theme
        tab: Option<String>,
        /// The project whose .claude/ways.yaml the screens read and write
        #[arg(long)]
        project: Option<PathBuf>,
        /// Test only: feed this key script to the screens, headless; repeatable. It cannot type into a masked entry
        #[arg(long, hide = true, action = clap::ArgAction::Append, allow_hyphen_values = true)]
        keys: Vec<String>,
        /// Print the frame at WIDTHxHEIGHT, headless, in the test kit's frame format
        #[arg(long, hide = true)]
        snap: Option<String>,
        /// The colour depth to draw at: truecolor, 256, 16 or none
        #[arg(long, hide = true)]
        depth: Option<String>,
        #[command(subcommand)]
        action: Option<SettingsCommand>,
    },
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
    /// Remove agent-ways: withdraw from every target, stop the agent, unlink the
    /// commands and delete the app. Your config and state stay unless --purge.
    /// Prints the plan and changes nothing without --yes.
    Uninstall {
        /// Do it. Without this flag the plan is printed and nothing changes.
        #[arg(long)]
        yes: bool,
        /// Also delete your config (your ways, API keys, settings) and state (events).
        #[arg(long)]
        purge: bool,
    },
}

#[derive(Subcommand)]
enum IntrospectCommand {
    /// Replay a session's way firings frame by frame on screen, or print the
    /// timeline as JSON with `--json`.
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
        /// Print the reconstructed timeline as JSON, with a session summary,
        /// the relevance gate's work and the near-miss events (the most recent
        /// session in scope without --session)
        #[arg(long, conflicts_with_all = ["speed", "keys", "snap", "depth"])]
        json: bool,
        /// Feed these keys to the screens, headless (tokens as `ways settings --keys`)
        #[arg(long, hide = true, num_args = 1..)]
        keys: Vec<String>,
        /// Print the screens at WIDTHxHEIGHT in the test kit's frame format, headless
        #[arg(long, hide = true)]
        snap: Option<String>,
        /// Colour depth to draw at: truecolor, 256, 16 or none
        #[arg(long, hide = true)]
        depth: Option<String>,
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
        /// Feed these keys to the screens, headless (tokens as `ways settings --keys`)
        #[arg(long, hide = true, num_args = 1..)]
        keys: Vec<String>,
        /// Print the screens at WIDTHxHEIGHT in the test kit's frame format, headless
        #[arg(long, hide = true)]
        snap: Option<String>,
        /// Colour depth to draw at: truecolor, 256, 16 or none
        #[arg(long, hide = true)]
        depth: Option<String>,
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
        /// Transcript path from the hook payload. Fired ways read the session's
        /// model id from it (stamped on the event as `model`) and its refire
        /// window; without it the binary locates the transcript by session id.
        #[arg(long)]
        transcript: Option<String>,
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
        /// Transcript path from the hook payload (model id and refire window
        /// for fired ways; see `scan prompt`).
        #[arg(long)]
        transcript: Option<String>,
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
        /// Transcript path from the hook payload (model id and refire window
        /// for fired ways; see `scan prompt`).
        #[arg(long)]
        transcript: Option<String>,
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
        /// The submitted prompt, when the invoking event is UserPromptSubmit.
        /// A harness envelope (Monitor notification, task hand-back, skill
        /// body) is not an operator turn, so the state lane skips it the way
        /// the prompt lane does.
        #[arg(long)]
        query: Option<String>,
        /// Hook event that invoked this scan (recorded as the envelope's
        /// `hookEventName`; the `hookSpecificOutput` shape itself is
        /// canonical for every event). When omitted, falls back to
        /// `SessionStart` and emits a stderr trace — `check-state.sh` already
        /// applies the same fallback via jq, so a missing value here means
        /// neither layer received `hook_event_name` and the recorded event
        /// name may be wrong for the actual invoking event.
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
enum SettingsCommand {
    /// Print a key's value in effect
    Get {
        key: String,
        /// The value with its layer, default and file
        #[arg(long)]
        json: bool,
        /// Read this file alone instead of the live layers
        #[arg(long)]
        file: Option<PathBuf>,
        /// The project whose .claude/ways.yaml is layered (default: CLAUDE_PROJECT_DIR, else the working directory)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Write a key; prints nothing on success
    Set {
        key: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        /// Write the project's .claude/ways.yaml instead of the user file
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Remove a key, so the layer below applies
    Unset {
        key: String,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// key=value lines for every key under a prefix
    List {
        prefix: Option<String>,
        /// A file-shaped fragment and each key's layer, default and file; the stored view unless --effective
        #[arg(long)]
        json: bool,
        /// With --json: every key as resolved, defaults included
        #[arg(long)]
        effective: bool,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// What a key or a section does
    Help { topic: Option<String> },
    /// Print the canonical fragment under a section or prefix, shaped like its file
    Emit {
        prefix: Option<String>,
        /// The values in effect instead of the canonical ones
        #[arg(long)]
        effective: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Check the settings files against the schema; exit 3 with findings
    Lint {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Write a settings object (YAML or JSON, from stdin or --file) and answer with a JSON report
    Apply {
        #[arg(long)]
        file: Option<PathBuf>,
        /// Write nothing; report the fragment that would be written
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Rewrite one section of a file from canonical
    Fix {
        section: String,
        /// Fix the project's .claude/ways.yaml instead of the user file
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Initialize user config at XDG path
    Init,
    /// Show the configuration (ADR-185: a table; --json the stored file; --json --effective the resolved state)
    Show {
        #[arg(long)]
        json: bool,
        /// With --json: the resolved configuration with defaults applied, rather than the stored file
        #[arg(long)]
        effective: bool,
    },
    /// Show config file paths
    Path,
    /// List projection targets: the Claude Code config directories agent-ways is active in (ADR-184)
    Targets {
        #[arg(long)]
        json: bool,
    },
    /// Manage one projection target (ADR-184)
    Target {
        #[command(subcommand)]
        action: TargetCommand,
    },
}

#[derive(Subcommand)]
enum TargetCommand {
    /// Preview what activating a config directory would link, merge, refuse, or remove
    Plan {
        /// Claude Code config directory (e.g. ~/.claude, or what CLAUDE_CONFIG_DIR names)
        dir: String,
        #[arg(long)]
        json: bool,
    },
    /// Activate a config directory: record it as a target, then reconcile into it. Stops on a blocked plan unless --force
    Add {
        dir: String,
        /// Move real paths at projection roots aside and proceed past a blocked plan
        #[arg(long)]
        force: bool,
        /// Show the plan and stop
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Re-enable a recorded target and reconcile into it
    Enable { dir: String },
    /// Disable a recorded target: withdraw our links and hooks, keep the record
    Disable { dir: String },
    /// Withdraw from a target and drop its record
    Remove { dir: String },
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
        Commands::Context { project, session, json } => cmd::context::run(project.as_deref(), session.as_deref(), json),
        Commands::Lint { path, schema, check, fix, all, global } => cmd::lint::run(path, schema, check, fix, all, global),
        Commands::Reflow { path, fix, json, quiet } => cmd::reflow::run(path, fix, json, quiet),
        Commands::Corpus { ways_dir, output, quiet, verbose, if_stale } => cmd::corpus::run(ways_dir, output, quiet, verbose, if_stale),
        Commands::Match { query, project } => cmd::match_cmd::run_late(query, project.as_deref()),
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
        Commands::Reconcile { source, dest, mode, dry_run, quiet, force } => {
            cmd::reconcile::run(source, dest, mode, dry_run, quiet, force)
        }
        Commands::Introspect { mode } => match mode {
            IntrospectCommand::Replay { session, project, all, speed, json, keys, snap, depth } => {
                let open = cmd::introspect::Open { keys, snap, depth };
                cmd::introspect::replay(session.as_deref(), project.as_deref(), all, speed, json, &open)
            }
            IntrospectCommand::List { project, all, json } => {
                cmd::introspect::list(project.as_deref(), all, json)
            }
            IntrospectCommand::Dump { session, project, all } => {
                cmd::introspect::dump(session.as_deref(), project.as_deref(), all)
            }
            IntrospectCommand::Live { session, project, keys, snap, depth } => {
                let open = cmd::introspect::Open { keys, snap, depth };
                cmd::introspect::live(session.as_deref(), project.as_deref(), &open)
            }
            IntrospectCommand::Fires { session, project, all, max_score, limit } => {
                cmd::introspect::fires(session.as_deref(), project.as_deref(), all, max_score, limit)
            }
        },
        Commands::Status { json } => cmd::status::run(json),
        Commands::Scan { mode } => match mode {
            // ADR-184 item 6: a project (or user config) with `enabled: false`
            // injects nothing. Checked before any lane runs.
            ScanCommand::Prompt { query, session, project, response_context, transcript } => {
                if !cmd::scan::enabled_for(project.as_deref()) {
                    return Ok(());
                }
                cmd::scan::prompt(
                    &query,
                    &session,
                    project.as_deref(),
                    response_context.as_deref(),
                    transcript.as_deref(),
                )
            }
            ScanCommand::Messages { session, project, transcript } => {
                if !cmd::scan::enabled_for(project.as_deref()) {
                    return Ok(());
                }
                cmd::scan::messages(&session, project.as_deref(), transcript.as_deref())
            }
            ScanCommand::Command { command, description, session, project, transcript } => {
                if !cmd::scan::enabled_for(project.as_deref()) {
                    return Ok(());
                }
                cmd::scan::command(
                    &command,
                    description.as_deref(),
                    &session,
                    project.as_deref(),
                    transcript.as_deref(),
                )
            }
            ScanCommand::File { path, session, project, transcript } => {
                if !cmd::scan::enabled_for(project.as_deref()) {
                    return Ok(());
                }
                cmd::scan::file(&path, &session, project.as_deref(), transcript.as_deref())
            }
            ScanCommand::Task { query, session, project, team } => {
                if !cmd::scan::enabled_for(project.as_deref()) {
                    return Ok(());
                }
                cmd::scan::task(&query, &session, project.as_deref(), team.as_deref())
            }
            ScanCommand::State { session, project, transcript, query, hook_event } => {
                if !cmd::scan::enabled_for(project.as_deref()) {
                    return Ok(());
                }
                let event = hook_event.unwrap_or_else(|| {
                    eprintln!(
                        "[ways] scan state invoked without --hook-event; defaulting to SessionStart. \
                         The envelope shape is canonical for every event, but the recorded \
                         hookEventName will be wrong if the invoking hook is a different event."
                    );
                    "SessionStart".to_string()
                });
                cmd::scan::state(&session, project.as_deref(), transcript.as_deref(), &event, query.as_deref())
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
        Commands::Settings { tab, project, keys, snap, depth, action } => {
            use cmd::settings as st;
            cmd::settings::exit_with(match action {
                None if tab.is_none() && project.is_none() && keys.is_empty() && snap.is_none() => st::bare(),
                None => st::tui::open(&st::tui::Open { tab, project, keys, snap, depth }),
                Some(SettingsCommand::Get { key, json, file, project }) => st::get(&key, json, file.as_deref(), project.as_deref()),
                Some(SettingsCommand::Set { key, value, project }) => st::set(&key, &value, project.as_deref()),
                Some(SettingsCommand::Unset { key, project }) => st::unset(&key, project.as_deref()),
                Some(SettingsCommand::List { prefix, json, effective, file, project }) => {
                    st::list(prefix.as_deref(), json, effective, file.as_deref(), project.as_deref())
                }
                Some(SettingsCommand::Help { topic }) => st::help(topic.as_deref()),
                Some(SettingsCommand::Emit { prefix, effective, project }) => st::emit(prefix.as_deref(), effective, project.as_deref()),
                Some(SettingsCommand::Lint { file, project }) => st::lint(file.as_deref(), project.as_deref()),
                Some(SettingsCommand::Apply { file, dry_run, project }) => st::apply(file.as_deref(), dry_run, project.as_deref()),
                Some(SettingsCommand::Fix { section, project }) => st::fix(&section, project.as_deref()),
            })
        }
        Commands::Config { action } => match action {
            ConfigCommand::Init => {
                let path = config::Config::init_user_config();
                println!("wrote config to {}", path.display());
                Ok(())
            }
            ConfigCommand::Show { json, effective } => cmd::config_cmd::show(json, effective),
            ConfigCommand::Path => {
                println!("{}", config::Config::config_path());
                Ok(())
            }
            ConfigCommand::Targets { json } => cmd::config_cmd::targets(json),
            ConfigCommand::Target { action } => match action {
                TargetCommand::Plan { dir, json } => cmd::config_cmd::target_plan(&dir, json),
                TargetCommand::Add { dir, force, dry_run, json } => {
                    cmd::config_cmd::target_add(&dir, force, dry_run, json)
                }
                TargetCommand::Enable { dir } => cmd::config_cmd::target_enable(&dir),
                TargetCommand::Disable { dir } => cmd::config_cmd::target_disable(&dir),
                TargetCommand::Remove { dir } => cmd::config_cmd::target_remove(&dir),
            },
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
        Commands::Projects { command } => cmd::projects::run(command),
        Commands::ProjectSlug { path } => {
            let project = path.unwrap_or_else(util::project_dir);
            println!("{}", claude_sessions::project_slug(&project));
            Ok(())
        }
        Commands::Hook { event } => cmd::hook::run(event),
        Commands::Permissions { action, global } => {
            match action {
                PermissionsCommand::Audit => cmd::permissions::audit(global),
            }
        }
        Commands::Agent { args } => cmd::agent::run(&args),
        Commands::Update { dry_run, git_ref } => cmd::update::run(dry_run, git_ref),
        Commands::Uninstall { yes, purge } => cmd::uninstall::run(yes, purge),
    }
}
