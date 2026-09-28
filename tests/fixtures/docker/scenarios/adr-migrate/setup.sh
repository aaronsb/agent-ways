# adr-migrate setup: a copy of agent-ways' own record corpus, on the adr/v1
# contract it declares, with the v1-capable tool vendored. The rehearsal for
# #566: a real agent migrates real records before anyone does it for real.
APP="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
mkdir -p docs/scripts
cp -r "$APP/docs/architecture" docs/
cp "$HOME/.claude/hooks/ways/documentation/adr/adr-tool" docs/scripts/adr
chmod +x docs/scripts/adr
# The records under test start from their v0 form, snapshotted beside this
# script from main before #581 migrated them. The release flavor clones with
# --depth 1, so the history to restore them from is not there.
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# Each replaces the record where the corpus keeps it: system/ before the
# intent folders (ADR-310), platform/ after.
for f in "$here"/v0/ADR-*.md; do
  base=$(basename "$f")
  live=$(ls docs/architecture/*/"$base" 2>/dev/null | head -1)
  cp "$f" "${live:-docs/architecture/platform/$base}"
done
if grep -q '^contract:' docs/architecture/*/ADR-179-*.md docs/architecture/*/ADR-186-*.md; then
  echo "setup: the records under test are already v1; the rehearsal would test nothing" >&2
  exit 1
fi
# Snapshot the two records the scenario migrates, to check nothing is invented.
mkdir -p "$HOME/.migrate-before"
cp docs/architecture/*/ADR-179-*.md docs/architecture/*/ADR-186-*.md "$HOME/.migrate-before/"
git add -A && git commit -qm "corpus"
