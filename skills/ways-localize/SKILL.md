---
name: ways-localize
description: Localize agent-ways into the operator's language — translate every way's matching metadata against the English root, tune it clean, and switch ways (and Claude Code) into that language. Use when the operator asks to run ways in their language ("set up ways in Spanish", "localiza ways al español", "ways auf Deutsch"). Not for authoring or editing individual ways (that is the ways skill), not for English installs (already built), and not for changing Claude Code's language alone (that is a settings.json edit).
allowed-tools: Bash, Read, Write, Edit
---

# ways-localize: Adopter-run localization

Turns an English-only ways install into a localized one (ADR-139). English is the
**source of truth**; a localization is a derivation, validated against the English
root. This skill is the operator-facing orchestrator for the lifecycle in
`docs/explanation/localization/` (scenario `01.011.E`) — interview, consent,
translate, tune, switch. The mechanics live in the design note
`docs/architecture/ways/ADR-183-single-language-localization-tuning-the-english-anchor-as-a-peer.md`; don't restate them.

The app source (the language registry, the embedder's Makefile, the docs) lives in
`$XDG_DATA_HOME/agent-ways`. `~/.claude` is a projection of it that does not carry
`tools/` (ADR-142). Resolve both once. The skill is global, so the working directory
is unknown at invocation:

```bash
APP="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"   # app source: tools/, docs/, bin/
ROOT="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"              # Claude Code config: settings.json
# Verify the app source is there before doing anything.
[ -f "$APP/tools/ways-core/languages.json" ] || { echo "No agent-ways app source at $APP. (Re-run the installer to (re)stage it.)"; exit 1; }
```

## 1. Interview — which language

Recognize the target from the operator's request, **in their own language** (the
operator is already reading and writing in it). Resolve it to a ways
**code** and a Claude Code **name** from the registry, and confirm with the operator
before proceeding:

```bash
jq -r '.languages | to_entries[] | "\(.key)\t\(.value.name)\t\(.value.active)"' \
  "$APP/tools/ways-core/languages.json"   # code  name  active
```

Use the code (e.g. `es`) for ways; the English name (e.g. `spanish`) for Claude Code.
If the language is `active: false` in the registry, say so — it can be enabled, but
that is a separate decision.

## 2. Consent — this is heavy, ask first

Localization downloads a 127 MB model and translates + tunes every way. State that
plainly and get a clear go-ahead before step 3. (The operator invoked the skill, but
the model download and the all-ways pass are the kind of cost that earns a confirm —
the same bar the delivery skills hold.)

## 3. Flip the mode switch

The **effective** switch is the user-scope `language` in
`$XDG_CONFIG_HOME/agent-ways/config.yaml` (`ways settings list --json` prints it). Set it to
the code:

```bash
CFG="${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/config.yaml"
mkdir -p "$(dirname "$CFG")"; touch "$CFG"
# replace an existing `language:` line, else append
grep -q '^language:' "$CFG" && sed -i "s/^language:.*/language: $CODE/" "$CFG" || printf 'language: %s\n' "$CODE" >> "$CFG"
"$APP/bin/ways" tune language --json | jq -r '.resolved_language'   # confirm it resolves to $CODE
```

## 4. Fetch the multilingual model (on-demand)

```bash
make -C "$APP/tools/way-embed" model-multilingual   # 127 MB, only when localizing
```

## 5. Translate every way against the English root

For each way, translate its frontmatter `description` + `vocabulary` into the target
language — **faithfully to the English meaning** (it is the root; you are deriving,
not reinventing). Write one `.locales.jsonl` line per way beside its `.md`:

```
{"lang":"<code>","description":"<translated>","vocabulary":"<translated keywords>"}
```

This is many small, independent units across ~all ways — a good **Workflow** fan-out
when the count is large (translate + tune per way in parallel); batch inline for a
handful. Vocabulary carries the *objective match words* in local form, so translate
intent, not word-for-word. (`.{lang}.md` stubs + `pack-locales.sh` are the alternate
path if you prefer per-file stubs; the script is
`$APP/tools/ways-cli/scripts/pack-locales.sh`.)

## 6. Build the corpus, then tune as the acceptance gate

```bash
"$APP/bin/ways" corpus --quiet               # localized mode → multi corpus + English anchor
"$APP/bin/ways" tune locale --lang "$CODE"          # root-anchored fidelity + discrimination
```

`ways tune locale` is the **objective gate**: fidelity = alignment to the English root,
discrimination = no collision with another way. Re-author flagged stubs, rebuild,
re-tune **until clean**. Do not declare done while entries are flagged — show the
clean `ways tune locale` output as the evidence.

## 7. Switch Claude Code's own language

```bash
# settings.json takes the NAME, not the code; effective next session / after /clear
jq --arg L "$NAME" '.language = $L' "$ROOT/settings.json" > "$ROOT/settings.json.tmp" \
  && mv "$ROOT/settings.json.tmp" "$ROOT/settings.json"
```

## 8. Report — in the operator's language

Summarize in the **target language**: how many ways localized, that `ways tune locale` is
clean, and that Claude Code's language takes effect next session.

## Not for

- Authoring or revising a single way — that is the **ways** skill.
- English installs — nothing to do; the intl pipeline is dormant by design.
- Setting only Claude Code's response language — that is a one-line `settings.json`
  edit, not a localization.

## See also

- `$APP/docs/explanation/localization/` (`01.009.E`–`01.013.E`) — the scenarios and the mode gate
- `$APP/docs/architecture/ways/ADR-183-single-language-localization-tuning-the-english-anchor-as-a-peer.md` — the mechanics
- the **ways** skill — authoring the English roots this skill derives from
