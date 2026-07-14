#!/usr/bin/env bash
# Repair a way-embed binary that macOS kills at launch with
# "SIGKILL (Code Signature Invalid)".
#
# Symptom: SessionStart reports "Embedding engine is NOT functional" and
# `ways corpus` prints `... failed (signal: 9 (SIGKILL))`. Any invocation of
# way-embed — even `--version` — exits 137 with no output, because the kernel
# rejects the binary at exec before dyld loads a single library.
#
# Root cause: the checked-in binary is validly ad-hoc (linker-)signed, and
# `codesign --verify` passes on disk. But after an in-place replace (a version
# upgrade `cp`-ing a new binary over the old path), macOS's Code Signing Monitor
# can hold a stale cached cdhash for that inode that no longer matches the file
# content, and kills the process at launch. See
#   docs/explanation/troubleshooting-embedding-engine.md
#
# Fix: re-sign the binary in place. That rewrites the signature (and the file),
# which invalidates the stale cache and lets the next exec re-evaluate cleanly.
# No rebuild is needed — the binary content was never the problem. Then
# regenerate the corpus so semantic matching comes back on.
#
# macOS only — codesign and this failure mode do not exist elsewhere. On other
# platforms the script is a no-op and exits 0.
#
# Exit code: 0 = repaired (or not applicable), 1 = re-signed but still failing.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT" || exit 2

if [ "$(uname -s)" != "Darwin" ]; then
    echo "Not macOS — the code-signing SIGKILL does not apply here. Nothing to do."
    exit 0
fi

if ! command -v codesign >/dev/null 2>&1; then
    echo "ERROR: codesign not found (expected on macOS)." >&2
    exit 2
fi

# Re-sign every way-embed binary present in bin/ (the invoked `way-embed` plus
# the platform-tagged copy `make setup` produces). Globbing keeps this correct
# if the platform tag ever changes (e.g. darwin-x86_64).
shopt -s nullglob
signed=0
for bin in bin/way-embed bin/way-embed-*; do
    [ -f "$bin" ] || continue
    echo "Re-signing $bin ..."
    codesign --force --sign - "$bin"
    signed=$((signed + 1))
done

if [ "$signed" -eq 0 ]; then
    echo "No way-embed binary found in bin/ — run 'make setup' first." >&2
    exit 2
fi

# Verify the invoked binary now launches (the exec that was being SIGKILL'd).
echo "Verifying bin/way-embed launches ..."
if ! bin/way-embed --version >/dev/null 2>&1; then
    echo "ERROR: bin/way-embed still fails to launch after re-signing." >&2
    echo "  Inspect the crash report for the real reason:" >&2
    echo "    ls -t ~/Library/Logs/DiagnosticReports/way-embed-*.ips | head -1" >&2
    exit 1
fi
echo "  ok: $(bin/way-embed --version)"

# Regenerate the corpus with embeddings so semantic matching is live again.
echo "Regenerating corpus ..."
bin/ways corpus

echo ""
echo "Done. Start a new Claude Code session to pick up the repaired engine."
