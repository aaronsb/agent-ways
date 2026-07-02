//! `ways-audit` — the compliance operator surface for the ways toolchain.
//!
//! A deliberately-invoked sibling to the `ways` runtime (ADR-151). It reads the
//! compliance **claims** carried by ways (the `provenance.yaml` sidecars) and
//! reports on them: coverage, traces, control/policy queries, gaps, staleness,
//! a traceability matrix, and claim-integrity lint.
//!
//! A claim is a control-*design* assertion, not evidence (ADR-200). Session-
//! derived **findings** — the assessment surface — are the next layer, built on
//! the same `ways-core` engine this binary already depends on.

mod audit;
mod helpers;
mod lint;
mod matrix;
mod query;
mod report;
mod trace;

use anyhow::Result;
use clap::{Parser, Subcommand};

use ways_core::provenance;

#[derive(Parser)]
#[command(
    name = "ways-audit",
    about = "Compliance claim reporting for ways (ADR-200)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Machine-readable JSON output
    #[arg(long, global = true)]
    json: bool,

    /// Scan global ways (ignore project-local)
    #[arg(long, global = true)]
    global: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Claim coverage report (default)
    Report,
    /// End-to-end claim trace for a single way
    Trace {
        /// Way ID (e.g., "softwaredev/code/quality")
        way: String,
    },
    /// Which ways claim a control
    Control {
        /// Search pattern for control IDs
        pattern: String,
    },
    /// Which ways derive from a policy
    Policy {
        /// Search pattern for policy URIs
        pattern: String,
    },
    /// Ways without a claim
    Gaps,
    /// Claims with stale verified dates
    Stale {
        /// Days before considered stale (default: 90)
        #[arg(default_value = "90")]
        days: u32,
    },
    /// Cross-reference claims with firing stats
    Active,
    /// Flat spreadsheet: way | control | justification
    Matrix,
    /// Validate claim integrity
    Lint,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine ways directory: project-local first, then global.
    let ways_dir = if !cli.global {
        helpers::detect_project_ways()
    } else {
        None
    };

    let manifest = provenance::generate_manifest(ways_dir)?;
    let json = cli.json;

    match cli.command {
        Command::Report => report::run(&manifest, json),
        Command::Trace { way } => trace::run(&manifest, &way, json),
        Command::Control { pattern } => query::control(&manifest, &pattern, json),
        Command::Policy { pattern } => query::policy(&manifest, &pattern, json),
        Command::Gaps => audit::gaps(&manifest, json),
        Command::Stale { days } => audit::stale(&manifest, days, json),
        Command::Active => audit::active(&manifest, json),
        Command::Matrix => matrix::run(&manifest, json),
        Command::Lint => lint::run(&manifest, json),
    }
}
