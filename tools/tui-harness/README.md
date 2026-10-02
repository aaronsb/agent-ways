# tui-harness

Run a TUI or a coloured CLI in a detached terminal, send it keys, and look at
what it renders as a PNG. No X server or GPU is involved. tmux owns the PTY,
the input and the screen buffer, and a small renderer turns
`tmux capture-pane -ep` output into an image.

This is a dev tool. It is a workspace crate, but it is not in `suite-bins` and
not in any release. It is a Rust port of the operator's `opentui-test` shell
harness (#722).

## Commands

```
tui-harness launch <name> [--cols N] [--rows M] [--font NAME] [--size PT] -- <cmd...>
tui-harness send   <name> [-l|-H|-N n]... <keys...>   # tmux send-keys keys: j Enter C-c, or -l "text"
tui-harness shot   <name> [--out PATH]   # prints the PNG path
tui-harness text   <name> [--ansi]       # pane contents; --ansi keeps SGR escapes
tui-harness attach <name>                # look in from your terminal; detach with C-b d
tui-harness down   <name>                # kills it only if this root launched it
tui-harness down   --all                 # every session of this root, its orphans and stale state
tui-harness prune  [--untagged]          # kill this root's orphans; drop dead and stale state
tui-harness ls                           # STATE: up, gone, foreign, orphan, launching, stale, untagged
tui-harness render --out PATH [--in FILE] [--cols N] [--rows M] [--font NAME] [--size PT]
```

Every command takes `--dir PATH` to set the state root. `render` turns a saved
`capture-pane -ep` dump (a file, or stdin) into a PNG without a session.

```bash
cargo build --manifest-path tools/Cargo.toml -p tui-harness
th=tools/target/debug/tui-harness
$th launch spike -- cargo run -q --manifest-path tools/spikes/settings-tui/Cargo.toml
$th send spike Tab
$th shot spike
$th down spike
```

A command that prints and exits closes its session at once. To shoot one, keep
the pane open: `-- sh -c 'ways list; sleep 600'`.

The command runs with the environment and working directory of the shell that
ran `launch`, whoever started the tmux server. A key or text ending in `;`
(`send app ';'`, `send app -l 'a;'`) is typed as is. Only the leading flags
`-l`, `-H` and `-N <count>` reach `send-keys` as flags; everything after them
is keys, so text can start with `-` and nothing can retarget the command.

`send`, `text` and `shot` act only on a session this root launched. Each
session is tagged with its root, and a session of another root, or an untagged
one, is refused.

In `ls`, `foreign` is a name this root has state for but another root's
session runs under, `orphan` is this root's session with no state left,
`launching` is a launch still under way, `stale` is state from an interrupted
launch (its environment file may still be on disk), and `untagged` is a
`tui-*` session no root tagged. `prune` and `down --all` never kill another
root's session or an untagged one, and leave a launch in progress alone;
`prune --untagged` kills untagged ones on request. A launch whose state is
removed while it runs (by an explicit `down` of its name) fails.

A command given as one word runs directly when it is a plain program name or
path, so a missing program fails the launch; a one-word shell string
(`'make test; read'`) runs through `$SHELL -c`, as tmux runs one.

## Defaults

- The geometry is 200x50.
- The font is JetBrains Mono at 14 px per em. Glyphs it lacks, such as Braille
  and Nerd Font icons, come from CaskaydiaMono Nerd Font Mono. A glyph that
  neither has, such as Chinese, Japanese or Korean, comes from a CJK font:
  Noto Sans Mono CJK SC, then Noto Sans CJK SC, then any font fontconfig lists
  for `:lang=zh`. With no CJK font it is drawn as the fallback's missing-glyph
  box. Fonts are found with `fc-match` and `fc-list`, and none is bundled. If
  fontconfig finds no font, shots still show colours and attributes but no
  glyphs.
- State lives in `$XDG_STATE_HOME/agent-ways/tui-harness/`, with
  `XDG_STATE_HOME` defaulting to `~/.local/state`. Sessions are under
  `sessions/<name>/env` and shots under
  `shots/<name>-<UTC timestamp with milliseconds>.png`.
- `--cols` and `--rows` go up to 10000, tmux's limit, and `--size` up to 512.
- A font that is not installed is replaced by whatever fontconfig picks, and
  `launch`, `shot` and `render` print a warning saying so.

## From a test

```rust
use std::time::Duration;
use tui_harness::{tmux_available, Harness, LaunchOptions, Renderer};

if !tmux_available() {
    eprintln!("SKIPPED: tmux is not installed");
    return;
}
let h = Harness::new(std::env::temp_dir().join("my-test"));
// The default env and cwd are the test process's; set `env` or `cwd` to
// point the command at a fixture HOME or config directory.
let s = h.launch("demo", &LaunchOptions { cols: 80, rows: 24, ..Default::default() }, &cmd)?;
s.wait_for("ready", Duration::from_secs(10))?;
s.send(&["j", "Enter"])?;
let text = s.text(false)?;
let img = s.capture_image_with(&Renderer::without_fonts(8, 16))?; // fixed cells, no fonts
s.down()?;
```

`launch` starts the command through the `tui-harness` binary (its hidden
`__exec-env` subcommand), so the binary must exist. From another crate's
tests, run `cargo build -p tui-harness` first; the library finds the binary
beside the test executable or on `PATH`. `TUI_HARNESS_BIN` or
`LaunchOptions::helper` names it explicitly.

`Renderer::without_fonts` gives fixed-size cells and draws no glyphs, so pixel
checks on background colours hold on any machine. `Renderer` is `Send` and
`Sync`. `tests/drive.rs` is a worked example; its `common` module has a `Drop`
guard that kills a test's sessions and removes its root even when the test
panics. CI installs tmux and sets `TUI_HARNESS_REQUIRE_TMUX=1`, which turns
those tests' skip into a failure.

## The swatch: the regression target

`tests/fixtures/swatch.sh` prints one row for each thing the renderer must
draw: attributes, the 16 basic colours, the 256-colour cube, the grey ramp, a
truecolor gradient, fg/bg pairs, reverse and dim, Braille, box drawing, Nerd
Font icons, and wide CJK text. `swatch.ansi` is its capture through tmux at
100x14, so `tests/swatch.rs` needs no tmux:

- **Exact colours.** The capture is rendered with `without_fonts`, and the exact
  RGB of named cells is asserted, one test per mode (`cube256_red_196`,
  `reverse_swaps_fg_and_bg`, and so on). These run everywhere, CI included.
- **Golden image.** A render with the fonts is compared against
  `swatch.golden.png`. A pixel counts as different when a channel is off by
  more than 32 levels. The test fails when more than 1% of pixels differ,
  which is the drift that hinting and font versions produce. It runs only
  when the renderer gets the fonts the golden was recorded with: JetBrains
  Mono, CaskaydiaMono Nerd Font Mono, and Noto Sans Mono CJK SC as the CJK
  face it actually picks. Otherwise it skips and names what is missing, or
  fails when `TUI_HARNESS_REQUIRE_FONTS=1`. The bound is loose for small
  regions: a broken reverse moved too few pixels to trip it, and the
  exact-colour tests catch that case.

To re-record after an intended change, re-capture `swatch.ansi` (the commands
are in `swatch.sh`), then run
`TUI_HARNESS_BLESS=1 cargo test -p tui-harness --test swatch`. That run writes
`swatch.golden.new.png` and fails. Review the image, then move it over
`swatch.golden.png` by hand. A bless on a machine without the recorded fonts
fails rather than skips.

## Notes

- tmux is started under `setsid -f`. Without it, a tmux server that this
  harness starts is reaped when the spawning shell returns. Where `setsid` is
  missing, as on macOS, tmux runs directly.
- Sessions run as `tui-<name>` on a private tmux server, socket
  `agent-ways-tui`. Every tmux call passes `-f /dev/null`, so whichever call
  starts that server, your `~/.tmux.conf` and its plugins (a session restorer,
  say) never load, and never change what a test sees. Its options are set
  explicitly: no status line, no pane border status, `history-limit 50000`,
  `default-terminal tmux-256color`, and `COLORTERM=truecolor` in the
  environment of the apps whose caller set none. To look in, use
  `tui-harness attach <name>` or `tmux -L agent-ways-tui attach -t tui-<name>`.
- Every root shares that server and the `tui-<name>` names, so each session is
  tagged with the root that launched it (the `@tui_harness_root` option).
  `down` kills a session only when the tag is its own root; otherwise it
  removes its own stale state and leaves the session running. A launch that
  times out or whose command cannot start kills what it started and removes
  its state. A root path holding a newline or tab is refused.
- The pane's process is `tui-harness __exec-env FILE -- CMD...`. The file, in
  the session's state directory and created `0600` with `O_EXCL`, holds the
  working directory and the environment as NUL-separated records. The helper
  deletes the file before anything else, builds the environment from nothing
  (plus tmux's `TERM`, `TMUX`, `TMUX_PANE`), changes directory, and execs the
  command. A name or value holding NUL, or a name that is empty or holds `=`,
  is refused at launch, since the file could not carry it exactly. No shell
  reads the file, so names bash treats as readonly (`UID`,
  `SHELLOPTS`) and a `PATH` without `rm` are fine, and the working directory
  never passes through tmux's format expansion. `launch` returns once the
  command has started, or fails with the helper's error. tmux's
  `new-session -e` is not used: a full environment overflows tmux's message
  size.
- The session's `env` metadata (its geometry, font and command line) is
  written `0600` before tmux starts, so an interrupted launch leaves state
  that `ls` shows and `prune` removes, along with any `environ` file older
  than a launch's timeout.
- Only SGR is interpreted: 16, 256 and truecolor fg and bg, bold, dim, italic,
  underline (including the `4:n` styles, drawn as a single line), reverse,
  strikethrough and conceal. The underline colour (`58`, `59`) is parsed and
  not drawn. A truncated extended colour ends its sequence, and a palette
  index above 255 is ignored. Wide characters take two cells and zero-width
  characters are dropped. A wide character in the last column would widen the
  image by a cell; tmux never puts one there. Sixel, OSC hyperlinks and
  complex shaping are not rendered.
- Cell metrics follow the original `render.py` (PIL on FreeType), so both give
  images of the same geometry from the same capture. Glyphs are unhinted, so
  strokes come out a little heavier. Unlike `render.py`, the capture's final
  newline does not add a blank row, and a shot is padded to the session's row
  count.
