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
//!   instead: download-first, build-fallback. Update manages the **whole suite**
//!   uniformly — `ways` (downgrade-guarded), way-embed (its own cache path), and
//!   the rest (`ways-audit`, `attend`, `attend-chat`) through one
//!   `refresh_component` path via their download-first `make` targets. No tool has
//!   a separate lifecycle, and none is optional to keep current; only the build
//!   fallback needs cargo (`attend-chat` falls back to a build until its first
//!   release is cut).
//! - **Rename-then-revert, never leave a broken install.** Each component's
//!   binary is *renamed* aside (not removed) to defeat the "already installed"
//!   early-return; if re-acquiring it fails, the old binary is moved back. A
//!   failed update restores the prior state rather than stranding a binary-less
//!   setup. (Self-replacement is safe on Unix: the running process keeps its own
//!   inode, so replacing `bin/ways` under it is fine and the new binary lands for
//!   the next invocation. On Windows a running `.exe` can't be replaced; there the
//!   command fails early and safe — it shells `bash`/`make`, absent on a bare
//!   Windows install — rather than corrupting anything.)

use crate::paths;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn run(dry_run: bool, git_ref: Option<String>) -> Result<()> {
    let app = paths::data_root();

    // Guard 1: a pre-1.0 in-place clone (~/.claude is itself the repo) must
    // migrate, not update in place.
    let projection = paths::projection_root();
    if super::reconcile::is_legacy_in_place(&projection) {
        bail!(
            "{} looks like a pre-1.0 in-place clone. Migrate to the 1.0 projection \
             first. The migrator was removed in 1.9.0 (ADR-179); run it from the \
             last tag that ships it:\n\
             \x20 git clone --branch ways-v1.8.3 https://github.com/aaronsb/agent-ways\n\
             \x20 cd agent-ways && make build && ./tools/target/release/ways migrate --what-if\n\
             Guide: docs/migration-1.0.md",
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

    // `--ref` is a different lifecycle from "pull the latest release": it pins
    // the app checkout to an arbitrary branch/tag/sha and builds the whole suite
    // from source. An unpublished ref has no pre-built binary to download, and
    // the ADR-150 downgrade guard is intentionally bypassed — you are pinning a
    // ref, not chasing newest. Handled entirely by run_ref_upgrade.
    if let Some(git_ref) = git_ref {
        return run_ref_upgrade(&app, &git_ref, dry_run, has_toolchain);
    }

    let ways_bin = app.join("bin").join(exe("ways"));

    if dry_run {
        println!("ways update would, in {}:", app.display());
        println!("  1. scripts/update.sh          — git pull (autostash-safe)");
        println!("     (binary steps 2-5 run only if the pull changed their source — a");
        println!("      content-only update skips straight to reproject)");
        println!("  2. refresh ways               — if cargo source changed: download pre-built (guarded), else build");
        println!("  3. refresh way-embed          — if tools/way-embed changed: download pre-built, else build (optional)");
        println!("  4. refresh ways-audit/attend/attend-chat — if cargo source changed: download pre-built, else build");
        println!("  5. make relink                — ensure every suite binary is symlinked onto PATH");
        println!("  6. {} corpus + reconcile      — regenerate corpus, reproject ~/.claude", ways_bin.display());
        println!("(dry-run — nothing executed)");
        return Ok(());
    }

    // 1. Pull. Capture HEAD before/after so we can tell what the pull actually
    //    touched. Content lands far more often than a release is cut, so the common
    //    update is docs/ways-only (e.g. a change to core.md). Rebuilding the suite for
    //    that is pure churn: the pre-built binaries lag the source, so "refreshing"
    //    downloads a binary that is behind, discards it under the ADR-150 guard, and
    //    rebuilds from source — producing a binary identical to the one installed.
    //    Gate each build group on whether its own source moved in this pull.
    let head_before = git_head(&app);
    eprintln!("==> pull ({})", app.display());
    run_step(Command::new("bash").arg("scripts/update.sh").current_dir(&app), "git pull")?;
    let head_after = git_head(&app);

    let (committed_cargo, committed_embed) = match (head_before.as_deref(), head_after.as_deref()) {
        // Both HEADs resolved — classify the diff. If git can't produce it, refresh
        // to be safe (Some→unwrap_or). If either HEAD is unreadable (odd/detached
        // state), also refresh to be safe.
        (Some(a), Some(b)) => changed_build_groups(&app, a, b).unwrap_or((true, true)),
        _ => (true, true),
    };
    // The diff above sees committed history only; also fold in any uncommitted
    // binary-source edits so a dirty working tree isn't compiled out by a
    // content-only pull. (The "installed binary matches source" guarantee this gate
    // relies on is really "matches HEAD, assuming the prior build was clean.")
    let (wt_cargo, wt_embed) = working_tree_build_groups(&app);
    let cargo_changed = committed_cargo || wt_cargo;
    let way_embed_changed = committed_embed || wt_embed;

    // Content-only update: nothing that feeds a binary changed. Skip the whole
    // download/build/relink dance and just reproject the pulled content (core.md,
    // ways, skills, hooks). This is the fast path the churn report was about — a
    // metadata pull must not trigger a cargo + cmake rebuild of the suite.
    if !cargo_changed && !way_embed_changed {
        eprintln!("==> binaries: no source change in this update — skipping suite rebuild");
        // relink is idempotent and cheap; keep the prior behavior of self-healing a
        // missing/broken suite PATH symlink on every update, not just rebuilds.
        if let Err(e) = run_step(Command::new("make").arg("relink").current_dir(&app), "relink") {
            eprintln!(
                "  ⚠ could not relink binaries ({e}); run `make install` in {} to fix PATH links.",
                app.display()
            );
        }
        reproject(&app, &ways_bin)?;
        println!("\nUpdate complete (content only — binaries unchanged). Restart Claude Code");
        println!("to pick up the refreshed ways, skills, and hooks.");
        return Ok(());
    }

    // 2. Core — ways. Download-first, rename-revert safe, with the ADR-150
    //    downgrade guard: a pre-built that is behind the pulled source is refused
    //    (built from source instead, or the previous binary kept) so the updater
    //    can never move backward. A failed refresh reverts and CONTINUES (we still
    //    reproject the pulled source) rather than aborting mid-update. Skipped when
    //    the cargo suite's source didn't move — the installed binary already matches.
    let ways_refreshed = if cargo_changed {
        eprintln!("==> refresh ways (pre-built first, downgrade-guarded)");
        match refresh_ways(&app, has_toolchain) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("  ⚠ ways binary NOT refreshed ({e}); keeping the previous binary.");
                false
            }
        }
    } else {
        true // ways source unchanged — the installed binary already matches the pull
    };

    // 3. Matcher — way-embed. Use its own force-refresh target: `rebuild-binary`
    //    owns way-embed's cache install path and is download-first, so the generic
    //    rename dance (which targets app/bin) doesn't apply here. Optional —
    //    semantic matching degrades to regex without it.
    if way_embed_changed {
        eprintln!("==> refresh way-embed (pre-built first)");
        if let Err(e) = run_step(
            Command::new("make").args(["-C", "tools/way-embed", "rebuild-binary"]).current_dir(&app),
            "way-embed refresh",
        ) {
            eprintln!("  ⚠ way-embed not refreshed ({e}); semantic matching degrades to regex until next update.");
        }
    }

    // 4. Awareness — attend/attend-chat. Now download-first (their `make` targets
    //    try the pre-built binary before building), so they refresh even without a
    //    toolchain; the build fallback still needs cargo but the download path does
    //    not. A failed refresh reverts and keeps the current version.
    // These are the rest of the suite — ways-audit (compliance) and the attend
    // awareness pair. `ways` (step 2, downgrade-guarded) and way-embed (step 3,
    // its own cache path) are refreshed above; everything else flows through the
    // same `refresh_component` path so the whole collection updates uniformly —
    // no separate lifecycle for any one tool.
    if cargo_changed {
        eprintln!("==> refresh ways-audit/attend/attend-chat (pre-built first)");
        for comp in ["ways-audit", "attend", "attend-chat"] {
            if let Err(e) = refresh_component(&app, comp, &[comp], &app) {
                eprintln!("  ⚠ {comp} not refreshed ({e}); it keeps its current version.");
            }
        }
    }

    // Ensure every suite binary is linked onto PATH. Refreshing only updates
    // `bin/`; a binary NEWLY ADDED to the suite (e.g. ways-audit for an install
    // that predates it) has no `$XDG_BIN` symlink from the original `make install`,
    // so without this it would sit in `bin/` unreachable. `make relink` is
    // idempotent and only links what exists.
    // Relink only when a build group changed — a binary NEWLY ADDED to the suite
    // needs its PATH symlink. `make relink` is idempotent and only links what exists.
    if cargo_changed || way_embed_changed {
        eprintln!("==> relink suite binaries onto PATH");
        if let Err(e) = run_step(Command::new("make").arg("relink").current_dir(&app), "relink") {
            eprintln!(
                "  ⚠ could not relink binaries ({e}); run `make install` in {} to fix PATH links.",
                app.display()
            );
        }
    }

    // 5. Regenerate the corpus + reproject with whatever ways binary is now in place.
    //    Always runs, so a failed binary refresh doesn't leave the pulled source
    //    un-projected.
    reproject(&app, &ways_bin)?;

    if ways_refreshed {
        println!("\nUpdate complete. Restart Claude Code to pick up the new version");
        println!("(a running session keeps the old hooks, ways, and skills in memory).");
    } else {
        println!("\nSource updated and reprojected, but the ways binary refresh failed (no pre-built");
        println!("available and no build toolchain?). Your install still runs the previous binary —");
        println!("retry `ways update`, or `make update` with a toolchain, then restart Claude Code.");
    }
    Ok(())
}

