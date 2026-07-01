//! `ways update` — update the agent-ways install from the binary itself.
//!
//! The 1.0 lifecycle separates the *app source* (`$XDG_DATA/agent-ways`, a git
//! checkout) from its *projection* (`~/.claude`, symlinks + a merged
//! `settings.json`) — ADR-142. Updating means refreshing the source, its
//! binaries, and reprojecting. Previously that meant "find the app dir, `cd`,
//! `make update`, `ways reconcile`". This wraps the mainline flow behind one
//! command, with the same guard/gate the other lifecycle verbs use.
//!
//! Two properties the naive `make update` lacks:
//!
//! - **Prefer pre-built binaries.** `make update` force-*builds* via cargo/cmake,
//!   which fails for anyone without a toolchain. This mirrors the *install* flow
//!   instead: download-first, build-fallback (the Makefile's `ways` / way-embed
//!   `setup-binary` targets). `attend`/`attend-chat` have no pre-built binaries
//!   yet, so they are refreshed **only if a toolchain is present**, else left
//!   running their current version.
//! - **Rename-then-revert, never leave a broken install.** Each component's
//!   binary is *renamed* aside (not removed) to defeat the "already installed"
//!   early-return; if re-acquiring it fails, the old binary is moved back. A
//!   failed update restores the prior state rather than stranding a binary-less
//!   setup. (Self-replacement is safe: the running process keeps its own inode;
//!   the new binary is in place for the next invocation.)

use crate::paths;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn run(dry_run: bool) -> Result<()> {
    let app = paths::data_root();

    // Guard 1: a pre-1.0 in-place clone (~/.claude is itself the repo) must
    // migrate, not update in place.
    let projection = paths::projection_root();
    if super::reconcile::is_legacy_in_place(&projection) {
        bail!(
            "{} looks like a pre-1.0 in-place clone. Migrate to the 1.0 projection first:\n\
             \x20 ways migrate --what-if   then   ways migrate --execute",
            projection.display()
        );
    }
    // Guard 2: the app source must be an agent-ways git checkout.
    if !is_app_checkout(&app) {
        bail!(
            "no agent-ways app source at {} (expected a git checkout with the agent-ways \
             Makefile). Re-run the installer to (re)stage it.",
            app.display()
        );
    }

    let has_toolchain = tool_present("cargo");
    let ways_bin = app.join("bin").join(exe("ways"));

    if dry_run {
        println!("ways update would, in {}:", app.display());
        println!("  1. scripts/update.sh          — git pull (autostash-safe)");
        println!("  2. refresh ways               — download pre-built, else build (rename→revert on fail)");
        println!("  3. refresh way-embed          — download pre-built, else build (optional)");
        if has_toolchain {
            println!("  4. refresh attend/attend-chat — build (toolchain present)");
        } else {
            println!("  4. skip attend/attend-chat    — no pre-built + no toolchain; keep current version");
        }
        println!("  5. {} corpus + reconcile      — regenerate corpus, reproject ~/.claude", ways_bin.display());
        println!("(dry-run — nothing executed)");
        return Ok(());
    }

    // 1. Pull.
    eprintln!("==> pull ({})", app.display());
    run_step(Command::new("bash").arg("scripts/update.sh").current_dir(&app), "git pull")?;

    // 2. Core — ways. Download-first; fatal (a failed refresh reverts and aborts).
    eprintln!("==> refresh ways (pre-built first)");
    refresh_component(&app, "ways", &["ways"], &app)
        .context("could not refresh the ways binary")?;

    // 3. Matcher — way-embed. Download-first; optional (semantic matching degrades
    //    to regex without it, so a failure warns rather than aborts).
    eprintln!("==> refresh way-embed (pre-built first)");
    if let Err(e) = refresh_component(&app, "way-embed", &["-C", "tools/way-embed", "setup-binary"], &app) {
        eprintln!("  ⚠ way-embed not refreshed ({e}); semantic matching keeps its current state.");
    }

    // 4. Awareness — attend/attend-chat. Build-only today, so gate on a toolchain.
    if has_toolchain {
        eprintln!("==> refresh attend/attend-chat (build)");
        for comp in ["attend", "attend-chat"] {
            if let Err(e) = refresh_component(&app, comp, &[comp], &app) {
                eprintln!("  ⚠ {comp} not refreshed ({e}); it keeps its current version.");
            }
        }
    } else {
        eprintln!(
            "==> attend/attend-chat: skipped — no pre-built binary and no build toolchain. \
             They keep running the current version; run `make deps` then `ways update` to refresh them."
        );
    }

    // 5. Regenerate the corpus (best-effort — it self-heals on next session too) and
    //    reproject with the freshly-built binary.
    if !ways_bin.exists() {
        bail!("expected a refreshed binary at {} but it is missing", ways_bin.display());
    }
    eprintln!("==> regenerate corpus");
    if let Err(e) = run_step(Command::new(&ways_bin).args(["corpus", "--quiet"]).current_dir(&app), "ways corpus") {
        eprintln!("  ⚠ corpus not regenerated now ({e}); it will self-heal on the next session.");
    }
    eprintln!("==> reconcile projection");
    run_step(Command::new(&ways_bin).arg("reconcile").current_dir(&app), "ways reconcile")?;

    println!("\nUpdate complete. A running Claude Code session keeps the old hooks, ways, and");
    println!("skills in memory — restart Claude Code to pick up the new version.");
    Ok(())
}

