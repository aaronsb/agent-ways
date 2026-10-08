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

    // Guard: the app source must be an agent-ways git checkout.
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
        println!("     (binary steps 2-4 run only if the pull changed their source, or a suite");
        println!("      binary's --version is older than its Cargo.toml, or way-embed's than its source)");
        println!("  2. refresh ways               — if cargo source changed: download pre-built (guarded), else build");
        println!("  3. refresh way-embed          — if tools/way-embed changed or way-embed is older than its source:");
        println!("     latest release, kept when at or ahead of it; else build (optional)");
        println!("  4. refresh the rest of tools/suite-bins — if cargo source changed: download pre-built, else build;");
        println!("     a stale binary whose source did not change: latest release in place, else build");
        println!("  5. make relink                — install any suite binary still missing, symlink the suite onto PATH");
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
    //    Gate each build group on whether its own source moved in this pull, and
    //    each suite binary also on whether it is older than its Cargo.toml.
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
    // way-embed's source can also move past the installed binary before its
    // release exists; once the release ships, no later pull need touch
    // tools/way-embed, so the version is checked as well (#772).
    let way_embed_changed = committed_embed
        || wt_embed
        || crate::paths::way_embed_in(&crate::paths::corpus_dir()).is_some_and(|bin| way_embed_stale(&app, &bin));

    // The diff misses a binary the source moved past before this pull, as when an
    // earlier update ran before the release assets existed (#772). A suite binary
    // older than its Cargo.toml at HEAD is refreshed whether or not the pull
    // touched it.
    let stale = if cargo_changed { Vec::new() } else { stale_suite_binaries(&app) };

    let before = binary_versions(&app);

    // Content-only update: nothing that feeds a binary changed and no binary is
    // older than its source. Skip the whole
    // download/build/relink dance and just reproject the pulled content (core.md,
    // ways, skills, hooks). This is the fast path the churn report was about — a
    // metadata pull must not trigger a cargo + cmake rebuild of the suite. The one
    // build it allows is relink's for a suite binary the install lacks, when the
    // pre-built download fails and cargo is present; it stops once the binary exists.
    if !cargo_changed && !way_embed_changed && stale.is_empty() {
        eprintln!("==> binaries: no source change in this update — skipping suite rebuild");
        // relink is idempotent and cheap when the suite is complete. It runs on every
        // update, not just rebuilds: it installs a suite binary the install lacks and
        // self-heals a missing or broken PATH symlink.
        if let Err(e) = run_step(Command::new("make").arg("relink").current_dir(&app), "relink") {
            eprintln!(
                "  ⚠ could not relink binaries ({e}); run `make link` in {} to fix PATH links.",
                app.display()
            );
        }
        reproject(&app, &ways_bin)?;
        print_complete(&version_changes(&before, &binary_versions(&app)));
        return Ok(());
    }

    // 2. Core — ways. Download-first, rename-revert safe, with the ADR-150
    //    downgrade guard: a pre-built that is behind the pulled source is refused
    //    (built from source instead, or the previous binary kept) so the updater
    //    can never move backward. A failed refresh reverts and CONTINUES (we still
    //    reproject the pulled source) rather than aborting mid-update. Skipped when
    //    the cargo suite's source didn't move; a stale ways is refreshed in step 4.
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
        true // ways source unchanged
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
        for comp in suite_bins(&app).iter().filter(|n| *n != "ways") {
            eprintln!("==> refresh {comp} (pre-built first)");
            if let Err(e) = refresh_component(&app, comp, &[comp.as_str()], &app) {
                eprintln!("  ⚠ {comp} not refreshed ({e}); it keeps its current version.");
            }
        }
    }
    for s in &stale {
        eprintln!("==> {} is {}, source is {}: refreshing it", s.name, s.installed, s.source);
        if let Err(e) = refresh_stale(&app, s, has_toolchain) {
            eprintln!("  ⚠ {e}");
        }
    }

    // Ensure every suite binary is installed and linked onto PATH. Refreshing only
    // updates `bin/`; a binary NEWLY ADDED to the suite has no `$XDG_BIN` symlink
    // from the original `make install`. `make relink` installs any suite binary
    // missing from `bin/`, then links what exists. The pulled Makefile owns the
    // suite list, so an updater older than a component still installs it here.
    eprintln!("==> relink suite binaries onto PATH");
    if let Err(e) = run_step(Command::new("make").arg("relink").current_dir(&app), "relink") {
        eprintln!(
            "  ⚠ could not relink binaries ({e}); run `make link` in {} to fix PATH links.",
            app.display()
        );
    }

    // 5. Regenerate the corpus + reproject with whatever ways binary is now in place.
    //    Always runs, so a failed binary refresh doesn't leave the pulled source
    //    un-projected.
    reproject(&app, &ways_bin)?;

    if ways_refreshed {
        print_complete(&version_changes(&before, &binary_versions(&app)));
    } else {
        println!("\nSource updated and reprojected, but the ways binary refresh failed (no pre-built");
        println!("available and no build toolchain?). Your install still runs the previous binary —");
        println!("retry `ways update`, or `make update-binaries` with a toolchain, then restart Claude Code.");
    }
    judge_setup();
    Ok(())
}

