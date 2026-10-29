---
contract: adr/v1
kind: decision
verb: add
capability: install
amends:
  - ADR-406#Decision
  - ADR-512#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "I think we need to detect our image display model and put it in a frame too. (preserving the title, resolution, scale and link to the file"
    via: "chat in the agent-ways session, 2026-10-29, while ways-theme's frames were being fitted; relayed verbatim by the calling session"
  - operator: aaronsb
    level: guided
    said: "should we ship the images mod with ways?"
    via: "chat in the agent-ways session, 2026-10-29, asked after ways-theme could not frame the images mod's pictures; relayed verbatim by the calling session"
  - operator: aaronsb
    level: directed
    said: "lets do it"
    via: "chat in the agent-ways session, 2026-10-29, in answer to the calling session's recommendation, written by the agent: ship images as a fourth mod, not critical, installed by default on the terminal, with the Linux-only pixel probe, the terminal matrix and its use without ways named as cautions; relayed verbatim by the calling session"
  - evidence: "the images mod at aaronsb/dotfiles 1b8fe15, `claude/mods/images`: plugin.json at 0.1.0 with five `userConfig` options (`renderer`, `showPathsInReplies`, `steerToSendUserFile`, `scale`, `showCaptions`); every render hook returns the engine's element off the terminal surface (hooks/register.tsx:521-598); the cell-size probe runs `python3` over `/proc/<pid>/fd` and `/proc/<pid>/stat` (hooks/lib.ts:145-173), and a terminal that reports no pixels, or a host without python3, is taken as 10x20-pixel cells (README); ImageMagick decodes every picture, and without it each site shows Claude Code's own row and one dim note (README); verified in kitty with Claude Code 2.1.304 (README)"
  - evidence: "aaronsb/dotfiles history: four commits touch `claude/mods/images` (83681f1, 7271cba, 8096185, 1b8fe15, all 2026-10-20); 83681f1 also carries the kitty and shell image tooling, which stays in the dotfiles (`zsh/.zsh/conf.d/72-images`). `claude/settings.d/70-mods.json` declares the `dotfiles` marketplace through `extraKnownMarketplaces` and enables `images@dotfiles`; that marketplace lists images alone (`claude/mods/.claude-plugin/marketplace.json`); `.dotfiles-manifest.toml` holds its `claude-mods` entry and `tests/run` its suite. The operator's `~/.claude/settings.json` holds no `pluginConfigs` entry for images"
  - evidence: "the images README and the dotfiles manifest: a plugin enabled from a marketplace declared in settings.json is absent from `claude plugin list` until a session has recorded that marketplace"
  - evidence: "the code at 968c2406: `Mod` with `critical`, and `MODS: [Mod; 3] = [ATTEND, JUDGE, THEME]` (tools/ways-cli/src/cmd/attend_mod.rs:50-100); `plan_converge` installs nothing beside a plugin of the mod's name from another marketplace and reports it (attend_mod.rs:274-282); the ways-judge and ways-theme plugins are held to ways' version by scripts/check-plugins.sh and moved by `WAYS_MANIFESTS` in scripts/release.sh:65; check-plugins.sh requires of every mod with hooks a test that runs its `register()` through `registrationsOf` and `passedOverHits`, and a copy of attend's passed-over list, which the images tests do not have; `mods.ways-theme.install` defaults false (tools/ways-core/src/settings.rs:560-570)"
  - upstream: "Claude Code's mod declarations as written by 2.1.304 carry `Raster`, `Image` and `$.ui.blit` on the terminal surface (the engine-written `.claude-plugin/types/claude-code/index.d.ts` of the images mod, headed \"Written by Claude Code 2.1.304\"); `claude plugin test [dir]` runs the tests of one mod's folder"
  - precedent: ADR-406
  - precedent: ADR-512
  - precedent: ADR-514
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-29
deciders:
  - aaronsb
related: [ADR-184, ADR-506, ADR-511, ADR-708]
observable:
  - "run: `git log --oneline -- plugins/images` lists the four commits from the dotfiles and the merge that brought them in, and `git blame plugins/images/hooks/lib.ts` attributes its lines to those commits"
  - "run: `make test-plugins` validates and tests `plugins/images`, holds its version to ways', and runs the frame contract with both mods' code"
  - "run: on Linux, `ways manage mod list` shows images wanted by default; on darwin it shows it not wanted until the operator installs it"
  - "run: on a host with `images@dotfiles` installed, `ways manage repair` reports it from another marketplace and plans no install of `images@agent-ways`; after the dotfiles stop shipping it and it is uninstalled, `ways manage repair --commit` installs `images@agent-ways`"
  - "try: with a session open, `ways settings set mods.images.scale 2`; the next picture drawn in that session is twice the size, with no restart"
  - "try: under Miami Nights with ways-theme installed, ask for an image; the picture sits in ways-theme's frame with its caption's linked name, pixel size and percentage under it, inside the frame"
  - "run: with no ImageMagick on PATH, `ways manage mod check images` prints one ⚠ line naming it and exits 3"