/// Refresh one component binary safely: rename the existing binary aside (so the
/// download/build target re-acquires it instead of early-returning "already
/// installed"), run the target, and on failure move the old binary back — never
/// leaving the slot empty.
fn refresh_component(app: &Path, name: &str, make_args: &[&str], make_dir: &Path) -> Result<()> {
    let bin = app.join("bin").join(exe(name));
    let backup = app.join("bin").join(format!("{}.pre-update", exe(name)));
    let had = bin.exists();
    if had {
        std::fs::rename(&bin, &backup)
            .with_context(|| format!("renaming {} aside", bin.display()))?;
    }

    let built = Command::new("make")
        .args(make_args)
        .current_dir(make_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        && bin.exists();

    if built {
        if had {
            let _ = std::fs::remove_file(&backup); // safe even if it's this running inode
        }
        Ok(())
    } else {
        if had {
            std::fs::rename(&backup, &bin)
                .with_context(|| format!("reverting {} after a failed refresh", bin.display()))?;
        }
        bail!("`make {}` did not produce a working {name} binary — reverted", make_args.join(" "));
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn tool_present(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// True if `dir` is an agent-ways git checkout (has `.git` and a Makefile that
/// mentions agent-ways).
fn is_app_checkout(dir: &Path) -> bool {
    dir.join(".git").exists()
        && std::fs::read_to_string(dir.join("Makefile"))
            .map(|m| m.contains("agent-ways"))
            .unwrap_or(false)
}

fn run_step(cmd: &mut Command, label: &str) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("running `{label}` (is it installed?)"))?;
    if !status.success() {
        bail!("`{label}` failed ({status}) — see the output above");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ways-update-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn is_app_checkout_requires_git_and_agentways_makefile() {
        let dir = tmp();
        assert!(!is_app_checkout(&dir));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("Makefile"), "all:\n\techo hi\n").unwrap();
        assert!(!is_app_checkout(&dir), "git + Makefile but not agent-ways");
        std::fs::write(dir.join("Makefile"), "# agent-ways\nall:\n").unwrap();
        assert!(is_app_checkout(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_reverts_the_binary_when_the_build_fails() {
        // A make target that can't produce the binary (bogus target) must leave the
        // original binary in place, not stranded.
        let app = tmp();
        std::fs::create_dir_all(app.join("bin")).unwrap();
        let bin = app.join("bin").join("ways");
        std::fs::write(&bin, "OLD-BINARY").unwrap();
        // No Makefile / bogus target -> make fails -> revert.
        let err = refresh_component(&app, "ways", &["definitely-not-a-real-target"], &app).unwrap_err();
        assert!(err.to_string().contains("reverted"), "got: {err}");
        assert!(bin.exists(), "original binary must be restored");
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "OLD-BINARY", "and be the same file");
        assert!(!app.join("bin").join("ways.pre-update").exists(), "backup consumed by the revert");
        let _ = std::fs::remove_dir_all(&app);
    }
}
