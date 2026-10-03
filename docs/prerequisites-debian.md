# Prerequisites — Debian / Ubuntu

```bash
sudo apt install jq make python3 git
```

For the GitHub CLI (`gh`):

```bash
# See https://github.com/cli/cli/blob/trunk/docs/install_linux.md
sudo mkdir -p -m 755 /etc/apt/keyrings
wget -qO- https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
sudo apt update && sudo apt install gh
```

**Already present on most Debian/Ubuntu installs:** `bash`, `coreutils` (provides `timeout`, `tr`, `sort`, `wc`, etc.), `grep`, `awk`, `sed`, `find`

**Install Claude Code:**

```bash
curl -fsSL https://claude.ai/install.sh | bash
```

The [Claude Code setup guide](https://code.claude.com/docs/en/setup) lists the other install methods and covers authentication.

**Log in to `gh`:** run `gh auth login`. The installer downloads the prebuilt binaries through `gh`.

**Only for a source build:** when `gh` is missing or not logged in, or no prebuilt binary fits your platform, the installer builds from source. That needs `cargo` (Rust 1.89 or later, from [rustup](https://rustup.rs/)), and `way-embed` needs cmake and a C++ compiler. `make deps` in the app dir installs cmake and the compiler. See [Finishing an install](finish-install.md).
