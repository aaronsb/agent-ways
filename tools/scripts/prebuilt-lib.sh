#!/usr/bin/env bash
# Pre-built binary download for every agent-ways component (the Rust suite and
# way-embed). Sourced, not executed: tools/scripts/download-prebuilt.sh is the
# command-line entry point, and tests/prebuilt-lib-test.sh drives these
# functions against a fake `gh`.
#
# Why the retry helpers exist: a transient GitHub API / network blip on a `gh release`
# call used to be swallowed (the call was `... 2>/dev/null`), so an empty
# result read as "no release" and `ways update` silently degraded to a
# from-source build. These helpers retry transient failures and — critically —
# distinguish "the API call failed" from "the API succeeded but found nothing",
# so a real blip is retried and, if it persists, reported honestly instead of
# masquerading as a missing binary.

# Echo the platform slug used in release asset names (`<component>-<platform>`),
# e.g. `ways-darwin-arm64`.
#
# ARM64 has two spellings for one architecture, and `uname -m` reports whichever
# the kernel prefers: `arm64` on Darwin, `aarch64` on Linux. The release assets
# follow the same split — `darwin-arm64` but `linux-aarch64`. So the mapping is
# per-OS, not global: a blanket `arm64`→`aarch64` rewrite asks for
# `darwin-aarch64`, an asset that has never been published, and every Apple
# Silicon download 404s into a silent from-source build.
detect_platform() {
  local os arch
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)
  case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64)
      case "$os" in
        darwin) arch=arm64 ;;
        *) arch=aarch64 ;;
      esac
      ;;
  esac
  printf '%s-%s\n' "$os" "$arch"
}

# Run a command, retrying on failure with exponential backoff. Returns the
# command's own exit status once it succeeds, or non-zero after the last try.
# Progress notes go to stderr so callers can capture stdout cleanly.
retry() {
  local n=1 max="${RETRY_MAX:-3}" delay="${RETRY_DELAY:-2}"
  while true; do
    if "$@"; then
      return 0
    fi
    if [[ "$n" -ge "$max" ]]; then
      return 1
    fi
    echo "  (attempt ${n}/${max} failed; retrying in ${delay}s...)" >&2
    sleep "$delay"
    n=$((n + 1))
    delay=$((delay * 2))
  done
}

# Echo the newest published release tag in REPO whose name starts with PREFIX
# (e.g. `attend-chat-v`), or nothing when no release matches.
#
# The components release on independent cadences from one repo, and `ways`
# cuts most of the releases. Taking the first match in a fixed-size window of
# recent releases stops finding the slower components once enough `ways`
# releases pile up in front of them (#518), so this walks every page of the
# releases API and picks the highest version by `sort -V` within the prefix.
# Drafts and prereleases are skipped: neither is an install target.
#
# Returns non-zero only when the API call fails after retries, so callers can
# tell "could not reach GitHub" apart from "reached it, no matching release".
latest_tag_for_prefix() {
  local repo="$1" prefix="$2" tags
  tags=$(retry gh api --paginate "repos/${repo}/releases?per_page=100" \
    --jq ".[] | select((.draft or .prerelease) | not) | .tag_name | select(startswith(\"${prefix}\"))") || return 1
  [[ -n "$tags" ]] || return 0
  printf '%s\n' "$tags" | sort -V | tail -1
}

# Echo the sha256 of a file, with `sha256sum` (Linux) or `shasum` (macOS).
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Echo the version a binary reports: the second word of `BIN --version`, as
# in `attend 0.15.1 (47d7a97)`. Empty when it prints no second word.
binary_version() {
  "$1" --version 2>/dev/null | awk 'NR == 1 { print $2 }'
}

