//! `ways agent …`: the ways agent's command line (ADR-502), run as its own
//! binary so the `ways` hook process links no TLS or provider code.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Runs `ways-agent` with `args` and exits with its status.
pub fn run(args: &[String]) -> Result<()> {
    let bin = resolve().context("ways-agent is not installed; run `ways update` to install it")?;
    let status = std::process::Command::new(&bin)
        .args(args)
        .status()
        .with_context(|| format!("running {}", bin.display()))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// The `ways-agent` binary, found the way hooks find it to start the agent.
pub(crate) fn resolve() -> Option<PathBuf> {
    ways_agent::client::agent_binary()
}