/// Current HEAD sha of the app checkout, or None if git can't answer.
fn git_head(app: &Path) -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(app)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Which build groups the pull touched, by diffing `a..b` for changed paths.
/// Returns `(cargo_suite_changed, way_embed_changed)`, or None if git can't produce
/// the diff (caller then refreshes to be safe). Equal shas short-circuit to
/// `(false, false)` — the pull was a no-op.
fn changed_build_groups(app: &Path, a: &str, b: &str) -> Option<(bool, bool)> {
    if a == b {
        return Some((false, false));
    }
    let out = Command::new("git")
        .args(["diff", "--name-only", a, b])
        .current_dir(app)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(classify_build_groups(text.lines()))
}

/// Repo-relative path prefixes for compile-time assets embedded into a cargo-suite
/// binary via `include_str!`/`include_bytes!` that live OUTSIDE `tools/`. A change
/// to one of these rebuilds the binary that embeds it, even though the path is not
/// under `tools/`, so the classifier must treat it as cargo source. KEEP IN SYNC
/// with the escaping includes in the tree — the `embedded_assets_outside_tools_are_classified`
/// test walks the source and fails if a new escape isn't listed here.
/// Currently: `hooks/memory-seed/` → embedded into `ways` (see memory_seed.rs).
const EMBEDDED_ASSET_PREFIXES: &[&str] = &["hooks/memory-seed/"];