# Install the pre-built binary of COMPONENT from a GitHub Release.
#
#   prebuilt_install COMPONENT RELEASE_TAG OUTPUT_DIR REPO BUILD_HINT
#
# RELEASE_TAG is a tag, or `latest` for the newest `<COMPONENT>-v*` release.
# The release carries `<COMPONENT>-<platform>` and, usually, `checksums.txt`.
# The binary lands at OUTPUT_DIR/<COMPONENT>, beside its platform-named copy.
# BUILD_HINT is the build-from-source command printed when the download fails.
#
# On success, prints the installed path on stdout and returns 0. On failure it
# says why on stderr, leaves no unverified binary behind, and returns 1, so the
# caller falls back to a source build.
#
# Checksums: when the release lists checksums.txt, the binary must have a line
# there and match it. A checksums.txt that is listed but cannot be fetched, or
# that has no line for the binary, refuses the install rather than skipping
# the check. A release without checksums.txt installs with a warning.
#
# Each call downloads into its own staging dir under OUTPUT_DIR, removed on
# exit, so parallel calls into one OUTPUT_DIR (`make -j setup`) never share a
# checksums.txt. The binary is installed by renaming a checked copy over
# OUTPUT_DIR/<COMPONENT>, which replaces an old file or a dangling symlink in
# one step and never leaves a partial binary at that path.
#
# The body runs in a subshell so the cleanup trap stays local to the call.
prebuilt_install() (
  comp="$1" tag="$2" out_dir="$3" repo="$4" hint="$5"
  platform="$(detect_platform)"
  bin_name="${comp}-${platform}"
  out_file="${out_dir}/${comp}"

  # A working binary is kept when its version is the release's, or when the
  # release cannot be resolved. One at another version is replaced (#772).
  installed=""
  if [[ -x "$out_file" ]] && "$out_file" --version >/dev/null 2>&1; then
    installed=$(binary_version "$out_file")
    [[ -n "$installed" ]] || installed="unknown"
  fi
  keep_installed() {
    echo "${comp} already installed and working: $out_file" >&2
    "$out_file" --version >&2
    echo "$out_file"
    exit 0
  }
  if [[ -n "$installed" && "$tag" != "latest" && "$installed" == "${tag#"${comp}-v"}" ]]; then
    keep_installed
  fi

  if ! command -v gh >/dev/null 2>&1; then
    [[ -n "$installed" ]] && keep_installed
    echo "error: gh CLI not found — build from source instead:" >&2
    echo "  ${hint}" >&2
    exit 1
  fi

  if [[ "$tag" == "latest" ]]; then
    # A failed API call (retries exhausted) is an honest error; an empty
    # answer means the API was reached and no release matches.
    if ! tag=$(latest_tag_for_prefix "$repo" "${comp}-v"); then
      [[ -n "$installed" ]] && keep_installed
      echo "error: could not reach GitHub Releases after retries (network/gh/auth?)." >&2
      echo "  Falling back to build-from-source: ${hint}" >&2
      exit 1
    fi
    if [[ -z "$tag" ]]; then
      [[ -n "$installed" ]] && keep_installed
      echo "No ${comp} release found. Build from source:" >&2
      echo "  ${hint}" >&2
      exit 1
    fi
    [[ "$installed" == "${tag#"${comp}-v"}" ]] && keep_installed
  fi
  [[ -n "$installed" ]] && echo "Replacing ${comp} ${installed} with ${tag}" >&2

  mkdir -p "$out_dir" || exit 1
  stage=$(mktemp -d "${out_dir}/.${comp}.XXXXXX") || exit 1
  trap 'rm -rf "$stage"' EXIT

  echo "Platform: ${platform}" >&2
  echo "Release:  ${tag}" >&2

  if ! assets=$(retry gh release view "$tag" --repo "$repo" --json assets --jq '.assets[].name'); then
    echo "error: could not read assets of ${tag} after retries (network/gh/auth?)." >&2
    echo "  Falling back to build-from-source: ${hint}" >&2
    exit 1
  fi
  if ! grep -qx -- "$bin_name" <<<"$assets"; then
    echo "No pre-built binary for ${platform} in release ${tag}." >&2
    echo "Available binaries:" >&2
    grep -- "^${comp}-" <<<"$assets" | sed 's/^/  /' >&2 || true
    echo "" >&2
    echo "Build from source instead:" >&2
    echo "  ${hint}" >&2
    exit 1
  fi

  echo "Downloading ${bin_name}..." >&2
  if ! retry gh release download "$tag" --repo "$repo" --pattern "$bin_name" \
      --dir "$stage" --clobber; then
    echo "error: download of ${bin_name} failed after retries — building from source instead." >&2
    echo "  ${hint}" >&2
    exit 1
  fi

  if grep -qx 'checksums.txt' <<<"$assets"; then
    if ! retry gh release download "$tag" --repo "$repo" --pattern checksums.txt \
        --dir "$stage" --clobber; then
      echo "error: checksums.txt is in ${tag} but its download failed after retries —" >&2
      echo "  refusing to install ${bin_name} unverified. Build from source: ${hint}" >&2
      exit 1
    fi
    # Match the file-name column exactly: `sha256sum` writes `<hash>  <name>`,
    # or `<hash> *<name>` in binary mode.
    expected=$(awk -v f="$bin_name" '$2 == f || $2 == "*" f { print $1; exit }' "$stage/checksums.txt")
    if [[ -z "$expected" ]]; then
      echo "error: checksums.txt in ${tag} has no line for ${bin_name} —" >&2
      echo "  refusing to install it unverified. Build from source: ${hint}" >&2
      exit 1
    fi
    actual=$(sha256_of "$stage/$bin_name")
    if [[ "$actual" != "$expected" ]]; then
      echo "CHECKSUM MISMATCH for ${bin_name}" >&2
      echo "  Expected: ${expected}" >&2
      echo "  Got:      ${actual}" >&2
      exit 1
    fi
    echo "Checksum verified: ${actual:0:12}..." >&2
  else
    echo "WARNING: no checksums.txt in ${tag} — skipping verification" >&2
  fi

  chmod +x "$stage/$bin_name"
  if ! "$stage/$bin_name" --version >/dev/null 2>&1; then
    echo "WARNING: binary downloaded but won't execute on this platform" >&2
    echo "Build from source instead:" >&2
    echo "  ${hint}" >&2
    exit 1
  fi

  # Same filesystem as the target, so each mv is a rename.
  cp "$stage/$bin_name" "$stage/$comp" || exit 1
  mv -f "$stage/$bin_name" "${out_dir}/${bin_name}" || exit 1
  mv -f "$stage/$comp" "$out_file" || exit 1

  echo "Installed: $out_file ($("$out_file" --version))" >&2
  ls -lh "$out_file" >&2
  echo "$out_file"
)
