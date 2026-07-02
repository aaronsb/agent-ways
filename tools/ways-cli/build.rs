use std::process::Command;

fn main() {
    // Bake git commit hash into the binary (used by the startup banner).
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=WAYS_COMMIT={commit}");

    // Bake `git describe` for version-truth and downgrade detection (ADR-150).
    // `--match ways-v*` pins the description to the *ways* release lineage (the
    // repo carries per-component and model tags; without the match the nearest by
    // distance — e.g. a way-embed/model tag — would show, misleadingly). `--long`
    // always emits the `-N-g<sha>` suffix (even exactly at a tag) so a sha is
    // always parseable; `--always` falls back to a bare sha if no ways tag is
    // reachable; `--dirty` flags a build with uncommitted changes. Example:
    // `ways-v1.0.0-78-gc595437`. The downgrade guard keys on the sha, not the tag.
    //
    // KEEP IN LOCKSTEP with `source_describe()` in src/cmd/update.rs — the guard
    // compares shas from both, so the abbreviation (default length here) and the
    // `--long`/`--match` flags must match or the comparison keys off divergent
    // strings. (They can't share a const: build.rs runs before the crate exists.)
    let build = git(&["describe", "--tags", "--always", "--long", "--dirty", "--match", "ways-v*"])
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=WAYS_BUILD={build}");

    // Rerun when HEAD, the tag/ref set, or the reflog changes so the baked strings
    // stay honest. `.git/logs/HEAD` is appended on every commit/checkout/reset/
    // merge — it catches ordinary same-branch commits that update a file *inside*
    // `.git/refs/heads/` without bumping the watched `.git/refs` directory mtime.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/logs/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-changed=.git/packed-refs");
}

/// Run a git command, returning its trimmed stdout on success.
fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}