/// Pure classifier: given changed repo-relative paths, decide which build groups
/// they touch — `(cargo_suite, way_embed)`. Every binary source lives under
/// `tools/`; way-embed (C++/cmake) is `tools/way-embed/`, and the cargo suite
/// (`ways`, `ways-audit`, `attend`, `attend-chat`, and their shared crates) is the
/// rest of `tools/`. The root `Makefile` drives both builds, so a change to it flags
/// both. A few compile-time assets embedded into a binary live outside `tools/`
/// (`EMBEDDED_ASSET_PREFIXES`) and count as cargo source. Anything else (docs,
/// hooks, skills, `*.md`) feeds no binary.
fn classify_build_groups<'a>(paths: impl Iterator<Item = &'a str>) -> (bool, bool) {
    let (mut cargo, mut embed) = (false, false);
    for p in paths.map(str::trim).filter(|p| !p.is_empty()) {
        if p == "Makefile" {
            cargo = true;
            embed = true;
        } else if let Some(rest) = p.strip_prefix("tools/") {
            if rest.starts_with("way-embed/") {
                embed = true;
            } else {
                cargo = true;
            }
        } else if EMBEDDED_ASSET_PREFIXES.iter().any(|pre| p.starts_with(pre)) {
            cargo = true;
        }
    }
    (cargo, embed)
}

/// Which build groups the working tree diverges from HEAD on — staged + unstaged
/// tracked changes. The `changed_build_groups` diff keys on committed HEAD, so
/// uncommitted edits to binary source (unusual on the release channel, but
/// possible) are invisible to it and a content-only pull would skip compiling
/// them. This closes that: `git diff --name-only HEAD` reuses the same classifier.
/// (Untracked-only new files are excluded; a real new module needs a tracked `mod`
/// line, which this catches.) `(false, false)` if git can't answer.
fn working_tree_build_groups(app: &Path) -> (bool, bool) {
    let Some(out) = Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(app)
        .output()
        .ok()
        .filter(|o| o.status.success())
    else {
        return (false, false);
    };
    classify_build_groups(String::from_utf8_lossy(&out.stdout).lines())
}

/// Regenerate the corpus (best-effort — it self-heals on the next session) and
/// reproject `~/.claude` with the installed ways binary. This is where the pulled
/// content (ways, skills, hooks, core.md) reaches the projection, so it runs on
/// every update path — including the content-only fast path.
fn reproject(app: &Path, ways_bin: &Path) -> Result<()> {
    if !ways_bin.exists() {
        bail!("no ways binary at {} after update — cannot reconcile. Re-run the installer.", ways_bin.display());
    }
    eprintln!("==> regenerate corpus");
    if let Err(e) = run_step(Command::new(ways_bin).args(["corpus", "--quiet"]).current_dir(app), "ways corpus") {
        eprintln!("  ⚠ corpus not regenerated now ({e}); it self-heals on the next session.");
    }
    eprintln!("==> reconcile projection");
    run_step(Command::new(ways_bin).arg("reconcile").current_dir(app), "ways reconcile")
}

