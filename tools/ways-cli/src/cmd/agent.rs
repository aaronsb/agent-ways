//! `ways agent …`: the ways agent's command line (ADR-502), run as its own
//! binary so the `ways` hook process links no TLS or provider code.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Runs `ways-agent` with `args` and exits with its status.
pub fn run(args: &[String]) -> Result<()> {
    let bin = resolve().context("ways-agent is not installed; run `ways manage update --commit` to install it")?;
    // On Unix, become ways-agent: no process in between holds its pipes or
    // outlives a signal sent to it.
    #[cfg(unix)]
    {
        let mut cmd = std::process::Command::new(&bin);
        cmd.args(args);
        // exec returns only when it fails.
        let e = super::proc::retry_if_busy(|| Err::<(), _>(std::os::unix::process::CommandExt::exec(&mut cmd))).unwrap_err();
        Err(e).with_context(|| format!("running {}", bin.display()))
    }
    #[cfg(not(unix))]
    {
        let status = super::proc::retry_if_busy(|| std::process::Command::new(&bin).args(args).status())
            .with_context(|| format!("running {}", bin.display()))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// The `ways-agent` binary, found the way hooks find it to start the agent.
pub(crate) fn resolve() -> Option<PathBuf> {
    ways_agent_core::client::agent_binary()
}
