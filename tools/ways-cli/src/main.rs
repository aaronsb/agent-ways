use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod agents;
mod cmd;
pub mod config;
mod frontmatter;
mod scanner;
pub mod session;
pub mod util;

#[derive(Parser)]
#[command(name = "ways", version, about = "Unified CLI for ways knowledge guidance")]
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
        /// Auto-fix what can be fixed (multi-line YAML, missing check sections)
        #[arg(long)]
        fix: bool,
        /// Scan global ways (ignore CLAUDE_PROJECT_DIR)
        #[arg(long)]
        global: bool,
    },
    /// Generate the ways corpus for matching engines
    Corpus {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Suppress progress output
        #[arg(long, short)]
        quiet: bool,
        /// Only regenerate if corpus is stale (newer way files exist)
        #[arg(long)]
        if_stale: bool,
    },
    /// Score a query against ways (embedding cosine similarity, ADR-125)
    Match {
        /// The query string to match
        query: String,
        /// Path to corpus JSONL
        #[arg(long)]
        corpus: Option<String>,
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
    /// Scan provenance sidecars
    Provenance {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
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
    /// Replay a session's way-firing history as an interactive animation
    Rethink {
        /// Session ID to replay directly (skip picker)
        #[arg(long)]
        session: Option<String>,
        /// Filter to sessions from this project path
        #[arg(long)]
        project: Option<String>,
        /// Initial frame speed in milliseconds (default: 1000)
        #[arg(long)]
        speed: Option<u64>,
        /// List sessions (non-interactive)
        #[arg(long)]
        list: bool,
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
    /// Manage configuration (init/show/path)
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Audit locale alias fidelity + discrimination (ADR-125 — flags stubs to re-author)
    Tune {
        /// Ways root directory (default: ~/.claude/hooks/ways)
        #[arg(long)]
        ways_dir: Option<String>,
        /// Filter to ways matching this substring (e.g., "security", "ea/")
        #[arg(long)]
        way: Option<String>,
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
    /// Tune firing-dynamics curves from observed cadence (ADR-123 Phase E)
    TuneCurves {
        /// Rewrite each suggested curve block in place (default: dry run)
        #[arg(long)]
        apply: bool,
        /// Minimum delta samples a way must have before it's suggested
        #[arg(long, default_value = "3")]
        min_fires: usize,
        /// Filter to events whose project path contains this substring
        #[arg(long)]
        project: Option<String>,
        /// Filter to ways whose id contains this substring
        #[arg(long)]
        way: Option<String>,
    },
    /// Permission audit — diff requires: fields against settings.json grants (ADR-116)
    Permissions {
        #[command(subcommand)]
        action: PermissionsCommand,
        /// Scan global ways (ignore project-local)
        #[arg(long, global = true)]
        global: bool,
    },
    /// Governance provenance queries — report, trace, control, policy, gaps, stale, active, matrix, lint
    Governance {
        #[command(subcommand)]
        mode: GovernanceCommand,
        /// Machine-readable JSON output
        #[arg(long, global = true)]
        json: bool,
        /// Scan global ways (ignore project-local)
        #[arg(long, global = true)]
        global: bool,
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
        /// `hookSpecificOutput` is canonical for all events; `SessionStart`
        /// and `PreToolUse` are the only events that take the legacy
        /// top-level `additionalContext` shape. Defaults to SessionStart.
        #[arg(long, default_value = "SessionStart")]
        hook_event: String,
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

#[derive(Subcommand)]
enum GovernanceCommand {
    /// Coverage report (default)
    Report,
    /// End-to-end provenance trace for a single way
    Trace {
        /// Way ID (e.g., "softwaredev/code/quality")
        way: String,
    },
    /// Which ways implement a control
    Control {
        /// Search pattern for control IDs
        pattern: String,
    },
    /// Which ways derive from a policy
    Policy {
        /// Search pattern for policy URIs
        pattern: String,
    },
    /// Ways without provenance
    Gaps,
    /// Ways with stale verified dates
    Stale {
        /// Days before considered stale (default: 90)
        #[arg(default_value = "90")]
        days: u32,
    },
    /// Cross-reference provenance with firing stats
    Active,
    /// Flat spreadsheet: way | control | justification
    Matrix,
    /// Validate provenance integrity
    Lint,
}

fn main() -> Result<()> {
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
        Commands::Lint { path, schema, check, fix, global } => cmd::lint::run(path, schema, check, fix, global),
        Commands::Corpus { ways_dir, quiet, if_stale } => cmd::corpus::run(ways_dir, quiet, if_stale),
        Commands::Match { query, corpus } => cmd::match_cmd::run(query, corpus),
        Commands::Embed { query, corpus, model } => cmd::embed::run(query, corpus, model),
        Commands::Siblings { id, threshold, corpus, model } => {
            cmd::siblings::run(id, threshold, corpus, model)
        }
        Commands::Graph { ways_dir, output } => cmd::graph::run(ways_dir, output),
        Commands::Tree { path, jaccard } => cmd::tree::run(path, jaccard),
        Commands::Provenance { ways_dir } => cmd::provenance::run(ways_dir),
        Commands::Init { project } => cmd::init::run(project.as_deref()),
        Commands::Template { path, description, vocabulary, scope, global } => {
            cmd::template::run(path, description, vocabulary, scope, global)
        }
        Commands::Language { filter, audit, json } => cmd::language::run(filter.as_deref(), audit, json),
        Commands::Stats { days, project, json, global } => {
            cmd::stats::run(days, project.as_deref(), json, global)
        }
        Commands::List { session, sort, json } => cmd::list::run(session.as_deref(), &sort, json),
        Commands::Rethink { session, project, speed, list } => {
            cmd::rethink::run(session.as_deref(), project.as_deref(), speed, list)
        }
        Commands::Status { json } => cmd::status::run(json),
        Commands::Scan { mode } => match mode {
            ScanCommand::Prompt { query, session, project } => {
                cmd::scan::prompt(&query, &session, project.as_deref())
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
                cmd::scan::state(&session, project.as_deref(), transcript.as_deref(), &hook_event)
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
        Commands::Suggest { file, min_freq } => cmd::suggest::run(file, min_freq),
        Commands::Tune { ways_dir, way, fidelity_threshold, discrimination_threshold, json } => {
            cmd::tune::run(ways_dir, way, fidelity_threshold, discrimination_threshold, json)
        }
        Commands::TuneCurves { apply, min_fires, project, way } => {
            cmd::tune_curves::run(apply, min_fires, project, way)
        }
        Commands::Reset { session, all, confirm } => {
            cmd::reset::run(session.as_deref(), all, confirm)
        }
        Commands::Permissions { action, global } => {
            match action {
                PermissionsCommand::Audit => cmd::permissions::audit(global),
            }
        }
        Commands::Governance { mode, json, global } => {
            let gov_mode = match mode {
                GovernanceCommand::Report => cmd::governance::Mode::Report,
                GovernanceCommand::Trace { way } => cmd::governance::Mode::Trace(way),
                GovernanceCommand::Control { pattern } => cmd::governance::Mode::Control(pattern),
                GovernanceCommand::Policy { pattern } => cmd::governance::Mode::Policy(pattern),
                GovernanceCommand::Gaps => cmd::governance::Mode::Gaps,
                GovernanceCommand::Stale { days } => cmd::governance::Mode::Stale(days),
                GovernanceCommand::Active => cmd::governance::Mode::Active,
                GovernanceCommand::Matrix => cmd::governance::Mode::Matrix,
                GovernanceCommand::Lint => cmd::governance::Mode::Lint,
            };
            cmd::governance::run(gov_mode, json, global)
        }
    }
}