---

# ADR-517: The images mod ships from agent-ways as a fourth mod, installed by default, its history moved from the operator's dotfiles

## Summary

- **Decided:** The `images` mod, which draws pictures inline in the terminal transcript, ships from agent-ways in the `agent-ways` marketplace beside attend, ways-judge and ways-theme. Its source moves to `plugins/images/` with its four commits, rewritten by `git filter-repo` from the dotfiles path and merged with `--allow-unrelated-histories`. It joins `MODS` as `images@agent-ways`, not critical, at Claude Code 2.1.304, and `mods.images.install` defaults to true on Linux. On darwin it defaults to false until a macOS cell-size probe has been verified on a Mac. Its version moves with ways', and its tests join `make test-plugins`. Its five options become ways settings under `mods.images.*`, and the plugin drops its `userConfig`. ways-theme frames the pictures images draws, and the contract between the two is tested in this repo with both mods' code loaded. The dotfiles stop shipping `images@dotfiles` after agent-ways ships images; until then repair reports the dotfiles copy and installs nothing beside it.
- **Trades away:** The mod's own release cadence: a change to it waits for a ways release, and its version jumps from 0.1.0 to ways'. ways takes on its terminal matrix (kitty, Ghostty, Konsole, iTerm2, tmux, ssh) and its ImageMagick dependency as its own to support. Its options leave Claude Code's `/config`, and a host that runs it without ways gets its defaults. A Mac gets no pictures until someone installs the mod there. Every Linux session gains a mod that draws in the transcript unless the operator turns it off. The imported commits get new hashes and lose any signature.
- **One-way?** No, apart from history. Withdrawing the mod is `mods.images.install: false`, and moving its folder back to the dotfiles is a session's work. The four imported commits and their merge stay in main's history once merged, since main is not rewritten.
- **Probes:** *Confident (one place):* images' options, its scale, captions, bare paths in replies, renderer and the SendUserFile hint, are set in ways settings beside ways-theme's, and Claude Code's `/config` no longer lists them. *Not confident (Mac):* on a Mac, images stays off until it has been run on one and its picture sizing checked there, and `ways manage mod install images` turns it on before then. The other choice is to have it on for Macs too, drawing every picture at a guessed cell size.
- **Inversion:** At one end every mod lives where it was written, and the mods that depend on each other agree on a contract across repositories and test suites. At the other end agent-ways ships every mod its operator uses. This record moves a mod into agent-ways when another agent-ways mod has to recognise what it draws, and leaves other mods where they are.

## Context

ways-theme (ADR-514) frames replies, tool rows and tool groups on the terminal surface. The images mod draws pictures under the rows that would otherwise show only a path, and lives in the operator's dotfiles, installed as `images@dotfiles` from a settings-declared marketplace. Framing its pictures, with their caption kept, needs ways-theme to recognise the element images draws. Held across two repositories, that contract would be tested in neither: `claude plugin test` runs one mod's folder, and ways-theme's tests cannot load a mod this repo does not hold.

The plumbing ADR-406 and ADR-512 built takes another mod as a table entry, a settings section and a marketplace entry: repair installs and converges it per target, `ways manage mod` and the settings screens' mod tab manage it, and the plugin checks hold its version and tests. The images mod measures its terminal's cell size through `/proc`, which exists on Linux only, and ways ships darwin builds.

## Decision

1. **A mod another shipped mod depends on ships from agent-ways.** When an agent-ways mod has to recognise or wrap what another mod draws, the other mod ships from the `agent-ways` marketplace, so the contract between them lives in one repository and one test run. images is the first such mod. A mod with no such dependency stays where it was written.

2. **The move keeps the history.** In a fresh clone of the dotfiles, `git filter-repo --path claude/mods/images/ --path-rename claude/mods/images/:plugins/images/` leaves the four commits that touched the mod, each holding only the mod's files at their new paths. That branch is fetched into agent-ways and merged with `git merge --allow-unrelated-histories`; the merge commit lists the dotfiles' original hashes from filter-repo's commit map. The rewrite keeps each commit's author, date and message. 83681f1's message describes the kitty tooling the dotfiles keep, and is left as written. `git subtree add` would also keep the commits, with their files at the root of the mod's folder, so `git log -- plugins/images` and `git blame` would stop at the merge; with filter-repo the files sit at their final paths in every commit. filter-repo runs only in a scratch clone and never in the operator's dotfiles checkout.

