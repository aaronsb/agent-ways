# images

A Claude Code mod that draws pictures inline in the terminal transcript, under the row that would otherwise show only a path.

## What it draws

- **SendUserFile**: the image attachments Claude sends you.
- **Read**: an image Claude opens (PNG, JPEG, GIF, WebP), drawn from the bytes the model was shown, under the Read row or under the folded "Read N files" line.
- **Images a reply names**: a markdown image `![alt](/abs/path.png)` or `![alt](~/path.png)`, or a bare absolute or `~/` path such as "saved the chart to /tmp/chart.png" or "`~/Pictures/x.jpg`". Bare paths count in prose, in inline code and in quotes, with trailing punctuation dropped. Fenced code blocks are skipped, and relative paths and URLs are ignored. A path that names no file draws nothing. Each file draws once, in the order the reply names it, up to 4 per reply.
- **Pasted images**: the images pasted with a prompt, drawn under that prompt.

A GIF draws its first frame. Other surfaces (desktop, VS Code, mobile) are left to Claude Code.

## Size and caption

A picture shows at its own size, one screen pixel per image pixel, times the `scale` option. On Linux the mod learns the terminal's cell size in pixels by asking the kernel for the window size of Claude Code's terminal (`TIOCGWINSZ`, through `python3` over `/proc`), measured again when the transcript width changes, as it does after a font size change. A terminal that reports no pixels (tmux, some others), or a machine without `python3`, is taken to have 10x20-pixel cells. On any other host, a Mac among them, no probe runs and cells are taken as 10x20 pixels, so a picture there can come out too large or too small for the real cells; `scale` corrects it. Half-blocks draw one column per image pixel, times `scale`.

The size is then held to the width the picture is drawn in and 60% of the transcript's height, with the aspect kept, and a picture narrower than 24 columns is enlarged to 24, so icons and thumbnails stay legible. An enlargement of 1.25 times or more is made nearest-neighbour, each image pixel a solid block; a smaller one, from rounding to whole cells, is left to the terminal to blend.

Under each picture a caption gives the file's name, its size in pixels, and how large it is shown against its own size: `chart.png · 1920×1080 · 83%`. The name links to the file (`file://`, OSC 8), where the picture has one; a pasted image has none.

## In ways-theme's frame

Each picture and its caption sit in one Box keyed `images-picture-<n>`. Under ways-theme with frames on, a row that carries a picture is framed with the picture and caption inside the frame. Where the row the mod is handed is already the theme's frame, the mod puts the pictures inside it, sized to the room within its border and padding. Where the mod draws beneath the theme, the theme narrows the picture to the frame's room and moves the caption's percentage with it. `tests/picture-frame.ts` holds the contract, one copy in each mod's tests (ADR-517).

## Pixels or half-blocks