/// `ways update --ref <ref>` — pin the install to a branch, tag, or commit and
/// build the whole suite from source, then relink + reconcile. Distinct from the
/// release-channel update: fetch-and-checkout instead of pull, force source
/// builds instead of download-first (an unpublished ref has no pre-built
/// binary), and no downgrade guard (an explicit pin is not a downgrade). Lands on
/// a detached HEAD at the ref; `ways update --ref main` returns to the channel.
fn run_ref_upgrade(app: &Path, git_ref: &str, dry_run: bool, has_toolchain: bool) -> Result<()> {
    let ways_bin = app.join("bin").join(exe("ways"));

    if dry_run {
        println!("ways update --ref {git_ref} would, in {}:", app.display());
        println!("  1. git fetch origin {git_ref}");
        println!("  2. git checkout --detach       — pin the checkout to the ref");
        println!("  3. make ways-rebuild ways-audit-rebuild attend-rebuild attend-chat-rebuild  (source, needs cargo)");
        println!("  4. make -C tools/way-embed     — build way-embed from source (needs cmake; optional)");
        println!("  5. make relink                 — symlink every suite binary onto PATH");
        println!("  6. {} corpus + reconcile       — regenerate corpus, reproject ~/.claude", ways_bin.display());
        println!("(dry-run — nothing executed)");
        return Ok(());
    }

    if !has_toolchain {
        bail!(
            "`ways update --ref` builds the suite from source, which needs a Rust toolchain \
             (cargo not found). Install it (https://rustup.rs/), then retry."
        );
    }

    // 1. Fetch the ref. A targeted fetch puts exactly <ref> into FETCH_HEAD,
    //    which then detaches uniformly whether it is a branch, tag, or sha. If
    //    the server won't serve the ref directly (rare — e.g. a bare sha), fall
    //    back to a full fetch and resolve the name in the working checkout.
    eprintln!("==> fetch {git_ref} ({})", app.display());
    let direct = Command::new("git")
        .args(["fetch", "origin", git_ref])
        .current_dir(app)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let checkout_target = if direct {
        "FETCH_HEAD".to_string()
    } else {
        eprintln!("  (couldn't fetch {git_ref} directly; fetching all refs + tags)");
        run_step(
            Command::new("git").args(["fetch", "--tags", "origin"]).current_dir(app),
            "git fetch",
        )?;
        git_ref.to_string()
    };

    // 2. Pin to the ref (detached). Fails loudly on a dirty tree rather than
    //    discarding local changes — the app checkout is normally clean (build
    //    artifacts are gitignored; corpus/settings land outside it).
    eprintln!("==> checkout {git_ref} (detached)");
    run_step(
        Command::new("git")
            .args(["-c", "advice.detachedHead=false", "checkout", "--detach", &checkout_target])
            .current_dir(app),
        "git checkout",
    )?;

    // 3. Build the Rust suite from source — force (no download for an
    //    unpublished ref). The *-rebuild targets each cargo-build and relink.
    eprintln!("==> build ways/ways-audit/attend/attend-chat from source");
    run_step(
        Command::new("make")
            .args(["ways-rebuild", "ways-audit-rebuild", "attend-rebuild", "attend-chat-rebuild"])
            .current_dir(app),
        "suite source build",
    )?;

    // 4. Build way-embed from source. Its default make target is a source build
    //    (cmake), unlike `rebuild-binary` which is download-first — so the ref's
    //    own matcher is what gets installed. Optional: semantic matching degrades
    //    to regex without it.
    eprintln!("==> build way-embed from source");
    match run_step(
        Command::new("make").args(["-C", "tools/way-embed"]).current_dir(app),
        "way-embed source build",
    ) {
        Ok(()) => {
            // The engine's find_way_embed() resolves the cache copy
            // ($XDG_CACHE/agent-ways/user/way-embed) BEFORE the projected
            // ~/.claude/bin symlink. A prior release install leaves a cache copy
            // that would shadow this fresh source build — which lands in bin/ and
            // is relinked into ~/.claude/bin, not the cache — so the ref's
            // way-embed would build but never actually run. Remove the shadowing
            // copy; it is regenerable cache (a later `ways update` re-downloads it).
            let cached = crate::paths::corpus_dir().join(exe("way-embed"));
            if cached.exists() {
                match std::fs::remove_file(&cached) {
                    Ok(()) => eprintln!("     cleared shadowing cache binary {}", cached.display()),
                    Err(e) => eprintln!("  ⚠ could not clear cache binary {} ({e})", cached.display()),
                }
            }
        }
        Err(e) => eprintln!("  ⚠ way-embed not rebuilt ({e}); semantic matching degrades to regex."),
    }

    // 5. Ensure every suite binary is linked onto PATH.
    eprintln!("==> relink suite binaries onto PATH");
    if let Err(e) = run_step(Command::new("make").arg("relink").current_dir(app), "relink") {
        eprintln!(
            "  ⚠ could not relink binaries ({e}); run `make install` in {} to fix PATH links.",
            app.display()
        );
    }

    // 6. Regenerate the corpus (best-effort) and reproject with the newly-built
    //    ways binary. Reconcile always runs so the checked-out source is projected.
    if !ways_bin.exists() {
        bail!("no ways binary at {} after the source build — cannot reconcile.", ways_bin.display());
    }
    eprintln!("==> regenerate corpus");
    if let Err(e) = run_step(Command::new(&ways_bin).args(["corpus", "--quiet"]).current_dir(app), "ways corpus") {
        eprintln!("  ⚠ corpus not regenerated now ({e}); it self-heals on the next session.");
    }
    eprintln!("==> reconcile projection");
    run_step(Command::new(&ways_bin).arg("reconcile").current_dir(app), "ways reconcile")?;

    println!("\nUpgraded to {git_ref} (built from source; the checkout is on a detached HEAD).");
    println!("Return to the release channel with:  ways update --ref main");
    println!("Restart Claude Code to pick up the new version.");
    Ok(())
}