/// The relevance judge's check and offer, after an update that succeeded. Its
/// failure is reported and never fails the update.
fn judge_setup() {
    println!();
    if let Err(e) = super::judge::setup() {
        eprintln!("ways: judge setup: {e:#}");
    }
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

/// The suite binaries, from `tools/suite-bins`: the list the pulled Makefile
/// builds and links, so an updater older than a component still refreshes it.
fn suite_bins(app: &Path) -> Vec<String> {
    std::fs::read_to_string(app.join("tools/suite-bins"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_lowercase()))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

/// A suite binary whose version differs from its source's.
#[derive(Debug, PartialEq, Eq)]
struct Stale {
    name: String,
    installed: String,
    source: String,
}

/// Every installed suite binary whose `--version` is older than the version in
/// the `Cargo.toml` under `tools/` whose package bears its name. A binary that
/// is missing, does not run, or reports no version is left to `make relink`;
/// one ahead of its source is left alone.
fn stale_suite_binaries(app: &Path) -> Vec<Stale> {
    let sources = source_versions(app);
    suite_bins(app)
        .into_iter()
        .filter_map(|name| {
            let installed = installed_version(&app.join("bin").join(exe(&name)))?;
            let source = sources.iter().find(|(n, _)| *n == name)?.1.clone();
            version_older(&installed, &source).then_some(Stale { name, installed, source })
        })
        .collect()
}

/// What an update can tell about one binary: the version it reports, without
/// its name (`1.35.0 (ways-v1.35.0-0-g48ce52b)`), and its size and content
/// hash, which move on a rebuild at the same version and stay put when the
/// same release is downloaded again.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    version: String,
    stamp: Option<(u64, u64)>,
}

/// A file's size and a hash of its bytes, for telling two copies apart
/// within one update run.
fn content_stamp(p: &Path) -> Option<(u64, u64)> {
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(p).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Some((bytes.len() as u64, h.finish()))
}

/// Each binary an update can refresh: the suite under `bin/` and the cached
/// way-embed. A binary that does not answer `--version` is left out.
fn binary_versions(app: &Path) -> Vec<(String, Seen)> {
    let mut bins: Vec<(String, std::path::PathBuf)> =
        suite_bins(app).into_iter().map(|n| (n.clone(), app.join("bin").join(exe(&n)))).collect();
    if let Some(p) = crate::paths::way_embed_in(&crate::paths::corpus_dir()) {
        bins.push(("way-embed".to_string(), p));
    }
    bins.into_iter()
        .filter_map(|(name, p)| {
            let line = version_line(&p)?;
            let version = line.strip_prefix(name.as_str()).unwrap_or(&line).trim().to_string();
            let stamp = content_stamp(&p);
            Some((name, Seen { version, stamp }))
        })
        .collect()
}

/// One entry per binary that moved: `name old → new`, `name v (rebuilt)` for a
/// new file at the same version, `name v (new)`, or `name v → not answering`.
fn version_changes(before: &[(String, Seen)], after: &[(String, Seen)]) -> Vec<String> {
    let mut changes: Vec<String> = after
        .iter()
        .filter_map(|(name, now)| match before.iter().find(|(b, _)| b == name) {
            None => Some(format!("{name} {} (new)", now.version)),
            Some((_, was)) if was.version != now.version => Some(format!("{name} {} → {}", was.version, now.version)),
            Some((_, was)) if was.stamp != now.stamp => Some(format!("{name} {} (rebuilt)", now.version)),
            Some(_) => None,
        })
        .collect();
    changes.extend(
        before
            .iter()
            .filter(|(name, _)| !after.iter().any(|(a, _)| a == name))
            .map(|(name, was)| format!("{name} {} → not answering", was.version)),
    );
    changes
}

/// The closing lines of an update whose ways binary is in place.
fn print_complete(changes: &[String]) {
    if changes.is_empty() {
        println!("\nUpdate complete (binaries unchanged). Restart Claude Code to pick up the");
        println!("refreshed ways, skills, and hooks.");
    } else {
        println!("\nUpdate complete: {}.", changes.join(", "));
        println!("Restart Claude Code to pick up the new binaries (a running session keeps the");
        println!("old hooks, ways, and skills in memory).");
    }
}

/// way-embed's version in its source: the `#define VERSION` in way-embed.cpp.
fn way_embed_source_version(app: &Path) -> Option<String> {
    std::fs::read_to_string(app.join("tools/way-embed/way-embed.cpp"))
        .ok()?
        .lines()
        .find_map(|l| {
            let rest = l.trim().strip_prefix("#define VERSION ")?;
            Some(rest.split('"').nth(1)?.to_string())
        })
}

/// Whether the installed way-embed is older than its source. A binary that
/// does not run, or a source with no version, is not reported stale.
fn way_embed_stale(app: &Path, bin: &Path) -> bool {
    match (installed_version(bin), way_embed_source_version(app)) {
        (Some(installed), Some(source)) => version_older(&installed, &source),
        _ => false,
    }
}

/// `(package name, version)` of each crate directly under `tools/`.
fn source_versions(app: &Path) -> Vec<(String, String)> {
    let Ok(dirs) = std::fs::read_dir(app.join("tools")) else { return Vec::new() };
    dirs.flatten()
        .filter_map(|d| std::fs::read_to_string(d.path().join("Cargo.toml")).ok())
        .filter_map(|m| Some((package_field(&m, "name")?, package_field(&m, "version")?)))
        .collect()
}

/// The first line a binary prints for `--version`.
fn version_line(bin: &Path) -> Option<String> {
    // A binary just written, here or by a parallel process, can refuse to exec
    // with ETXTBSY while another fork still holds its write handle; it clears
    // in milliseconds, so a few short retries tell it apart from a broken binary.
    let mut tries = 0;
    let out = loop {
        match Command::new(bin).arg("--version").output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            r => break r.ok().filter(|o| o.status.success())?,
        }
    };
    String::from_utf8_lossy(&out.stdout).lines().next().map(|l| l.trim().to_string())
}

