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
    /// Engine health: binary, model, corpus, project
    Status {
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Read and change settings; the screens on a terminal (ADR-503)
    ///
    /// Read and change settings through their files (ADR-503); alone on a
    /// terminal, the settings screens. Exit codes: 0 done, 2 usage or unknown
    /// key, 3 rejected, 4 overridden, 5 write failed
    #[command(disable_help_subcommand = true, args_conflicts_with_subcommands = true)]
    Settings {
        /// Open the screens on this tab
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(cmd::settings::tui::tab_names()))]
        tab: Option<String>,
        /// The project whose .claude/ways.yaml the screens read and write
        #[arg(long)]
        project: Option<PathBuf>,
        /// Test only: feed this key and mouse script (`click:COL,ROW`, `wheel:up@COL,ROW`) to the screens, headless; repeatable. It cannot type into a masked entry
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
    /// Claude Code config directories agent-ways is active in (ADR-184)
    ///
    /// Run bare on a terminal, it opens the settings screens on their install
    /// tab; in a pipe, it prints this help.
    Target {
        #[command(subcommand)]
        action: Option<TargetCommand>,
    },
    /// The ways agent: keys, models and the daemon (ADR-502)
    ///
    /// Runs `ways-agent`; `ways agent --help` lists its commands. The engine,
    /// model and mode are settings: `ways settings list gate` (ADR-507). Run
    /// bare on a terminal, it opens the settings screens on their gate tab.
    #[command(disable_help_flag = true)]
    Agent {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Claude Code's projects and their session history (ADR-504)
    ///
    /// Claude Code's projects: list, search, show, stats, cleanup, hygiene and
    /// relocate (ADR-504). With no subcommand, it opens the projects screen on
    /// a terminal and lists them in a pipe.
    Projects {
        #[command(subcommand)]
        command: Option<cmd::projects::ProjectsCommand>,
    },
    /// This session and past ones: fired ways, replay, reset
    ///
    /// Run bare on a terminal, it opens the session screen; in a pipe, it
    /// prints this help.
    Session {
        #[command(subcommand)]
        action: Option<SessionCommand>,
    },
    /// Context-window usage for a session
    ///
    /// Token counts read from the session's transcript.
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
    /// Write and check ways
    #[command(arg_required_else_help = true)]
    Author {
        #[command(subcommand)]
        action: AuthorCommand,
    },
    /// Measure matching against telemetry and locales
    #[command(arg_required_else_help = true)]
    Tune {
        #[command(subcommand)]
        action: TuneCommand,
    },
    /// Set up .claude/ways/ in a project (ADR-128)
    ///
    /// Writes the project's .claude/ways/ structure and the MEMORY.md seed.
    Init {
        /// Project directory (default: CLAUDE_PROJECT_DIR or cwd)
        #[arg(long)]
        project: Option<String>,
    },
    /// Rebuild the matching corpus
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
    /// Update agent-ways: pull, refresh binaries, rebuild, reproject
    ///
    /// Update the agent-ways install — pull, refresh binaries (pre-built first),
    /// regenerate corpus, reproject
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
    /// Repair the projection into each target (ADR-144)
    ///
    /// Converges every enabled target toward the projection manifest and
    /// withdraws from every disabled one (ADR-144, ADR-184). Symlink mode
    /// (default) links each projection root into the source checkout, so a
    /// `git pull` in $XDG_DATA is live with no further step. Idempotent; silent
    /// when already up to date.
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
    /// Remove agent-ways; without --yes, print the plan only
    ///
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
    /// Serve one Claude Code hook: read its JSON payload on stdin and print
    /// what the hook returns (ADR-504 §11). The scripts under hooks/ways call
    /// this and nothing else.
    #[command(hide = true)]
    Hook {
        #[arg(value_enum)]
        event: cmd::hook::HookEvent,
    },
    /// Display a way, check, or core guidance (session-aware)
    #[command(hide = true)]
    Show {
        #[command(subcommand)]
        what: ShowCommand,
    },
    /// Scan ways and output matched content (replaces hook scan loops)
    #[command(hide = true)]
    Scan {
        #[command(subcommand)]
        mode: ScanCommand,
    },
    /// Look ways up on request, as one JSON object: the machine interface the
    /// MCP tools `ways_search`, `ways_read` and `ways_neighbors` call (ADR-701 §5)
    #[command(hide = true)]
    Lookup {
        /// Project directory (default: CLAUDE_PROJECT_DIR, else the working directory)
        #[arg(long, global = true)]
        project: Option<PathBuf>,
        #[command(subcommand)]
        what: LookupCommand,
    },
    /// Print the projection manifest — the desired state of ~/.claude derived
    /// from `git ls-files` over the projection allowlist (ADR-144). The
    /// reconciler converges ~/.claude toward this.
    #[command(hide = true)]
    Manifest {
        /// Source checkout to derive from (default: $XDG_DATA/agent-ways)
        #[arg(long)]
        source: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Print the directory name Claude Code gives a project under its projects
    /// dir (`claude_sessions::project_slug`). Defaults to `CLAUDE_PROJECT_DIR`,
    /// else the working directory. Lets shell macros read per-project state
    /// without re-deriving the rule.
    #[command(hide = true)]
    ProjectSlug {
        /// Project path
        path: Option<String>,
    },
    /// Print the per-user sessions root directory, for scripts the binary does
    /// not run (macros and postchecks get it as WAYS_SESSIONS_ROOT).
    #[command(hide = true)]
    SessionsRoot,
    /// Print the canonical events-log path (`paths::events_log()`), which
    /// every telemetry writer and reader resolves.
    #[command(hide = true)]
    EventsLogPath,
    /// Check the relevance judge's keys and, on a terminal, offer to add one.
    /// `ways update` runs this; the installer calls it.
    #[command(hide = true)]
    JudgeSetup,
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Ways fired in this session, with epoch and disclosure state
    Ways {
        /// Session ID (if omitted, auto-detects current session)
        #[arg(long)]
        session: Option<String>,
        /// Sort order: epoch (default, conversation order), name, distance
        #[arg(long, default_value = "epoch")]
        sort: String,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
        /// Also list the ways the relevance judge kept out
        #[arg(long)]
        matched: bool,
    },
    /// Replay a session's way firings frame by frame
    ///
    /// On screen, or the timeline as JSON with `--json`.
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
        /// With --json, also the ways the relevance judge kept out
        #[arg(long, requires = "json")]
        matched: bool,
        /// Feed these keys and clicks to the screens, headless (tokens as `ways settings --keys`)
        #[arg(long, hide = true, num_args = 1..)]
        keys: Vec<String>,
        /// Print the screens at WIDTHxHEIGHT in the test kit's frame format, headless
        #[arg(long, hide = true)]
        snap: Option<String>,
        /// Colour depth to draw at: truecolor, 256, 16 or none
        #[arg(long, hide = true)]
        depth: Option<String>,
    },
    /// List the sessions in scope
    ///
    /// A table, or `--json` for an agent.
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
    /// Dump a session's introspection as JSON, for an agent
    ///
    /// Turns, fired ways, their criteria, keyed transcript join, and matched
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
        /// Also list the ways the relevance judge kept out
        #[arg(long)]
        matched: bool,
    },
    /// Follow the current session's way firings as they happen
    ///
    /// The replay screen opened on it, following the newest frame as the
    /// session writes, with the project's sessions behind it. Defaults to
    /// the most recent session in the current project.
    Live {
        /// Session ID to monitor (default: most recent in the current project)
        #[arg(long)]
        session: Option<String>,
        /// Scope to this project path (default: current project)
        #[arg(long)]
        project: Option<String>,
        /// Feed these keys and clicks to the screens, headless (tokens as `ways settings --keys`)
        #[arg(long, hide = true, num_args = 1..)]
        keys: Vec<String>,
        /// Print the screens at WIDTHxHEIGHT in the test kit's frame format, headless
        #[arg(long, hide = true)]
        snap: Option<String>,
        /// Colour depth to draw at: truecolor, 256, 16 or none
        #[arg(long, hide = true)]
        depth: Option<String>,
    },
    /// List semantic fires as score, surface and way
    ///
    /// Read straight from events.jsonl, with no introspection model: check
    /// whether each fire matched the surface it fired on. Defaults to the most
    /// recent session in the current project, lowest score first.
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
        /// Also list the ways the relevance judge kept out
        #[arg(long)]
        matched: bool,
        /// Machine-readable JSON output, for an agent tuning a way
        #[arg(long)]
        json: bool,
    },
    /// Switch ways off or on for this session's subagents and teammates
    ///
    /// Without on or off, reports which switch is in effect: this session's,
    /// the project's or user's `subagents:` setting, or the default (on).
    /// Holds until switched back or the session's state is cleared.
    Subagents {
        /// on or off; omit to report
        #[arg(value_parser = ["on", "off"])]
        state: Option<String>,
        /// Session ID (if omitted, auto-detects current session)
        #[arg(long)]
        session: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Clear session markers when ways stop firing or fire wrongly
    ///
    /// Reset session state when ways stop firing or fire incorrectly. Clears
    /// markers, epoch counters, and check fire counts from /tmp. Use when: a way
    /// should fire but doesn't (stale marker), checks fire too aggressively
    /// (inflated epoch), or after debugging the way tree. Default is dry run —
    /// add --confirm to actually delete.
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
}