/// Refresh one component binary safely: rename the existing binary aside (so the
/// download/build target re-acquires it instead of early-returning "already
/// installed"), run the target, and on failure move the old binary back — never
/// leaving the slot empty.
fn refresh_component(app: &Path, name: &str, make_args: &[&str], make_dir: &Path) -> Result<()> {
    let bin = app.join("bin").join(exe(name));
    let backup = app.join("bin").join(format!("{}.pre-update", exe(name)));

    // Recover from an interrupted prior run: if the slot is empty but a backup is
    // left over, restore it before we start — never leave the good copy orphaned.
    if !bin.exists() && backup.exists() {
        std::fs::rename(&backup, &bin)
            .with_context(|| format!("restoring an orphaned backup {}", backup.display()))?;
    }

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

/// How a candidate binary's build compares to the pulled source (ADR-150).
#[derive(Debug, PartialEq, Eq)]
enum Freshness {
    /// Same commit, or the source is an ancestor of the candidate — safe to install.
    AtLeastAsNew,
    /// The candidate is strictly behind the source — installing it would downgrade.
    Older,
    /// Lineage can't be established (unparseable/absent provenance, sha not in
    /// local history). Caller decides — prefer building when a toolchain exists.
    Unknown,
}

/// Refresh the `ways` binary safely, with the downgrade guard. Download-first
/// (`make ways`), then compare the freshly-installed binary's baked `git describe`
/// against the pulled source. Only keep the download when it is at least as new;
/// otherwise build from source (toolchain present) or restore the previous binary
/// (none) — never leave an older binary in place, never leave the slot empty.
fn refresh_ways(app: &Path, has_toolchain: bool) -> Result<()> {
    let bin = app.join("bin").join(exe("ways"));
    let backup = app.join("bin").join(format!("{}.pre-update", exe("ways")));

    // Recover an orphaned backup from an interrupted prior run.
    if !bin.exists() && backup.exists() {
        std::fs::rename(&backup, &bin)
            .with_context(|| format!("restoring an orphaned backup {}", backup.display()))?;
    }

    let source = source_describe(app);
    let had = bin.exists();
    if had {
        std::fs::rename(&bin, &backup)
            .with_context(|| format!("renaming {} aside", bin.display()))?;
    }

    // Download-first (build fallback lives inside the Makefile `ways` target).
    let installed = run_make(app, &["ways"]) && bin.exists();
    if !installed {
        if had {
            std::fs::rename(&backup, &bin)?;
        }
        bail!("`make ways` did not produce a ways binary — reverted");
    }

    // Guard: is the just-installed binary at least as new as the pulled source?
    let candidate = read_build_describe(&bin);
    let verdict = match (candidate.as_deref(), source.as_deref()) {
        (Some(c), Some(s)) => compare_freshness(c, s, |a, b| is_ancestor(app, a, b)),
        _ => Freshness::Unknown,
    };
    match &verdict {
        Freshness::AtLeastAsNew => {}
        Freshness::Older => eprintln!(
            "  ⚠ downloaded pre-built ({}) is behind the pulled source ({}) — not a valid update.",
            candidate.as_deref().unwrap_or("?"),
            source.as_deref().unwrap_or("?"),
        ),
        Freshness::Unknown => eprintln!(
            "  ⚠ could not verify the downloaded binary's lineage (candidate={}, source={}).",
            candidate.as_deref().unwrap_or("?"),
            source.as_deref().unwrap_or("?"),
        ),
    }

    match guard_action(&verdict, had, has_toolchain) {
        GuardAction::KeepDownload => {
            if had {
                let _ = std::fs::remove_file(&backup);
            }
            Ok(())
        }
        GuardAction::BuildFromSource => {
            eprintln!("     building ways from source (the pulled checkout is authoritative)…");
            if run_make(app, &["ways-rebuild"]) && bin.exists() {
                if had {
                    let _ = std::fs::remove_file(&backup);
                }
                Ok(())
            } else {
                if had {
                    std::fs::rename(&backup, &bin)?;
                }
                bail!("source build failed — reverted to the previous binary");
            }
        }
        GuardAction::RestorePrevious => {
            // guard_action only returns this when `had`, so the backup exists.
            std::fs::rename(&backup, &bin).with_context(|| {
                format!("restoring {} (unverifiable/older download, no toolchain)", bin.display())
            })?;
            bail!(
                "downloaded ways binary is not a verified upgrade and no toolchain is present \
                 — kept the previous binary (install a toolchain, then `ways update`)"
            );
        }
    }
}

/// What the guard does with a freshly-downloaded binary.
#[derive(Debug, PartialEq, Eq)]
enum GuardAction {
    /// Accept the download.
    KeepDownload,
    /// Build from the pulled source instead (the checkout is authoritative).
    BuildFromSource,
    /// Restore the previously-installed binary — never downgrade to something
    /// older-or-unverifiable when we can't build.
    RestorePrevious,
}

/// The guard's decision table (ADR-150 §3). A download that is provably at least
/// as new is kept. Anything `Older` or `Unknown` is not trusted: build from source
/// when a toolchain is present (the checkout can't be behind itself); else restore
/// the previously-running binary rather than downgrade. The *only* time an
/// unprovable/older download is kept is when there is no previous binary AND no
/// toolchain — an empty slot is worse than an unverifiable one.
fn guard_action(verdict: &Freshness, had: bool, has_toolchain: bool) -> GuardAction {
    match verdict {
        Freshness::AtLeastAsNew => GuardAction::KeepDownload,
        Freshness::Older | Freshness::Unknown => {
            if has_toolchain {
                GuardAction::BuildFromSource
            } else if had {
                GuardAction::RestorePrevious
            } else {
                GuardAction::KeepDownload
            }
        }
    }
}

/// Run a `make` target in `dir`, returning whether it succeeded.
fn run_make(dir: &Path, args: &[&str]) -> bool {
    Command::new("make")
        .args(args)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The pulled source's `git describe`, or None. KEEP IN LOCKSTEP with the flag
/// list in build.rs (`WAYS_BUILD`): the guard compares shas derived from both, so
/// the abbreviation length and `--long`/`--match` flags must be identical or it
/// keys off divergent strings.
fn source_describe(app: &Path) -> Option<String> {
    Command::new("git")
        .args(["describe", "--tags", "--always", "--long", "--dirty", "--match", "ways-v*"])
        .current_dir(app)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The `git describe` a binary bakes, read from its `--version` output —
/// the parenthesized provenance in `ways X.Y.Z (ways-v...-g<sha>)`. None when the
/// binary predates baked provenance (no parenthetical) or can't be run.
fn read_build_describe(bin: &Path) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let start = s.find('(')?;
    let end = s[start..].find(')')? + start;
    let inner = s[start + 1..end].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// Whether commit `a` is an ancestor of commit `b` (Some(true)/Some(false)), or
/// None if git can't answer (unknown sha, not a repo). Mirrors
/// `git merge-base --is-ancestor` exit semantics (0 = ancestor, 1 = not).
fn is_ancestor(app: &Path, a: &str, b: &str) -> Option<bool> {
    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", a, b])
        .current_dir(app)
        .status()
        .ok()?;
    match status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

/// Extract the commit sha from a `git describe --long` string:
/// `ways-v1.0.0-78-gc595437` → `c595437`; a bare `--always` sha → itself; a
/// trailing `-dirty` and the `unknown` sentinel are handled. None when no sha is
/// present (e.g. a legacy tag-only describe with no `-g` suffix).
fn describe_sha(desc: &str) -> Option<String> {
    let d = desc.trim();
    let d = d.strip_suffix("-dirty").unwrap_or(d);
    if d.is_empty() || d == "unknown" {
        return None;
    }
    // The `--long` format always ends `-g<sha>`; take the sha after the last `-g`.
    if let Some(idx) = d.rfind("-g") {
        let sha = &d[idx + 2..];
        if !sha.is_empty() && sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(sha.to_string());
        }
    }
    // `--always` with no reachable tag emits a bare sha.
    if d.len() >= 4 && d.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(d.to_string());
    }
    None
}

/// Compare a candidate binary's build against the source. `is_ancestor(a, b)`
/// answers whether commit `a` precedes `b`. The candidate is `Older` only when
/// its commit is a strict ancestor of the source; equal shas or a
/// non-ancestor (source behind/diverged) are `AtLeastAsNew`; anything we can't
/// resolve is `Unknown`.
fn compare_freshness(
    candidate: &str,
    source: &str,
    is_ancestor: impl Fn(&str, &str) -> Option<bool>,
) -> Freshness {
    let (Some(cand), Some(src)) = (describe_sha(candidate), describe_sha(source)) else {
        return Freshness::Unknown;
    };
    if cand == src {
        return Freshness::AtLeastAsNew;
    }
    // The shas are abbreviated. If the *same* commit is abbreviated to different
    // lengths on the two sides (git auto-lengthens on ambiguity), this fast path
    // misses and we fall to `is_ancestor`, which returns true (a commit is its own
    // ancestor) → `Older` → a needless but safe source rebuild. Same repo + default
    // abbrev makes this rare; the failure is toward caution, never a downgrade.
    match is_ancestor(&cand, &src) {
        Some(true) => Freshness::Older,
        Some(false) => Freshness::AtLeastAsNew,
        None => Freshness::Unknown,
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
    fn refresh_recovers_an_orphaned_backup_from_an_interrupted_run() {
        // Prior run was killed after rename-aside: slot empty, good copy in backup.
        let app = tmp();
        std::fs::create_dir_all(app.join("bin")).unwrap();
        // Use the same OS-aware binary name the production code derives via `exe()`
        // (`ways.exe` on Windows); hardcoding the Unix name here made the recovery
        // branch look for a backup the test never created, failing Windows-only.
        std::fs::write(app.join("bin").join(format!("{}.pre-update", exe("ways"))), "GOOD").unwrap();
        // Even though this refresh's make target fails, recovery restores the good
        // copy first, and the revert keeps it in place.
        let _ = refresh_component(&app, "ways", &["bogus-target"], &app);
        let bin = app.join("bin").join(exe("ways"));
        assert!(bin.exists(), "orphaned backup must be restored");
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "GOOD");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn refresh_reverts_the_binary_when_the_build_fails() {
        // A make target that can't produce the binary (bogus target) must leave the
        // original binary in place, not stranded.
        let app = tmp();
        std::fs::create_dir_all(app.join("bin")).unwrap();
        // OS-aware name (see the sibling recovery test): keeps this test genuinely
        // exercising the revert path on Windows rather than passing vacuously.
        let bin = app.join("bin").join(exe("ways"));
        std::fs::write(&bin, "OLD-BINARY").unwrap();
        // No Makefile / bogus target -> make fails -> revert.
        let err = refresh_component(&app, "ways", &["definitely-not-a-real-target"], &app).unwrap_err();
        assert!(err.to_string().contains("reverted"), "got: {err}");
        assert!(bin.exists(), "original binary must be restored");
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "OLD-BINARY", "and be the same file");
        assert!(!app.join("bin").join(format!("{}.pre-update", exe("ways"))).exists(), "backup consumed by the revert");
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn describe_sha_extracts_the_commit() {
        assert_eq!(describe_sha("ways-v1.0.0-78-gc595437").as_deref(), Some("c595437"));
        assert_eq!(describe_sha("ways-v1.0.0-78-gc595437-dirty").as_deref(), Some("c595437"));
        // Bare `--always` sha (no reachable tag).
        assert_eq!(describe_sha("69a8475").as_deref(), Some("69a8475"));
        assert_eq!(describe_sha("69a8475-dirty").as_deref(), Some("69a8475"));
        // No sha to key on.
        assert_eq!(describe_sha("unknown"), None);
        assert_eq!(describe_sha(""), None);
        assert_eq!(describe_sha("ways-v1.0.0"), None); // legacy tag-only, no -g suffix
    }

    #[test]
    fn compare_freshness_orders_by_commit_ancestry() {
        // Equal shas → at least as new, without consulting git.
        assert_eq!(
            compare_freshness("ways-v1.0.0-0-gc0ffee", "ways-v1.0.0-0-gc0ffee", |_, _| panic!("not called")),
            Freshness::AtLeastAsNew
        );
        // Candidate is an ancestor of source → strictly older → refuse (downgrade).
        assert_eq!(
            compare_freshness("ways-v1.0.0-0-gc0ffee", "ways-v1.0.0-78-gbeef00", |a, b| {
                assert_eq!((a, b), ("c0ffee", "beef00"));
                Some(true)
            }),
            Freshness::Older
        );
        // Candidate not an ancestor (source behind / diverged) → not a downgrade.
        assert_eq!(
            compare_freshness("ways-v1.1.0-0-gaaaa11", "ways-v1.0.0-0-gbbbb22", |_, _| Some(false)),
            Freshness::AtLeastAsNew
        );
        // Git can't resolve the ancestry (sha not local) → Unknown.
        assert_eq!(
            compare_freshness("ways-v1.0.0-0-gabc123", "ways-v1.0.0-1-gdef456", |_, _| None),
            Freshness::Unknown
        );
        // Unparseable candidate provenance (legacy binary → describe_sha None) → Unknown.
        assert_eq!(
            compare_freshness("unknown", "ways-v1.0.0-1-gdef456", |_, _| panic!("not called")),
            Freshness::Unknown
        );
    }

    #[test]
    fn guard_action_never_downgrades() {
        use Freshness::*;
        use GuardAction::*;
        // A proven-fresh download is always kept, regardless of toolchain/previous.
        for &had in &[true, false] {
            for &tc in &[true, false] {
                assert_eq!(guard_action(&AtLeastAsNew, had, tc), KeepDownload);
            }
        }
        // Older/Unknown with a toolchain → build from source (authoritative checkout).
        assert_eq!(guard_action(&Older, true, true), BuildFromSource);
        assert_eq!(guard_action(&Unknown, false, true), BuildFromSource);
        // Older/Unknown, no toolchain, but a previous binary exists → restore it
        // rather than downgrade. (Finding #1: an Unknown legacy download must NOT
        // evict a newer previous binary.)
        assert_eq!(guard_action(&Older, true, false), RestorePrevious);
        assert_eq!(guard_action(&Unknown, true, false), RestorePrevious);
        // Older/Unknown, no toolchain, AND no previous binary → keep the download;
        // an unverifiable binary still beats an empty slot.
        assert_eq!(guard_action(&Older, false, false), KeepDownload);
        assert_eq!(guard_action(&Unknown, false, false), KeepDownload);
    }

    #[test]
    fn classify_build_groups_routes_changed_paths() {
        use super::classify_build_groups as c;
        // Docs/ways-only pull → no binary group (the churn-report case).
        assert_eq!(
            c(["CLAUDE.md", "hooks/ways/core.md", "docs/x.md", "skills/y/SKILL.md"].into_iter()),
            (false, false)
        );
        // Cargo suite source → cargo only.
        assert_eq!(c(["tools/ways-cli/src/main.rs"].into_iter()), (true, false));
        assert_eq!(c(["tools/ways-core/src/lib.rs"].into_iter()), (true, false));
        assert_eq!(c(["tools/Cargo.lock"].into_iter()), (true, false));
        assert_eq!(c(["tools/attend/src/config.rs"].into_iter()), (true, false));
        // way-embed source → embed only.
        assert_eq!(c(["tools/way-embed/way-embed.cpp"].into_iter()), (false, true));
        // Embedded asset outside tools/ (include_str! into ways) → cargo.
        assert_eq!(c(["hooks/memory-seed/seed-v1.md"].into_iter()), (true, false));
        // Root Makefile drives both builds.
        assert_eq!(c(["Makefile"].into_iter()), (true, true));
        // Mixed change touches both.
        assert_eq!(
            c(["tools/way-embed/x.cpp", "tools/attend/src/lib.rs"].into_iter()),
            (true, true)
        );
        // Blank/whitespace lines are ignored.
        assert_eq!(c(["", "  ", "CLAUDE.md"].into_iter()), (false, false));
    }

    // --- Durability guard for finding #1: an embedded asset that escapes `tools/`
    // must be reflected in EMBEDDED_ASSET_PREFIXES, or a change to it silently skips
    // the rebuild. Walk the source for include_str!/include_bytes! literals, resolve
    // each relative to its file, and assert every one that lands outside `tools/` is
    // covered. A future escaping include that isn't listed fails this test. ---

    fn find_include_paths(contents: &str) -> Vec<String> {
        let mut out = Vec::new();
        for marker in ["include_str!(\"", "include_bytes!(\""] {
            let mut rest = contents;
            while let Some(i) = rest.find(marker) {
                let after = &rest[i + marker.len()..];
                if let Some(end) = after.find('"') {
                    out.push(after[..end].to_string());
                    rest = &after[end..];
                } else {
                    break;
                }
            }
        }
        out
    }

    fn normalize(p: &Path) -> std::path::PathBuf {
        use std::path::Component;
        let mut out = std::path::PathBuf::new();
        for comp in p.components() {
            match comp {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    }

    fn visit_rs(dir: &Path, f: &mut dyn FnMut(&Path, &str)) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "target" || n == "llama.cpp") {
                    continue;
                }
                visit_rs(&p, f);
            } else if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(c) = std::fs::read_to_string(&p) {
                    f(&p, &c);
                }
            }
        }
    }

    #[test]
    fn embedded_assets_outside_tools_are_classified() {
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR")); // tools/ways-cli
        let tools = crate_root.parent().unwrap(); // tools/
        let repo = tools.parent().unwrap(); // repo root
        let mut escaping: Vec<String> = Vec::new();
        visit_rs(tools, &mut |file, contents| {
            for lit in find_include_paths(contents) {
                let resolved = normalize(&file.parent().unwrap().join(&lit));
                if !resolved.starts_with(tools) {
                    let rel = resolved
                        .strip_prefix(repo)
                        .unwrap_or(&resolved)
                        .to_string_lossy()
                        .replace('\\', "/");
                    escaping.push(rel);
                }
            }
        });
        // Sanity: the known escape (memory-seed → ways) is found, so a broken walker
        // can't pass this test vacuously.
        assert!(
            escaping.iter().any(|r| r.starts_with("hooks/memory-seed/")),
            "expected to find the memory-seed include escape; found: {escaping:?}",
        );
        for rel in &escaping {
            assert!(
                EMBEDDED_ASSET_PREFIXES.iter().any(|pre| rel.starts_with(pre)),
                "embedded asset `{rel}` escapes tools/ but isn't in EMBEDDED_ASSET_PREFIXES \
                 — a change to it would skip the rebuild. Add its prefix.",
            );
        }
    }
}