/// The version a binary reports: the second word of `--version`, as in
/// `attend 0.15.1 (47d7a97)`.
fn installed_version(bin: &Path) -> Option<String> {
    version_line(bin)?.split_whitespace().nth(1).map(str::to_string)
}

/// Whether version `a` is older than `b`: numeric `X.Y.Z` cores compared in
/// order, and at an equal core a pre-release (`1.2.0-rc1`) is older than the
/// release. Versions that do not parse are older when they differ.
fn version_older(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> Option<(Vec<u64>, bool)> {
        let (core, pre) = match v.split_once('-') {
            Some((c, _)) => (c, true),
            None => (v, false),
        };
        Some((core.split('.').map(|n| n.parse().ok()).collect::<Option<_>>()?, pre))
    }
    match (parse(a), parse(b)) {
        (Some((ac, ap)), Some((bc, bp))) => ac < bc || (ac == bc && ap && !bp),
        _ => a != b,
    }
}

/// A string field under `[package]` in a Cargo.toml, such as `name` or `version`.
fn package_field(manifest: &str, field: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(value) = line.strip_prefix(field).and_then(|r| r.trim_start().strip_prefix('=')) {
                return Some(value.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Regenerate the corpus (best-effort — a failure keeps the previous corpus and
/// the next session retries) and
/// reproject `~/.claude` with the installed ways binary. This is where the pulled
/// content (ways, skills, hooks, core.md) reaches the projection, so it runs on
/// every update path — including the content-only fast path.
fn reproject(app: &Path, ways_bin: &Path) -> Result<()> {
    if !ways_bin.exists() {
        bail!("no ways binary at {} after update — cannot reconcile. Re-run the installer.", ways_bin.display());
    }
    eprintln!("==> regenerate corpus");
    if let Err(e) = run_step(Command::new(ways_bin).args(["corpus", "--quiet"]).current_dir(app), "ways corpus") {
        eprintln!("  ⚠ corpus not regenerated ({e}); the next session retries.");
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
        println!("  3. make ways-rebuild ways-audit-rebuild [ways-mcp-rebuild] [ways-agent-rebuild] attend-rebuild attend-chat-rebuild  (source, needs cargo)");
        println!("  4. make -C tools/way-embed     — build way-embed from source (needs cmake; optional)");
        println!("  5. make relink                 — install any suite binary still missing, symlink the suite onto PATH");
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
    //    ways-mcp joins when the ref's Makefile has its target; a ref that
    //    predates the server has none.
    let mut targets = vec!["ways-rebuild", "ways-audit-rebuild", "attend-rebuild", "attend-chat-rebuild"];
    if has_make_target(app, "ways-mcp-rebuild") {
        targets.insert(2, "ways-mcp-rebuild");
    }
    // Likewise ways-agent (the relevance gate); older refs lack the target.
    if has_make_target(app, "ways-agent-rebuild") {
        let at = targets.iter().position(|t| *t == "attend-rebuild").unwrap_or(targets.len());
        targets.insert(at, "ways-agent-rebuild");
    }
    eprintln!("==> build the suite from source ({})", targets.join(" "));
    run_step(Command::new("make").args(&targets).current_dir(app), "suite source build")?;

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
            // The engine's paths::way_embed() resolves the cache copy
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

    // 5. Install any suite binary still missing and link the suite onto PATH.
    eprintln!("==> relink suite binaries onto PATH");
    if let Err(e) = run_step(Command::new("make").arg("relink").current_dir(app), "relink") {
        eprintln!(
            "  ⚠ could not relink binaries ({e}); run `make link` in {} to fix PATH links.",
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
        eprintln!("  ⚠ corpus not regenerated ({e}); the next session retries.");
    }
    eprintln!("==> reconcile projection");
    run_step(Command::new(&ways_bin).arg("reconcile").current_dir(app), "ways reconcile")?;

    println!("\nUpgraded to {git_ref} (built from source; the checkout is on a detached HEAD).");
    println!("Return to the release channel with:  ways update --ref main");
    println!("Restart Claude Code to pick up the new version.");
    judge_setup();
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

/// Refresh a stale suite binary whose source this pull did not change. The
/// binary stays in place while `download-prebuilt.sh` checks the latest release
/// (one API call; replaced only by a newer release), so a release that lags its
/// version bump is not downloaded again on every update. A binary still older
/// than its source is then built, when cargo is present, with the
/// rename-revert protection of `refresh_component`.
fn refresh_stale(app: &Path, stale: &Stale, has_toolchain: bool) -> Result<()> {
    let name = stale.name.as_str();
    let fetched = Command::new("bash")
        .args(["tools/scripts/download-prebuilt.sh", name])
        .current_dir(app)
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let Some(now) = stale_suite_binaries(app).into_iter().find(|s| s.name == name) else {
        return Ok(());
    };
    // The download script says on stderr why it failed, or why it kept the
    // binary without checking (no gh, GitHub unreachable).
    let why = if fetched { format!("no {name} {} release reachable", now.source) } else { "the release download failed".to_string() };
    if !has_toolchain {
        bail!(
            "{name} is {}, source is {}: {why}, and no toolchain to build it; the next update checks again",
            now.installed, now.source
        );
    }
    eprintln!("     {why}; building {name} from source");
    refresh_component(app, name, &[&format!("{name}-rebuild")], app)
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

/// Whether the Makefile in `dir` defines `target`. `make -n` exits 2 on a
/// missing rule and 0 on a target it could run.
fn has_make_target(dir: &Path, target: &str) -> bool {
    Command::new("make")
        .args(["-n", target])
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
    // The shas are abbreviated, and git sizes the abbreviation by the repository's
    // object count: the release build's shallow checkout gives 7 characters, a full
    // clone of this repo 8. One sha prefixing the other is the same commit. Without
    // this, `is_ancestor` reports the commit as its own ancestor → `Older`, and an
    // install with no toolchain keeps its previous binary.
    if cand.starts_with(&src) || src.starts_with(&cand) {
        return Freshness::AtLeastAsNew;
    }
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
    fn version_older_orders_by_numeric_core_then_pre_release() {
        assert!(version_older("0.4.0", "0.5.1"));
        assert!(version_older("0.9.0", "0.10.0"), "numeric, not string, order");
        assert!(version_older("1.2.0-rc1", "1.2.0"));
        assert!(!version_older("1.2.0-rc1", "1.2.0-rc2"), "two pre-releases of one core are level");
        assert!(!version_older("0.5.1", "0.5.1"));
        assert!(!version_older("0.6.0", "0.5.1"), "a binary ahead of its source is not stale");
        assert!(version_older("garbage", "0.5.1"));
        assert!(!version_older("garbage", "garbage"));
    }

    #[test]
    fn package_field_reads_the_package_table_only() {
        let manifest = "[workspace]\nversion = \"9.9.9\"\n\n[package]\nname = \"attend\"\nversion = \"0.15.1\"\n\n[dependencies]\nversion = \"1\"\n";
        assert_eq!(package_field(manifest, "version").as_deref(), Some("0.15.1"));
        assert_eq!(package_field(manifest, "name").as_deref(), Some("attend"));
        assert_eq!(package_field("[package]\nversion=\"1.2.3\"\n", "version").as_deref(), Some("1.2.3"));
        assert_eq!(package_field("[package]\nversion.workspace = true\n", "version"), None);
        assert_eq!(package_field("[dependencies]\nversion = \"1\"\n", "version"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_whose_version_differs_from_its_cargo_toml_is_stale() {
        use std::os::unix::fs::PermissionsExt;
        let app = tmp();
        let bin_dir = app.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let crate_at = |dir: &str, name: &str, version: &str| {
            let d = app.join("tools").join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("Cargo.toml"), format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")).unwrap();
        };
        let bin = |name: &str, out: &str| {
            let b = bin_dir.join(name);
            std::fs::write(&b, format!("#!/bin/sh\necho '{out}'\n")).unwrap();
            std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        crate_at("ways-cli", "ways", "1.30.1");
        bin("ways", "ways 1.30.1 (ways-v1.30.1-2-gaabfd2c)");
        crate_at("ways-agent", "ways-agent", "0.5.1");
        bin("ways-agent", "ways-agent 0.4.0");
        crate_at("attend", "attend", "0.15.1");
        bin("attend", "attend 0.15.1 (47d7a97)");
        crate_at("attend-chat", "attend-chat", "0.6.2");
        bin("attend-chat", "attend-chat 0.7.0-dev"); // ahead of its source: left alone
        crate_at("ways-audit", "ways-audit", "1.0.1"); // no binary: left to relink
        bin("unlisted", "unlisted 0.0.1"); // not in suite-bins: ignored
        std::fs::write(app.join("tools/suite-bins"), "# comment\nways\nways-audit\nways-agent\nattend\nattend-chat\n").unwrap();

        assert_eq!(
            stale_suite_binaries(&app),
            vec![Stale { name: "ways-agent".into(), installed: "0.4.0".into(), source: "0.5.1".into() }]
        );
        let _ = std::fs::remove_dir_all(&app);
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
    fn the_closing_line_names_what_moved_including_a_rebuild_at_the_same_version() {
        let (t0, t1) = (0xa, 0xb);
        let seen = |v: &str, len: u64, hash: u64| Seen { version: v.to_string(), stamp: Some((len, hash)) };
        let before = vec![
            ("ways".to_string(), seen("1.35.0 (ways-v1.35.0-0-g48ce52b)", 10, t0)),
            ("attend".to_string(), seen("0.15.3 (a8b0511)", 20, t0)),
            ("way-embed".to_string(), seen("1.1.2", 30, t0)),
            ("ways-audit".to_string(), seen("1.0.2", 40, t0)),
        ];
        assert!(version_changes(&before, &before).is_empty(), "nothing moved");

        let after = vec![
            ("ways".to_string(), seen("1.35.0 (ways-v1.35.0-3-gabc1234)", 10, t1)),
            ("attend".to_string(), seen("0.15.3 (a8b0511)", 20, t0)),
            ("way-embed".to_string(), seen("1.1.2", 30, t1)),
            ("ways-mcp".to_string(), seen("0.1.0", 50, t1)),
        ];
        assert_eq!(
            version_changes(&before, &after),
            [
                "ways 1.35.0 (ways-v1.35.0-0-g48ce52b) → 1.35.0 (ways-v1.35.0-3-gabc1234)",
                "way-embed 1.1.2 (rebuilt)",
                "ways-mcp 0.1.0 (new)",
                "ways-audit 1.0.2 → not answering",
            ]
        );
    }

    #[test]
    fn the_same_bytes_written_again_carry_the_same_stamp() {
        let dir = tmp();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, b"release bytes").unwrap();
        std::fs::write(&b, b"release bytes").unwrap();
        assert_eq!(content_stamp(&a), content_stamp(&b), "a re-download of the same release is unchanged");
        std::fs::write(&b, b"rebuilt bytes").unwrap();
        assert_ne!(content_stamp(&a), content_stamp(&b), "a rebuild is seen");
    }

    #[cfg(unix)]
    #[test]
    fn way_embed_is_stale_only_when_it_reports_a_version_older_than_its_source() {
        use std::os::unix::fs::PermissionsExt;
        let app = tmp();
        let bin = app.join("way-embed");
        let reports = |v: &str| {
            std::fs::write(&bin, format!("#!/bin/sh\necho 'way-embed {v}'\n")).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        reports("1.1.2");
        assert!(!way_embed_stale(&app, &bin), "no source: not stale");

        std::fs::create_dir_all(app.join("tools/way-embed")).unwrap();
        std::fs::write(
            app.join("tools/way-embed/way-embed.cpp"),
            "#define VERSION_MAJOR 9\n#define VERSION \"1.2.0\" // the release\nint main() {}\n",
        )
        .unwrap();
        assert_eq!(way_embed_source_version(&app).as_deref(), Some("1.2.0"));
        assert!(way_embed_stale(&app, &bin), "1.1.2 under a 1.2.0 source");
        reports("1.2.0");
        assert!(!way_embed_stale(&app, &bin), "matches its source");
        reports("1.3.0");
        assert!(!way_embed_stale(&app, &bin), "ahead of its source");
        std::fs::remove_file(&bin).unwrap();
        assert!(!way_embed_stale(&app, &bin), "missing binary: left to relink");
    }

    #[cfg(unix)]
    fn stale_app(release_version: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let app = tmp();
        let exec = |p: &Path, body: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        std::fs::create_dir_all(app.join("tools/ways-agent")).unwrap();
        std::fs::write(app.join("tools/suite-bins"), "ways-agent\n").unwrap();
        std::fs::write(app.join("tools/ways-agent/Cargo.toml"), "[package]\nname = \"ways-agent\"\nversion = \"0.5.1\"\n").unwrap();
        exec(&app.join("bin/ways-agent"), "#!/bin/sh\necho 'ways-agent 0.4.0'\n");
        // The fake download logs its call and installs the latest release.
        exec(
            &app.join("tools/scripts/download-prebuilt.sh"),
            &format!("#!/bin/sh\necho download >> calls\nprintf '#!/bin/sh\\necho \"ways-agent {release_version}\"\\n' > bin/ways-agent\n"),
        );
        std::fs::write(
            app.join("Makefile"),
            "ways-agent-rebuild:\n\techo rebuild >> calls\n\tprintf '#!/bin/sh\\necho \"ways-agent 0.5.1\"\\n' > bin/ways-agent\n\tchmod +x bin/ways-agent\n",
        )
        .unwrap();
        app
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_binary_takes_the_latest_release_in_place_and_builds_only_when_still_behind() {
        let stale = |app: &Path| stale_suite_binaries(app).pop().expect("ways-agent is stale");
        let calls = |app: &Path| std::fs::read_to_string(app.join("calls")).unwrap_or_default();

        // The release caught up with the source: one download, no build.
        let app = stale_app("0.5.1");
        refresh_stale(&app, &stale(&app), true).unwrap();
        assert_eq!(calls(&app), "download\n");
        assert!(stale_suite_binaries(&app).is_empty());
        assert!(!app.join("bin/ways-agent.pre-update").exists(), "the binary was never moved aside for the download");
        let _ = std::fs::remove_dir_all(&app);

        // The release lags the source: the download leaves it behind, so it is built.
        let app = stale_app("0.5.0");
        refresh_stale(&app, &stale(&app), true).unwrap();
        assert_eq!(calls(&app), "download\nrebuild\n");
        assert!(stale_suite_binaries(&app).is_empty());
        let _ = std::fs::remove_dir_all(&app);

        // No toolchain: it stays at the latest release and says why.
        let app = stale_app("0.5.0");
        let err = refresh_stale(&app, &stale(&app), false).unwrap_err().to_string();
        assert!(err.contains("no ways-agent 0.5.1 release reachable"), "got: {err}");
        assert_eq!(calls(&app), "download\n");
        let _ = std::fs::remove_dir_all(&app);

        // A failed download says so, and the build still runs.
        let app = stale_app("0.5.0");
        std::fs::write(app.join("tools/scripts/download-prebuilt.sh"), "#!/bin/sh\necho download >> calls\nexit 1\n").unwrap();
        let err = refresh_stale(&app, &stale(&app), false).unwrap_err().to_string();
        assert!(err.contains("the release download failed"), "got: {err}");
        refresh_stale(&app, &stale(&app), true).unwrap();
        assert_eq!(calls(&app), "download\ndownload\nrebuild\n");
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
        // The same commit abbreviated to different lengths (shallow CI checkout vs a
        // full clone) is the same commit, without consulting git.
        assert_eq!(
            compare_freshness("ways-v1.24.0-0-g64ef02c", "ways-v1.24.0-0-g64ef02cb", |_, _| panic!("not called")),
            Freshness::AtLeastAsNew
        );
        assert_eq!(
            compare_freshness("ways-v1.24.0-0-g64ef02cb", "ways-v1.24.0-0-g64ef02c", |_, _| panic!("not called")),
            Freshness::AtLeastAsNew
        );
        // Different lengths that are not prefixes are different commits: git decides.
        assert_eq!(
            compare_freshness("ways-v1.0.0-0-gabc1234", "ways-v1.0.0-3-gabd12345", |a, b| {
                assert_eq!((a, b), ("abc1234", "abd12345"));
                Some(true)
            }),
            Freshness::Older
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
        // Read at run time: a test binary reused from another checkout keeps the
        // path it was built at.
        let crate_root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into())); // tools/ways-cli
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