3. **The mod in this repo.** The commits after the merge:
   - add images to `plugins/.claude-plugin/marketplace.json` with its plugin description;
   - set its version to ways', add it to the ways loop in `scripts/check-plugins.sh` and to `WAYS_MANIFESTS` in `scripts/release.sh`, so `scripts/release.sh bump ways` moves it;
   - add the test check-plugins.sh requires of a mod with hooks, its `register()` run through `registrationsOf` and checked with `passedOverHits`, and the copy of attend's passed-over list, so `make test-plugins` runs its tests;
   - rewrite the README's Install section for agent-ways.

4. **The registry entry.** `MODS` gains `IMAGES: Mod { name: "images", id: "images@agent-ways", label: "images mod", min_claude_code: "2.1.304", critical: false }`. The floor is the release whose engine-written declarations carry every call the mod makes (`Image`, `Raster`, `$.ui.blit`, `onScreen`), the one it was verified on. Not critical: a missing or pending images mod degrades nothing agent-ways does. The settings registry gains `mods.images`, with `install` a machine-scope key read per target as ADR-512 item 1 sets for every mod. Its default is true on Linux and false on darwin (item 7). The mod draws on the terminal surface only and hands every other surface Claude Code's own element, as it does today.

5. **Its options are ways settings.** `mods.images.renderer` (`auto`, `image`, `halfblock`; default `auto`), `scale` (0.25 to 4; default 1), `show_captions`, `show_paths_in_replies` and `steer_to_send_user_file` (each default true) take the meanings the plugin's options have. They are set at user, target or project scope, the project winning, as ways-theme's render settings are (ADR-514 item 11), and edited in the mod tab or with `ways settings set`. The plugin drops its `userConfig`, so Claude Code's `/config` offers none of them.
   - The mod reads `ways settings list mods.images --json --effective` at an interactive session's start, and again before it draws a picture when the last read is more than 5 s old. It runs no poll of its own, so a session that draws nothing calls `ways` once.
   - Each read is bounded at 1.5 s. A missing or failing `ways`, or output that does not parse, keeps the last good values, or the defaults before there are any, and writes one debug line per distinct reason (ADR-514 items 7 and 10).
   - `ways manage mod check images` adds one check, ImageMagick on PATH, and a missing one is a ⚠ line naming it, by ADR-512 item 5's exit rule.

6. **The frame contract.** While `custom:ways` is selected, ways-theme frames each picture images draws, with its caption, as one framed row; outside that it leaves them as images draws them. The contract:
   - images marks the element it draws for a picture and its caption so ways-theme can find it without reading pixels;
   - a picture is sized to the width it is drawn in, so the frame's four columns come out of the picture's width;
   - the caption keeps the file's name with its link, its size in pixels and the percentage it is shown at, inside the frame.

   The contract is written once, in this repo, and `make test-plugins` checks it with both mods' code: ways-theme's tests load images' own `register()` from `plugins/images`, or, where the test host confines a test to its mod's folder, each mod's tests hold one fixture of the contract, kept to one text by check-plugins.sh as it keeps the passed-over list. The framing itself is the ways-theme change in flight on `fix/theme-frames-images`.

7. **Darwin.** The `/proc` probe does not run off Linux, and a Mac's `/usr/bin/python3` without the Command Line Tools opens an install dialog. Until a macOS path is verified on a Mac, the mod takes 10x20-pixel cells on darwin without running `python3`, and `mods.images.install` defaults to false there. The macOS path finds Claude Code's terminal without `/proc`, from the process's tty and its ancestors', and runs no interpreter stub that prompts. Once it has been run on a Mac in kitty or Ghostty and in iTerm2, the darwin default becomes true in the same change. An operator who installs it on a Mac before then gets pictures at the 10x20 size, which `scale` corrects.

