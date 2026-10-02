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
tui-harness send   <name> <keys...>      # tmux send-keys passthrough: j Enter C-c, or -l "text"
tui-harness shot   <name> [--out PATH]   # prints the PNG path
tui-harness text   <name> [--ansi]       # pane contents; --ansi keeps SGR escapes
tui-harness attach <name>                # look in from your terminal; detach with C-b d
tui-harness down   <name>
tui-harness ls
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
  `sessions/<name>/env` and shots under `shots/<name>-<UTC timestamp>.png`.

## From a test

```rust
use std::time::Duration;
use tui_harness::{tmux_available, Harness, LaunchOptions, Renderer};

if !tmux_available() {
    eprintln!("SKIPPED: tmux is not installed");
    return;
}
let h = Harness::new(std::env::temp_dir().join("my-test"));
let s = h.launch("demo", &LaunchOptions { cols: 80, rows: 24, ..Default::default() }, &cmd)?;
s.wait_for("ready", Duration::from_secs(10))?;
s.send(&["j", "Enter"])?;
let text = s.text(false)?;
let img = s.capture_image_with(&Renderer::without_fonts(8, 16))?; // fixed cells, no fonts
s.down()?;
```

`Renderer::without_fonts` gives fixed-size cells and draws no glyphs, so pixel
checks on background colours hold on any machine. `tests/drive.rs` is a worked
example. CI installs tmux and sets `TUI_HARNESS_REQUIRE_TMUX=1`, which turns that
test's skip into a failure.

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
  when fontconfig has JetBrains Mono, CaskaydiaMono Nerd Font Mono and Noto
  Sans CJK SC; otherwise it skips and names the missing fonts. The bound is
  loose for small regions: a broken reverse moved too few pixels to trip it,
  and the exact-colour tests catch that case.

To re-record after an intended change, re-capture `swatch.ansi` (the commands
are in `swatch.sh`), then run
`TUI_HARNESS_BLESS=1 cargo test -p tui-harness --test swatch`. That run writes
`swatch.golden.new.png` and fails. Review the image, then move it over
`swatch.golden.png` by hand.

## Notes

- tmux is started under `setsid -f`. Without it, a tmux server that this
  harness starts is reaped when the spawning shell returns. Where `setsid` is
  missing, as on macOS, tmux runs directly.
- Sessions run as `tui-<name>` on a private tmux server, socket
  `agent-ways-tui`. That server starts with `-f /dev/null`, so your
  `~/.tmux.conf` never changes what a test sees. Its options are set
  explicitly: no status line, no pane border status, `history-limit 50000`,
  `default-terminal tmux-256color`, and `COLORTERM=truecolor` in the
  environment of the apps it runs. To look in, use `tui-harness attach <name>`
  or `tmux -L agent-ways-tui attach -t tui-<name>`.
- Only SGR is interpreted: 16, 256 and truecolor fg and bg, bold, dim, italic,
  underline and reverse. Wide characters take two cells and zero-width
  characters are dropped. Sixel, OSC hyperlinks and complex shaping are not
  rendered.
- Cell metrics follow the original `render.py` (PIL on FreeType), so both give
  images of the same geometry from the same capture. Glyphs are unhinted, so
  strokes come out a little heavier. Unlike `render.py`, the capture's final
  newline does not add a blank row, and a shot is padded to the session's row
  count.
