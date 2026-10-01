//! `ways agent …`: the ways agent's command line (ADR-502), run as its own
//! binary so the `ways` hook process links no TLS or provider code.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::util::home_dir;

const BIN: &str = "ways-agent";

/// Runs `ways-agent` with `args` and exits with its status.
pub fn run(args: &[String]) -> Result<()> {
    let bin = resolve().context("ways-agent is not installed; run `ways update` to install it")?;
    let status = std::process::Command::new(&bin)
        .args(args)
        .status()
        .with_context(|| format!("running {}", bin.display()))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// The `ways-agent` binary: beside this `ways`, else in the projected
/// `~/.claude/bin`, else on `PATH`.
pub(crate) fn resolve() -> Option<PathBuf> {
    let name = format!("{BIN}{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join(&name)));
    let projected = Some(home_dir().join(".claude").join("bin").join(&name));
    let on_path = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&name)).find(|p| p.is_file()));
    [beside, projected, on_path].into_iter().flatten().find(|p| p.is_file())
}