8. **The switch from the dotfiles.** In this order:
   1. agent-ways ships images (items 2 to 7). On a host where `images@dotfiles` is installed, repair reports it as a plugin of the mod's name from another marketplace and installs nothing beside it, as attend_mod.rs does for every mod. Repair also counts as foreign an `images@<other>` that settings.json's `enabledPlugins` enables, since a plugin enabled from a settings-declared marketplace is absent from `claude plugin list` until a session has recorded that marketplace. The rule holds for every mod. So no host draws each picture twice.
   2. The dotfiles change removes the mod, the `dotfiles` marketplace (images was its one plugin), `claude/settings.d/70-mods.json`, the `claude-mods` manifest entry and its `tests/run` suite, so `dotfiles deploy` leaves `images@dotfiles` out of settings.json. Where a session recorded it, `claude plugin uninstall images@dotfiles` and `claude plugin marketplace remove dotfiles` take it out of Claude Code.
   3. `ways manage repair --commit`, or the next `ways manage update --commit`, installs `images@agent-ways`. The operator's settings hold no images options today, so nothing carries over.

   The gap between steps 2 and 3 is one command, and in it pictures show as paths.

9. **Under the item descriptor.** If the item descriptor proposed in PR #1206 is accepted, images is a `builtin` mod item like the other three, with nothing particular to it.

This amends ADR-406's Decision, item 8, whose marketplace now ships four mods, images among those released with ways; and ADR-512's Decision, item 1, whose `mods.<name>` sections gain `mods.images`, with item 3's foreign-plugin case widened to a plugin enabled in settings.json.

## Consequences

### Positive

- ways-theme and images are changed, tested and released together, so a change to what images draws cannot break the frame unseen.
- images gets repair's install, update, floor check and per-target state, and `ways manage mod` and the mod tab manage it, with no code of its own for any of it.
- Its options sit where the other mods' settings sit, and a change reaches an open session at its next picture.
- The mod's reasoning stays readable in this repo's log and blame.
- A plugin enabled from a settings-declared marketplace is now seen as foreign for every mod, closing a window in which repair could install a second copy.

### Negative

- Reports from every terminal images runs in become agent-ways issues, tmux and ssh among them, and the mod's behaviour differs across them (pixels in kitty and Ghostty, half-blocks elsewhere, the PNG's bytes over ssh).
- A Linux host without ImageMagick shows a dim note at each picture until it is installed; `mod check` names it, and the installer installs nothing for it.
- A Mac gets no pictures by default until the macOS probe is verified, and someone has to run it on a Mac to get there.
- The mod's version tracks ways', so a ways release bumps it with nothing changed in it, and a fix to it waits for a ways release.
- A host that runs images without agent-ways can no longer set its options; it gets the defaults.
- The frame contract adds a test that spans two mods, which check-plugins.sh has not had before.
- In the gap of item 8 step 2, until repair runs, the operator's sessions show paths where pictures were.

### Neutral

- The dotfiles keep the shell image tooling (`72-images`, `ls-thumbs`, the kitty aliases); only the Claude Code mod moves.
- The mod's cache directory, its render log flag and its ssh handling are unchanged.
- `claude --plugin-dir plugins/images` stays the way to load a working copy, as the README's Developing section already says.

## Alternatives Considered

- **Keep images in the dotfiles and have ways-theme recognise it across repositories.** No move. Rejected: the contract would live in two repositories and be tested in neither, and a change in the dotfiles could break the frame with no failing test here.
- **Give images a repository and marketplace of its own.** It stays usable without agent-ways and keeps its own releases. Rejected: the contract with ways-theme still spans two repositories, and a second marketplace adds a second release pipeline and install path for one mod.
- **Copy the files in one commit.** The simplest move. Rejected: the four commits carry the reasons behind the sizing, the burst handling and the captions, and blame would point at the import.
- **`git subtree add` from a `git subtree split` of the dotfiles path.** No extra tool, and the commits keep their original trees. Rejected for item 2's reason: path-limited log and blame stop at the merge.
- **Off by default everywhere, as ways-theme is.** ways-theme replaces the person's Claude Code theme, which is why it waits to be asked for. images replaces nothing: it draws under rows that show only a path, and does nothing off the terminal, so it is on by default where it has been verified.
- **On by default on darwin with the 10x20 fallback.** Pictures on a Mac from the first session. Not chosen for now: no one has run the mod on a Mac, pictures there would be sized by a guess, and most Macs lack ImageMagick, so many would see dim notes instead. Item 7's verification makes it the default.
- **Keep the options in the plugin's `userConfig`.** The mod keeps working without ways, and Claude Code's `/config` edits them. Rejected: ADR-514 put every mod setting in ways settings, with no menu in Claude Code, and two homes for one mod's settings would leave the mod tab unable to show them.
- **Critical, as attend and ways-judge are.** Rejected: agent-ways loses nothing when images is missing, and a critical flag would mark a host degraded for a missing picture renderer.
