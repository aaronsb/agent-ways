# adr-migrate setup: a copy of agent-ways' own record corpus, on the adr/v1
# contract it declares, with the v1-capable tool vendored. The rehearsal for
# #566: a real agent migrates real records before anyone does it for real.
APP="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
mkdir -p docs/scripts
cp -r "$APP/docs/architecture" docs/
cp "$HOME/.claude/hooks/ways/documentation/adr/adr-tool" docs/scripts/adr
chmod +x docs/scripts/adr
# Snapshot the two records the scenario migrates, to check nothing is invented.
mkdir -p "$HOME/.migrate-before"
cp docs/architecture/system/ADR-179-*.md docs/architecture/system/ADR-186-*.md "$HOME/.migrate-before/"
git add -A && git commit -qm "corpus"
