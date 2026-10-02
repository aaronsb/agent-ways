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
  and Nerd Font icons, come from CaskaydiaMono Nerd Font Mono. Both are found
  with `fc-match`, and no font is bundled. If fontconfig finds no font, shots
  still show colours and attributes but no glyphs.
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
example.

## Notes

- tmux is started under `setsid -f`. Without it, a tmux server that this
  harness starts is reaped when the spawning shell returns. Where `setsid` is
  missing, as on macOS, tmux runs directly.
- Sessions run on your default tmux server as `tui-<name>`, so
  `tmux attach -t tui-<name>` works. The harness turns `pane-border-status` off
  for its window so that a border line in your tmux config does not take a row
  from the pane.
- Only SGR is interpreted: 16, 256 and truecolor fg and bg, bold, dim, italic,
  underline and reverse. Wide characters take two cells and zero-width
  characters are dropped. Sixel, OSC hyperlinks and complex shaping are not
  rendered.
- Cell metrics follow the original `render.py` (PIL on FreeType), so both give
  images of the same geometry from the same capture. Glyphs are unhinted, so
  strokes come out a little heavier. Unlike `render.py`, the capture's final
  newline does not add a blank row, and a shot is padded to the session's row
  count.
