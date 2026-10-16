//! `ways manage reinstall`: put the installed version back cleanly, keeping
//! what the operator owns (ADR-509 §3).
//!
//! The order is what makes it safe. The same commit, from the source the
//! install came from, is fetched and verified into a stage beside the app
//! first. Only then does ADR-198's uninstall run, without `--purge`, followed
//! by the staged installer and a repair of every target the kept config
//! lists. A failed fetch or verification removes nothing. A failed install
//! keeps the stage and prints the line that finishes from it. An install that
//! built the app and whose repair failed, most often at a real path in a
//! target, keeps the app, and `ways manage repair` finishes it.
//!
//! The fetch and its check are the installer's own (`install.sh --stage`, the
//! function its bootstrap fetches through). The checkout test and the HEAD
//! read are the ones `ways manage update` uses.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::uninstall::Uninstall;
use super::update::{build_from_source, git_head, is_app_checkout, run_step, source_describe, tool_present};
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

/// The command that installs from a kept stage and then removes it.
fn finish_line(stage: &Path, url: &str) -> String {
    let s = quote(&stage.to_string_lossy());
    format!("cd {s} && AGENT_WAYS_ORIGIN={} bash scripts/install.sh && rm -rf {s}", quote(url))
}

/// The installer's exit code when the app is built and on PATH and the repair
/// after the build failed, most often at a real path (`scripts/install.sh`).
const REPAIR_FAILED: i32 = 3;

/// A word for a POSIX shell, single-quoted.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn run(mode: super::lifecycle::Mode, purge: bool) -> Result<()> {
    if purge {
        eprintln!(
            "ways manage reinstall keeps your config and state, so it does not take --purge.\n\
             To delete them as well, run `ways manage uninstall --purge --commit`, then the installer."
        );
        std::process::exit(2);
    }
    let app = paths::data_root();
    let stage = stage_dir(&app);
    if !is_app_checkout(&app) {
        // An uninstall that stopped partway can take the app's .git and leave
        // the stage as the only good copy.
        if let Some(url) = is_app_checkout(&stage).then(|| git(&stage, &["remote", "get-url", "origin"])).flatten() {
            eprintln!(
                "A stage from an earlier reinstall is at {}. If that reinstall stopped partway, delete what is left of {}, then finish from the stage:\n  {}",
                stage.display(),
                app.display(),
                finish_line(&stage, &url)
            );
        }
        bail!("no agent-ways app source at {} to reinstall; run the installer instead", app.display());
    }
    let source = Source::read(&app)?;
    let uninstall = Uninstall::plan(false)?;

    println!("Reinstall {} from {}:\n", source.label(), source.url);
    println!("1. Fetch and verify that commit into {}. If this fails, nothing is removed.", stage.display());
    println!("2. Uninstall, keeping your config and state:");
    // The stage holds the committed HEAD only; edits in the app dir go with it.
    let dirty = git(&app, &["status", "--porcelain"]).is_some();
    if dirty {
        println!("   {} has local changes, which the uninstall deletes.", app.display());
    }
    if let Some(u) = &uninstall {
        u.print();
    }
    println!("3. Install from the stage (scripts/install.sh), then project every target your config lists.");
    // A detached HEAD is a `manage update --ref` source build. The installer's
    // setup downloads release binaries, so the suite is built from source again.
    let source_build = source.branch.is_none();
    if source_build {
        println!("   The install is a source build (detached HEAD), so the suite is built from source again, as `ways manage update --ref` did.");
    }
    if mode == super::lifecycle::Mode::WhatIf {
        println!("\n{}", super::lifecycle::commit_hint("ways manage reinstall"));
        return Ok(());
    }

    if source_build && !tool_present("cargo") {
        bail!(
            "this install was built from source at {}, which needs a Rust toolchain (cargo not found); nothing was removed. \
             Install it (https://rustup.rs/), then retry.",
            source.label()
        );
    }

    // 1. A stage left by an earlier run beside a clean app is stale: that run
    //    either finished or never removed anything. Beside a changed app it may
    //    be the only good copy, after an uninstall that stopped partway.
    if stage.exists() {
        if dirty {
            eprintln!(
                "A stage from an earlier reinstall is at {}, and {} has changes.\n\
                 If that reinstall stopped partway, delete what is left of the app, then finish from the stage:\n  {}\n\
                 Otherwise remove the stage and run `ways manage reinstall --commit` again.",
                stage.display(),
                app.display(),
                finish_line(&stage, &source.url)
            );
            bail!("a stage from an earlier reinstall is in the way");
        }
        std::fs::remove_dir_all(&stage).with_context(|| format!("removing the old stage {}", stage.display()))?;
    }
    // An installer from before stage mode ignores the flags and updates the
    // app in place, so it is not run at all.
    let installer = app.join("scripts/install.sh");
    if !std::fs::read_to_string(&installer).is_ok_and(|t| t.contains("--stage)")) {
        bail!(
            "{} has no --stage mode, which reinstall fetches through; nothing was removed. \
             Run `ways manage update --commit` to move to a version that has it.",
            installer.display()
        );
    }
    eprintln!("==> fetch and verify {} into {}", source.rev, stage.display());
    // From the app dir, so a relative origin URL resolves as git resolves it there.
    let mut fetch = Command::new("bash");
    fetch.arg(&installer).arg("--stage").arg(&stage).args(["--from", &source.url, "--rev", &source.rev]).current_dir(&app);
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
            eprintln!(
                "\nThe uninstall did not finish. The staged copy is kept at {}.\n\
                 Fix the cause above and delete what is left of {}, then finish the install with:\n  {}",
                stage.display(),
                app.display(),
                finish_line(&stage, &source.url)
            );
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
        .map(|s| s.code());
    let stopped = match installed {
        Ok(Some(0)) => false,
        // The app is built and on PATH; the repair after the build failed,
        // and the installer has named the cause.
        Ok(Some(REPAIR_FAILED)) => true,
        _ => {
            // A half-made app dir would turn the finishing run into an update,
            // which pulls past the version being put back. It is a fresh clone
            // of the stage and holds nothing of the operator's.
            if app.exists() {
                let _ = std::fs::remove_dir_all(&app);
            }
            eprintln!(
                "\nThe install did not finish. The staged copy is kept at {}.\n\
                 Fix the cause above, then finish the install with:\n  {}",
                stage.display(),
                finish_line(&stage, &source.url)
            );
            bail!("the install from the stage failed");
        }
    };
    // The app is built, so the stage is not needed any more.
    let _ = std::fs::remove_dir_all(&stage);

    // The binaries of a source build, from the restored source.
    if source_build {
        if let Err(e) = build_from_source(&app) {
            eprintln!(
                "\nagent-ways {} is installed with release binaries, and the source build failed. Fix the cause above, then run:\n  ways manage update --ref {} --commit",
                source.label(),
                source.rev
            );
            return Err(e);
        }
    }
    if stopped {
        eprintln!(
            "\nagent-ways {} is installed, and no target is projected yet. Fix the cause above, then finish with:\n  ways manage repair --commit",
            source.label()
        );
        bail!("the repair after the install failed");
    }

    // 4. The installer projects `~/.claude` only, and the uninstall withdrew
    //    every target. A bare repair converges each target the kept config
    //    lists.
    eprintln!("==> project every target");
    if let Err(e) = super::reconcile::run(Some(app.to_string_lossy().into_owned()), None, None, false, false, false) {
        eprintln!("\nagent-ways {} is installed, but a target was not projected. Fix the cause above, then run:\n  ways manage repair --commit", source.label());
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