- **Pixels**: in a terminal where Claude Code draws kitty graphics with Unicode placeholders (kitty and Ghostty, or any terminal once `CLAUDE_CODE_FORCE_TERMINAL_IMAGES=1` turns Claude Code's detection on), the mod uses Claude Code's `Image` element.
- **Half-blocks**: everywhere else, Konsole and iTerm2 among them, `Image` would draw only its alt text. The mod draws a `Raster` of `▀` cells instead: each cell's foreground is the upper pixel and its background the lower one, so a cell holds two pixels.

An `Image` reads its picture from the resized PNG in the cache directory; the terminal opens the file itself. Verified in kitty with Claude Code 2.1.304, installed, on this version: the whole scrollback keeps its pictures, including ones scrolled out of view and back. Claude Code frees an `Image`'s pixels when its row leaves the view; the file source is the likely reason they come back, since the terminal can read a file again when the row is drawn again and a bytes source gives it nothing to read. The mod also raises the source's `generation` when a row is reported off screen and then back (`onScreen` `null`, then set), but `onScreen` stayed absent on every render throughout that check, so that step did not fire; it is kept because it is harmless. When Claude Code runs over ssh (`SSH_CONNECTION` or `SSH_TTY` set), the terminal cannot read this machine's files, so the PNG's bytes go in the element instead, up to 2 MB; a larger picture draws as half-blocks.

The mod does not keep a list of terminal names. In `auto` it asks Claude Code: it draws the first picture of a session as half-blocks with a 1x1 transparent `Image` beside it, then calls `$.ui.blit` on that `Image`. Claude Code refuses a blit to an `Image` that is drawing its alt, so a refusal means half-blocks and an accepted blit means pixels. The answer holds for the rest of the session.

## Options

The options are ways settings under `mods.images`, set at user, target or project scope, the project winning, in the settings screens' mod tab or with `ways settings set`, as in `ways settings set mods.images.scale 2`. The mod reads `ways settings list mods.images --json --effective` at an interactive session's start, and again before it draws a picture when its last read is over 5 seconds old, so a change reaches the next picture with no restart. A missing or failing `ways` keeps the last options read, or the defaults, and writes one line to the debug log per reason. Claude Code's `/config` lists none of them.

`show_paths_in_replies` (default `true`): draw images a reply names by a bare path. Off, a reply draws only its markdown images.

`steer_to_send_user_file` (default `true`): add a short section to the system prompt, on the terminal surface only, saying the terminal draws images and that a request to see one is answered with SendUserFile, which also shows it in remote and web views. Where the request does not offer SendUserFile, the section names Read and absolute paths instead. Off, the prompt is left as Claude Code composes it. Where an organization manages Claude Code, its policy plugin passes over the hook that adds the section.

`scale` (default `1`, from `0.25` to `4`): picture size against one screen pixel per image pixel. `2` doubles every picture, `0.5` halves it; the width and height limits and the 24-column floor still apply.

`show_captions` (default `true`): the line under each picture with its linked file name, pixel size and shown percentage.

`renderer`:

| Value | Effect |
|---|---|
| `auto` (default) | Pixels where Claude Code draws kitty graphics, half-blocks elsewhere |
| `image` | Always the `Image` element. Shows only the alt text where the terminal cannot draw it |
| `halfblock` | Always half-blocks |

## Requirements

ImageMagick, as `magick` (version 7) or `convert` (version 6), on `PATH`. It decodes and resizes every picture. Without it, each site shows Claude Code's own row and one dim note, and `ways manage mod check images` names it. The installer does not install it.

The mod keeps resized pictures, and pasted or Read images written as data URI files, in `$XDG_RUNTIME_DIR/claude-images` (mode 0700). Without `XDG_RUNTIME_DIR` it uses `$TMPDIR/claude-images-$USER`, or `/tmp/claude-images-$USER`. It writes its own files there through `sh` (`cat`, `mv`), as ImageMagick writes its: an installed plugin's `$.fs` writes did not land there. Its child processes run one at a time and from `/`, so a burst of renders (an agent reading several images at once) and a session whose working directory was removed both still draw. A failed check for ImageMagick or the directory is asked again by the next picture. When the directory takes no write, a Read or pasted image shows a dim note in place of the picture, and the session gets one dim line saying so.

## Install

agent-ways ships the mod in its `agent-ways` marketplace as `images@agent-ways`, released with ways and carrying ways' version (ADR-517). `ways manage repair --commit` installs it in every enabled target, on Linux and macOS alike, where Claude Code is 2.1.304 or later; `mods.images.install` is its wanted state, true by default.

```bash
ways manage mod list                        # wanted and installed, per target
ways manage mod check images                # Claude Code's version, the install, ImageMagick
ways manage mod uninstall images --commit   # writes mods.images.install: false and removes it
ways manage mod install images --commit     # puts it back
```

Repair installs nothing beside a plugin named `images` from another marketplace, whether Claude Code lists it or `settings.json` enables it, and reports it, so two copies never draw each picture twice. Uninstall that copy and run `ways manage repair --commit`.

## Smoke test

In a fresh session in kitty or Ghostty, ask:

```text
show me /usr/share/pixmaps/archlinux-logo.png
```

The picture appears in the transcript.

## Render log

To see which transcript components the mod is handed, create `debug` in the cache directory: `touch $XDG_RUNTIME_DIR/claude-images/debug`. Each tool row, tool result, tool group and message then appends one line to `render.log` beside it: the component, its `requestId`, where it is on screen (`onScreen`: `null` while scrolled out of view, `first-last/of` rows when in it, `absent` when the surface does not say), the tool, and the shape of its props, with strings shown only by their length and booleans by their value. The flag is looked for at most every 2 seconds; remove it to stop.

## Developing

```bash
claude --plugin-dir plugins/images      # load it for one session
claude plugin validate plugins/images
claude plugin test plugins/images
```