#[derive(Subcommand)]
enum AuthorCommand {
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
    /// Scaffold a new way file (ADR-139)
    ///
    /// Writes frontmatter and a body template. Ways are authored English-only;
    /// locale stubs come from ways-localize.
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
        /// Create in your own ways ($XDG_CONFIG_HOME/agent-ways/ways/) instead of project-local
        #[arg(long)]
        global: bool,
    },
    /// Show how a query matches ways under the live matcher (ADR-160)
    ///
    /// Diagnose how a query matches ways under the live late-interaction matcher
    /// (ADR-160): peak · share · body-confirm · fired, per candidate — the tool
    /// for authoring a way against how it actually fires. Ways compete as they do
    /// in a prompt scan in agent scope (toggles, scope, `when:`); `--all` competes
    /// every way.
    Match {
        /// The query string to match
        query: String,
        /// Project directory (for project-local ways; default: current)
        #[arg(long)]
        project: Option<String>,
        /// Compete every way, ignoring scope and `when:` (toggled-off ways stay out)
        #[arg(long)]
        all: bool,
        /// Print every candidate as one JSON object
        #[arg(long)]
        json: bool,
    },
    /// Score a probe file through the real prompt scan (ADR-701)
    ///
    /// Runs each probe as a fresh session through the scan the hooks run: alias
    /// rank, eligibility, admission, body confirmation and the firing gate, with
    /// the relevance judge off and no state carried between probes. `-tool`
    /// kinds go through the Bash lane with the prompt as the tool description.
    /// Reports where the expected way ranked and which stage decided it, then
    /// the pass rate (the way fires and no `must_not` way outranks it).
    ///
    /// Build the corpus from the same tree first:
    /// `ways corpus --ways-dir hooks/ways --output DIR`, then pass
    /// `--ways-dir hooks/ways --corpus DIR`.
    Probe {
        /// Probe file (default: tests/probes/tree-sample.tsv)
        file: Option<String>,
        /// Ways root to score against (default: the shipped ways; needs --corpus)
        #[arg(long, requires = "corpus")]
        ways_dir: Option<String>,
        /// Corpus built from that ways root: its directory or ways-corpus-en.jsonl
        #[arg(long, requires = "ways_dir")]
        corpus: Option<String>,
        /// Project directory for `when:` preconditions (default: current)
        #[arg(long)]
        project: Option<String>,
        /// Print one TSV row per probe instead of the table
        #[arg(long)]
        tsv: bool,
        /// Run with this `matching.body_rank` mode, whatever the config says (default: the
        /// configured value). The summary header names the mode whenever it is not `off`.
        #[arg(long, value_name = "off|on|scaled|scaled-single", value_parser = ["off", "on", "scaled", "scaled-single"])]
        body_rank: Option<String>,
        /// Treat every row as an unrelated prompt (its `expected_way` is ignored): print the
        /// way ranked first with its score and the ways that fired, then how many rows fired
        /// anything. For checking that a matcher change leaves unrelated prompts silent.
        #[arg(long, conflicts_with = "tsv")]
        unrelated: bool,
        /// Weight of the best body section in the blend (default 0.25; evaluation only)
        #[arg(long, value_name = "W")]
        body_rank_weight: Option<f64>,
        /// Confirm a way with one section against that section instead of its alias (evaluation only)
        #[arg(long)]
        single_section_confirm: bool,
    },
    /// Analyze a progressive-disclosure tree
    Tree {
        /// Way path or short name (e.g., "supplychain" or full path)
        path: String,
        /// Show Jaccard similarity between siblings
        #[arg(long)]
        jaccard: bool,
    },
    /// Score way-against-way cosine similarity
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
    /// Suggest vocabulary for a way file
    Suggest {
        /// Path to a way file
        file: String,
        /// Minimum term frequency for suggestions
        #[arg(long, default_value = "2")]
        min_freq: u32,
    },
    /// Export ways as a JSONL graph of nodes and edges
    Graph {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Output file (default: stdout)
        #[arg(long, short)]
        output: Option<String>,
    },
    /// Export the golden-prompt sidecars as TSV (ADR-701 §9)
    ///
    /// Reads `{wayname}.golden.jsonl` beside each way and `golden-none.jsonl` at
    /// the root. With `--tsv`, prints `prompt<TAB>expected_way<TAB>kind` rows
    /// sorted by way id; without it, a short summary.
    Golden {
        /// Ways root directory (default: the shipped ways root)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Print the rows as TSV
        #[arg(long)]
        tsv: bool,
        /// Print the tree-sampled probe set as TSV: prompt, expected_way, kind, role, must_not
        #[arg(long, conflicts_with = "tsv")]
        probes: bool,
        /// With --probes: the multi-sentence set, each way's situational then direct prompt joined (kind `joined`)
        #[arg(long, requires = "probes")]
        joined: bool,
    },
    /// Detect or repair hard-wrapped markdown prose
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
    /// Diff ways' requires: against settings.json grants (ADR-116)
    Permissions {
        /// Scan global ways (ignore project-local)
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum TuneCommand {
    /// Audit locale alias fidelity and discrimination (ADR-125)
    ///
    /// Flags locale stubs to re-author.
    Locale {
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
    /// Flag ways firing in off-domain sessions (ADR-134)
    ///
    /// Audits fire relevance (ADR-134 Decision 3).
    Precision {
        /// Minimum sessions a way must have fired in before it's flagged
        #[arg(long, default_value_t = cmd::tune_precision::MIN_SESSIONS)]
        min_sessions: usize,
        /// Off-class rate at or above which a way is flagged (0.0–1.0)
        #[arg(long, default_value_t = cmd::tune_precision::FLAG_THRESHOLD)]
        flag_threshold: f64,
        /// Filter to events in this project path or under it
        #[arg(long)]
        project: Option<String>,
        /// Filter to ways whose id contains this substring
        #[arg(long)]
        way: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Usage statistics from the event log
    Stats {
        /// Last N days only
        #[arg(long)]
        days: Option<u32>,
        /// Filter to this project path or under it (default: CLAUDE_PROJECT_DIR)
        #[arg(long)]
        project: Option<String>,
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
        /// Show stats across all projects (ignore CLAUDE_PROJECT_DIR)
        #[arg(long)]
        global: bool,
    },
    /// Language coverage: models, stubs and per-way routing
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
}

#[derive(Subcommand)]
enum LookupCommand {
    /// Candidates for a query on the prompt lane, with route, cosine, share and margin
    Search {
        query: String,
        /// The session, for its scope
        #[arg(long)]
        session: Option<String>,
        /// How many candidates to return
        #[arg(long, default_value = "5")]
        top: usize,
    },
    /// A way's body as injection renders it; stamping is the PostToolUse hook's (`ways hook pull`)
    Read {
        /// Way id (e.g. "softwaredev/code/quality")
        id: String,
        /// The session whose scope a mismatch is reported against
        #[arg(long)]
        session: Option<String>,
    },
    /// A way's parent, children, See Also edges and nearest semantic neighbours
    Neighbors {
        id: String,
        /// The session, to mark neighbours whose scope would not reach it
        #[arg(long)]
        session: Option<String>,
        /// How many semantic neighbours to return
        #[arg(long, default_value = "5")]
        top: usize,
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
    /// The theme tab on a terminal; list, copy, rename and delete themes
    ///
    /// Alone, the screens on the theme tab. Its actions copy any theme to a
    /// new user theme file, and rename or delete a user theme; a bundled
    /// theme is never renamed or deleted. theme.active follows a rename, and
    /// falls back to terminal when the active theme is deleted.
    #[command(args_conflicts_with_subcommands = true)]
    Theme {
        #[command(subcommand)]
        action: Option<ThemeCommand>,
        /// The project whose .claude/ways.yaml the screens read and write
        #[arg(long)]
        project: Option<PathBuf>,
        /// Test only: as `ways settings --keys`
        #[arg(long, hide = true, action = clap::ArgAction::Append, allow_hyphen_values = true)]
        keys: Vec<String>,
        /// Print the frame at WIDTHxHEIGHT, headless
        #[arg(long, hide = true)]
        snap: Option<String>,
        /// The colour depth to draw at: truecolor, 256, 16 or none
        #[arg(long, hide = true)]
        depth: Option<String>,
    },
}

#[derive(Subcommand)]
enum ThemeCommand {
    /// The themes on offer, their source and the active one
    List {
        /// Each theme's name, label, source, file and whether it is active
        #[arg(long)]
        json: bool,
    },
    /// Copy a theme, bundled or the user's, to a new user theme file
    Copy {
        from: String,
        to: String,
        #[arg(long)]
        json: bool,
    },
    /// Rename a user theme's file and name; theme.active follows
    Rename {
        from: String,
        to: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a user theme's file; theme.active falls back to terminal when it named it
    Delete {
        name: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum TargetCommand {
    /// List the targets and their converged state
    List {
        #[arg(long)]
        json: bool,
    },
    /// Preview what activating a config directory would change
    ///
    /// Every root it would link, merge, refuse or remove. Nothing is touched.
    Plan {
        /// Claude Code config directory (e.g. ~/.claude, or what CLAUDE_CONFIG_DIR names)
        dir: String,
        #[arg(long)]
        json: bool,
    },
    /// Record a config directory as a target and reconcile into it
    ///
    /// Stops on a blocked plan unless --force.
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
    /// Withdraw our links and hooks from a target, keeping its record
    Disable { dir: String },
    /// Withdraw from a target and drop its record
    Remove { dir: String },
}

fn main() -> Result<()> {
    // Rust ignores SIGPIPE, so `ways settings | head` panicked on the first
    // print after `head` closed the pipe. The default disposition ends the
    // process quietly, as every other Unix filter does.
    #[cfg(unix)]
    // SAFETY: setting a signal disposition before any thread starts.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
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

/// Whether stdin and stdout are both a terminal: a screen needs one to draw
/// on and one to read keys from.
fn on_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::io::stdin().is_terminal()
}

/// A group run with no verb: its screen on a terminal; in a pipe its help on
/// stderr and exit 2, as a group that needs a verb (ADR-507 §7, notes of
/// 2026-10-02). The long help, unlike clap's short one for a missing verb,
/// carries the line saying a bare run on a terminal opens the screen.
fn bare_group(group: &str, screen: impl FnOnce() -> Result<()>) -> Result<()> {
    use clap::CommandFactory;
    if on_terminal() {
        return screen();
    }
    let help = Cli::command().try_get_matches_from(["ways", group, "--help"]).expect_err("--help ends parsing");
    eprint!("{}", help.render());
    std::process::exit(2);
}

/// The settings screens on `tab`, as `ways settings <tab>` opens them.
fn settings_screen(tab: &str) -> Result<()> {
    let open = cmd::settings::tui::Open { tab: Some(tab.into()), ..Default::default() };
    cmd::settings::exit_with(cmd::settings::tui::open(&open))
}

fn run() -> Result<()> {
    // A bare `ways` prints help, with the banner only on a terminal
    // (ADR-507 §7). `--help` and `help` are clap's and carry no banner. The
    // top-level help ends with the judge's warning when it cannot gate; only
    // that help reads the judge's state, so other commands never pay for it.
    use clap::CommandFactory;
    let help = || {
        let help = Cli::command();
        match cmd::judge::help_footer() {
            Some(warning) => help.after_help(warning),
            None => help,
        }
    };
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.len() == 2 && matches!(args[1].to_str(), Some("--help" | "-h" | "help")) {
        help().get_matches_from(args);
        unreachable!("clap prints the help and exits");
    }
    let Some(command) = Cli::parse_from(args).command else {
        use std::io::IsTerminal;
        if std::io::stdout().is_terminal() {
            cmd::banner::run()?;
        }
        help().print_help()?;
        println!();
        return Ok(());
    };

    match command {
        Commands::Context { project, session, json } => cmd::context::run(project.as_deref(), session.as_deref(), json),
        Commands::Corpus { ways_dir, output, quiet, verbose, if_stale } => cmd::corpus::run(ways_dir, output, quiet, verbose, if_stale),
        Commands::Init { project } => cmd::init::run(project.as_deref()),
        Commands::Manifest { source, json } => cmd::manifest::run(json, source),
        Commands::Reconcile { source, dest, mode, dry_run, quiet, force } => {
            cmd::reconcile::run(source, dest, mode, dry_run, quiet, force)
        }
        Commands::Status { json } => cmd::status::run(json),
        Commands::Lookup { project, what } => {
            // Every reader below resolves the project as a hook would, from the
            // environment, before the config is first read.
            if let Some(p) = project {
                std::env::set_var("CLAUDE_PROJECT_DIR", p);
            }
            cmd::lookup::emit(match what {
                LookupCommand::Search { query, session, top } => {
                    cmd::lookup::ensure_enabled().and_then(|()| cmd::lookup::search_json(&query, session.as_deref(), top))
                }
                LookupCommand::Read { id, session } => {
                    cmd::lookup::ensure_enabled().and_then(|()| cmd::lookup::read_json(&id, session.as_deref()))
                }
                LookupCommand::Neighbors { id, session, top } => cmd::lookup::neighbors_json(&id, session.as_deref(), top),
            })
        }
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
                Some(SettingsCommand::Theme { action: None, project, keys, snap, depth }) => {
                    st::tui::open(&st::tui::Open { tab: Some("theme".into()), project, keys, snap, depth })
                }
                Some(SettingsCommand::Theme { action: Some(op), .. }) => match op {
                    ThemeCommand::List { json } => st::themes::list(json),
                    ThemeCommand::Copy { from, to, json } => st::themes::run(st::themes::Op::Copy { from: &from, to: &to }, json),
                    ThemeCommand::Rename { from, to, json } => st::themes::run(st::themes::Op::Rename { from: &from, to: &to }, json),
                    ThemeCommand::Delete { name, json } => st::themes::run(st::themes::Op::Delete { name: &name }, json),
                },
            })
        }
        Commands::Target { action: None } => bare_group("target", || settings_screen("install")),
        Commands::Target { action: Some(action) } => match action {
            TargetCommand::List { json } => cmd::target::list(json),
            TargetCommand::Plan { dir, json } => cmd::target::plan(&dir, json),
            TargetCommand::Add { dir, force, dry_run, json } => cmd::target::add(&dir, force, dry_run, json),
            TargetCommand::Enable { dir } => cmd::target::enable(&dir),
            TargetCommand::Disable { dir } => cmd::target::disable(&dir),
            TargetCommand::Remove { dir } => cmd::target::remove(&dir),
        },
        Commands::Session { action: None } => {
            bare_group("session", || cmd::introspect::replay(None, None, false, None, false, false, &cmd::introspect::Open::default()))
        }
        Commands::Session { action: Some(action) } => match action {
            SessionCommand::Ways { session, sort, json, matched } => cmd::list::run(session.as_deref(), &sort, json, matched),
            SessionCommand::Replay { session, project, all, speed, json, matched, keys, snap, depth } => {
                let open = cmd::introspect::Open { keys, snap, depth };
                cmd::introspect::replay(session.as_deref(), project.as_deref(), all, speed, json, matched, &open)
            }
            SessionCommand::List { project, all, json } => {
                cmd::introspect::list(project.as_deref(), all, json)
            }
            SessionCommand::Dump { session, project, all, matched } => {
                cmd::introspect::dump(session.as_deref(), project.as_deref(), all, matched)
            }
            SessionCommand::Live { session, project, keys, snap, depth } => {
                let open = cmd::introspect::Open { keys, snap, depth };
                cmd::introspect::live(session.as_deref(), project.as_deref(), &open)
            }
            SessionCommand::Fires { session, project, all, max_score, limit, matched, json } => {
                cmd::introspect::fires(session.as_deref(), project.as_deref(), all, max_score, limit, matched, json)
            }
            SessionCommand::Reset { session, all, confirm } => cmd::reset::run(session.as_deref(), all, confirm),
            SessionCommand::Subagents { state, session, json } => cmd::subagents::run(state.as_deref(), session.as_deref(), json),
        },
        Commands::Author { action } => match action {
            AuthorCommand::Lint { path, schema, check, fix, all, global } => cmd::lint::run(path, schema, check, fix, all, global),
            AuthorCommand::Template { path, description, vocabulary, scope, global } => {
                cmd::template::run(path, description, vocabulary, scope, global)
            }
            AuthorCommand::Match { query, project, all, json } => cmd::match_cmd::run_late(query, project.as_deref(), all, json),
            AuthorCommand::Probe { file, ways_dir, corpus, project, tsv, body_rank, unrelated, body_rank_weight, single_section_confirm } => {
                cmd::probe::run(
                    file,
                    ways_dir,
                    corpus,
                    project.as_deref(),
                    tsv,
                    cmd::probe::Eval { body_rank: body_rank.as_deref().and_then(config::BodyRank::parse), unrelated, body_rank_weight, single_section_confirm },
                )
            }
            AuthorCommand::Tree { path, jaccard } => cmd::tree::run(path, jaccard),
            AuthorCommand::Siblings { id, threshold, corpus, model } => cmd::siblings::run(id, threshold, corpus, model),
            AuthorCommand::Suggest { file, min_freq } => cmd::suggest::run(file, min_freq),
            AuthorCommand::Graph { ways_dir, output } => cmd::graph::run(ways_dir, output),
            AuthorCommand::Golden { ways_dir, tsv, probes, joined } => cmd::golden::run(ways_dir, tsv, probes, joined),
            AuthorCommand::Reflow { path, fix, json, quiet } => cmd::reflow::run(path, fix, json, quiet),
            AuthorCommand::Permissions { global } => cmd::permissions::audit(global),
        },
        Commands::Tune { action } => match action {
            TuneCommand::Locale { ways_dir, way, lang, fidelity_threshold, discrimination_threshold, json } => {
                cmd::tune::run(ways_dir, way, lang, fidelity_threshold, discrimination_threshold, json)
            }
            TuneCommand::Precision { min_sessions, flag_threshold, project, way, json } => {
                cmd::tune_precision::run(min_sessions, flag_threshold, project, way, json)
            }
            TuneCommand::Stats { days, project, json, global } => cmd::stats::run(days, project.as_deref(), json, global),
            TuneCommand::Language { filter, audit, json } => cmd::language::run(filter.as_deref(), audit, json),
        },
        Commands::SessionsRoot => {
            println!("{}", session::sessions_root());
            Ok(())
        }
        Commands::EventsLogPath => {
            println!("{}", paths::events_log().display());
            Ok(())
        }
        // Bare on a terminal, the projects screen; in a pipe, `list`.
        Commands::Projects { command: None } if on_terminal() => cmd::projects::screen::open(&cmd::introspect::Open::default()),
        Commands::Projects { command } => cmd::projects::run(command),
        Commands::ProjectSlug { path } => {
            let project = path.unwrap_or_else(util::project_dir);
            println!("{}", claude_sessions::project_slug(&project));
            Ok(())
        }
        Commands::Hook { event } => cmd::hook::run(event),
        // `ways-agent` prints its own help in a pipe, so only the terminal
        // case is ways' to handle.
        Commands::Agent { args } if args.is_empty() && on_terminal() => settings_screen("gate"),
        Commands::Agent { args } => cmd::agent::run(&args),
        Commands::Update { dry_run, git_ref } => cmd::update::run(dry_run, git_ref),
        Commands::JudgeSetup => cmd::judge::setup(),
        Commands::Uninstall { yes, purge } => cmd::uninstall::run(yes, purge),
    }
}
