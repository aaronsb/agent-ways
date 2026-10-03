# Prerequisites — macOS

Install via [Homebrew](https://brew.sh/):

```bash
brew install jq gh python3 coreutils
```

`coreutils` provides GNU `timeout`, which several hook scripts use. By default Homebrew installs it as `gtimeout`. To make it available as `timeout`, add gnubin to your PATH:

```bash
# Add to ~/.zshrc or ~/.bash_profile
export PATH="$(brew --prefix coreutils)/libexec/gnubin:$PATH"
```

**Already present on macOS:** `git` and `make` (via Xcode CLT), `bash`, `grep`, `awk`, `sed`

> If you don't have Xcode Command Line Tools: `xcode-select --install`

**Install Claude Code:**

```bash
curl -fsSL https://claude.ai/install.sh | bash
```

The [Claude Code setup guide](https://code.claude.com/docs/en/setup) lists the other install methods and covers authentication.

**Log in to `gh`:** run `gh auth login`. The installer downloads the prebuilt binaries through `gh`.

**Only for a source build:** when `gh` is missing or not logged in, or no prebuilt binary fits your platform, the installer builds from source. That needs `cargo` (Rust 1.89 or later, from [rustup](https://rustup.rs/)), and `way-embed` needs cmake and a C++ compiler. `make deps` in the app dir installs cmake and the compiler. See [Finishing an install](finish-install.md).
