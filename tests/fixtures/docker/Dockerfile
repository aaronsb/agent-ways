# Tier 1 live fixture image (ADR-186): Debian with Claude Code installed, an
# unprivileged user, and the tools the installer needs. The agent-ways install
# itself happens at run time inside run-tier1.sh, so the image is reusable
# across branches and flavors.
#
# Build args:
#   CLAUDE_VERSION    Claude Code version to install; `latest` for the newest.
#                     compose.yaml always passes it; the pin lives in
#                     test-live.sh. The default here only covers a bare
#                     `docker build`.
#   CLAUDE_INSTALLER  `native` (claude.ai/install.sh) or `npm`
ARG CLAUDE_VERSION=latest
ARG CLAUDE_INSTALLER=native

FROM debian:trixie-slim

ARG CLAUDE_VERSION
ARG CLAUDE_INSTALLER

# Installer prerequisites (git, jq, make), the download helpers' tools (gh,
# curl), and what docs/prerequisites-debian.md lists. No C++ toolchain: the
# way-embed binary has to arrive as a release download, and the runner
# asserts that it did. A source-build fallback is the defect under test.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      bash ca-certificates curl wget git jq make bc gzip python3 procps unzip gh \
 && rm -rf /var/lib/apt/lists/*

# The npm installer needs Node; the native one does not. Root installs it
# system-wide so `claude` is on PATH for the test user.
RUN if [ "$CLAUDE_INSTALLER" = "npm" ]; then \
      curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
   && apt-get install -y --no-install-recommends nodejs \
   && rm -rf /var/lib/apt/lists/* \
   && if [ "$CLAUDE_VERSION" = "latest" ]; then npm install -g @anthropic-ai/claude-code; \
      else npm install -g "@anthropic-ai/claude-code@${CLAUDE_VERSION}"; fi; \
    fi

RUN useradd -m -s /bin/bash tester
USER tester
WORKDIR /home/tester
ENV HOME=/home/tester
ENV PATH=/home/tester/.local/bin:$PATH

# The checkout is bind-mounted from the host, owned by whatever uid the host
# uses. Git refuses to read a repository owned by another user unless told.
RUN git config --global --add safe.directory '*' \
 && git config --global user.name tester \
 && git config --global user.email tester@example.invalid

# Native installer lands in ~/.local/bin/claude.
RUN if [ "$CLAUDE_INSTALLER" = "native" ]; then \
      if [ "$CLAUDE_VERSION" = "latest" ]; then curl -fsSL https://claude.ai/install.sh | bash; \
      else curl -fsSL https://claude.ai/install.sh | bash -s -- "$CLAUDE_VERSION"; fi; \
    fi \
 && claude --version
