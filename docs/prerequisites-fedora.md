# Prerequisites — Fedora / RHEL

```bash
sudo dnf install jq make python3 git
```

For the GitHub CLI (`gh`):

```bash
sudo dnf install 'dnf-command(config-manager)'
sudo dnf config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo
sudo dnf install gh
```

**Already present on most Fedora/RHEL installs:** `bash`, `coreutils` (provides `timeout`, `tr`, `sort`, `wc`, etc.), `grep`, `awk`, `sed`, `find`

**Install Claude Code:**

```bash
curl -fsSL https://claude.ai/install.sh | bash
```

The [Claude Code setup guide](https://code.claude.com/docs/en/setup) lists the other install methods and covers authentication.

**Only for a source build:** when no prebuilt binary fits your platform, the installer builds from source. That needs `cargo` (Rust 1.89 or later, from [rustup](https://rustup.rs/)), and `way-embed` needs cmake and a C++ compiler. `make deps` in the app dir installs cmake and the compiler. See [Finishing an install](finish-install.md).
