//! `ways manage reinstall`: put the installed version back cleanly, keeping
//! what the operator owns (ADR-509 §3).
//!
//! The order is what makes it safe. The same commit, from the source the
//! install came from, is fetched and verified into a stage beside the app
//! first. Only then does ADR-198's uninstall run, without `--purge`, followed
//! by the staged installer and a repair of every target the kept config
//! lists. A failed fetch or verification removes nothing. A failed install
//! keeps the stage and prints the line that finishes from it.
//!
//! The fetch and its check are the installer's own (`install.sh --stage`, the
//! function its bootstrap fetches through). The checkout test and the HEAD
//! read are the ones `ways manage update` uses.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::uninstall::Uninstall;
use super::update::{git_head, is_app_checkout, run_step, source_describe};
use crate::paths;

/// Where the installed version came from: the origin remote, the commit, and
/// the branch it is on (None on a detached HEAD, as `manage update --ref`
/// leaves it).
struct Source {
    url: String,
    rev: String,
    branch: Option<String>,
    describe: Option<String>,
}

impl Source {
    fn read(app: &Path) -> Result<Source> {
        let url = git(app, &["remote", "get-url", "origin"])
            .with_context(|| format!("{} has no origin remote to fetch the same version from", app.display()))?;
        let rev = git_head(app).with_context(|| format!("cannot read HEAD in {}", app.display()))?;
        let branch = git(app, &["symbolic-ref", "--short", "-q", "HEAD"]);
        Ok(Source { url, rev, branch, describe: source_describe(app) })
    }

    fn label(&self) -> String {
        let at = self.describe.clone().unwrap_or_else(|| self.rev.chars().take(12).collect());
        match &self.branch {
            Some(b) => format!("{at} on {b}"),
            None => format!("{at} (detached)"),
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The stage sits beside the app dir. Uninstall deletes only directories
/// named `agent-ways`, so it never reaches the stage.
fn stage_dir(app: &Path) -> PathBuf {
    app.with_file_name("agent-ways.reinstall")
}

/// A word for a POSIX shell, single-quoted.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn run(yes: bool, purge: bool) -> Result<()> {
    if purge {
        eprintln!(
            "ways manage reinstall keeps your config and state, so it does not take --purge.\n\
             To delete them as well, run `ways manage uninstall --purge --yes`, then the installer."
        );
        std::process::exit(2);
    }
    let app = paths::data_root();
    if !is_app_checkout(&app) {
        bail!("no agent-ways app source at {} to reinstall; run the installer instead", app.display());
    }
    let source = Source::read(&app)?;
    let stage = stage_dir(&app);
    let uninstall = Uninstall::plan(false)?;

    println!("Reinstall {} from {}:\n", source.label(), source.url);
    println!("1. Fetch and verify that commit into {}. If this fails, nothing is removed.", stage.display());
    println!("2. Uninstall, keeping your config and state:");
    // The stage holds the committed HEAD only; edits in the app dir go with it.
    if git(&app, &["status", "--porcelain"]).is_some() {
        println!("   {} has local changes, which the uninstall deletes.", app.display());
    }
    if let Some(u) = &uninstall {
        u.print();
    }
    println!("3. Install from the stage (scripts/install.sh), then project every target your config lists.");
    if !yes {
        println!("\nNothing changed. Run `ways manage reinstall --yes` to do this.");
        return Ok(());
    }

    // 1. A stage left by an earlier run is stale: that run either finished or
    //    never removed anything, since the app is still here.
    if stage.exists() {
        std::fs::remove_dir_all(&stage).with_context(|| format!("removing the old stage {}", stage.display()))?;
    }
    eprintln!("==> fetch and verify {} into {}", source.rev, stage.display());
    let mut fetch = Command::new("bash");
    fetch.arg(app.join("scripts/install.sh")).arg("--stage").arg(&stage).args(["--from", &source.url, "--rev", &source.rev]);
    if let Some(b) = &source.branch {
        fetch.args(["--branch", b]);
    }
    let fetched = run_step(&mut fetch, "fetch and verify").and_then(|()| {
        if !is_app_checkout(&stage) || git_head(&stage).as_deref() != Some(source.rev.as_str()) {
            bail!("the staged copy at {} is not agent-ways at {}", stage.display(), source.rev);
        }
        Ok(())
    });
    if let Err(e) = fetched {
        let _ = std::fs::remove_dir_all(&stage);
        bail!("{e:#}; nothing was removed");
    }

    // 2. Uninstall without --purge.
    eprintln!("==> uninstall, keeping config and state");
    if let Some(u) = &uninstall {
        if let Err(e) = u.apply() {
            eprintln!("The staged copy is kept at {}.", stage.display());
            return Err(e);
        }
    }

    // 3. The staged installer. It clones the stage into the app dir, so the
    //    app's branch is the stage's; AGENT_WAYS_ORIGIN points it back at the
    //    source the install came from.
    eprintln!("==> install from {}", stage.display());
    let installed = Command::new("bash")
        .arg("scripts/install.sh")
        .current_dir(&stage)
        .env("AGENT_WAYS_ORIGIN", &source.url)
        .status()
        .is_ok_and(|s| s.success());
    if !installed {
        // A half-made app dir would turn the finishing run into an update,
        // which pulls past the version being put back. It is a fresh clone of
        // the stage and holds nothing of the operator's.
        if app.exists() {
            let _ = std::fs::remove_dir_all(&app);
        }
        let s = stage.to_string_lossy();
        eprintln!(
            "\nThe install did not finish. The staged copy is kept at {}.\n\
             Fix the cause above, then finish the install with:\n  \
             cd {} && AGENT_WAYS_ORIGIN={} bash scripts/install.sh && rm -rf {}",
            stage.display(),
            quote(&s),
            quote(&source.url),
            quote(&s)
        );
        bail!("the install from the stage failed");
    }
    let _ = std::fs::remove_dir_all(&stage);

    // 4. The installer projects `~/.claude` only, and the uninstall withdrew
    //    every target. A bare repair converges each target the kept config
    //    lists.
    eprintln!("==> project every target");
    if let Err(e) = super::reconcile::run(Some(app.to_string_lossy().into_owned()), None, None, false, false, false) {
        eprintln!("\nagent-ways {} is installed, but a target was not projected. Fix the cause above, then run:\n  ways manage repair", source.label());
        return Err(e);
    }
    println!("\nagent-ways {} is reinstalled. Restart Claude Code to pick it up.", source.label());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_is_beside_the_app_and_not_a_name_uninstall_deletes() {
        let stage = stage_dir(Path::new("/home/u/.local/share/agent-ways"));
        assert_eq!(stage, Path::new("/home/u/.local/share/agent-ways.reinstall"));
        assert_ne!(stage.file_name().unwrap(), "agent-ways");
    }

    #[test]
    fn quote_survives_a_single_quote() {
        assert_eq!(quote("/tmp/it's"), r"'/tmp/it'\''s'");
    }
}
