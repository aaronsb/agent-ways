# images

A Claude Code mod that draws pictures inline in the terminal transcript, under the row that would otherwise show only a path.

## What it draws

- **SendUserFile**: the image attachments Claude sends you.
- **Read**: an image Claude opens (PNG, JPEG, GIF, WebP), drawn from the bytes the model was shown, under the Read row or under the folded "Read N files" line.
- **Images a reply names**: a markdown image `![alt](/abs/path.png)` or `![alt](~/path.png)`, or a bare absolute or `~/` path such as "saved the chart to /tmp/chart.png" or "`~/Pictures/x.jpg`". Bare paths count in prose, in inline code and in quotes, with trailing punctuation dropped. Fenced code blocks are skipped, and relative paths and URLs are ignored. A path that names no file draws nothing. Each file draws once, in the order the reply names it, up to 4 per reply.
- **Pasted images**: the images pasted with a prompt, drawn under that prompt.

A GIF draws its first frame. Pictures are kept to 72 columns and 30 rows, never wider than the transcript, with the aspect kept. Other surfaces (desktop, VS Code, mobile) are left to Claude Code.

## Pixels or half-blocks

- **Pixels**: in a terminal where Claude Code draws kitty graphics with Unicode placeholders (kitty and Ghostty, or any terminal once `CLAUDE_CODE_FORCE_TERMINAL_IMAGES=1` turns Claude Code's detection on), the mod uses Claude Code's `Image` element.
- **Half-blocks**: everywhere else, Konsole and iTerm2 among them, `Image` would draw only its alt text. The mod draws a `Raster` of `▀` cells instead: each cell's foreground is the upper pixel and its background the lower one, so a cell holds two pixels.

An `Image` reads its picture from the resized PNG in the cache directory; the terminal opens the file itself. Verified in kitty with Claude Code 2.1.304, installed, on this version: the whole scrollback keeps its pictures, including ones scrolled out of view and back. Claude Code frees an `Image`'s pixels when its row leaves the view; the file source is the likely reason they come back, since the terminal can read a file again when the row is drawn again and a bytes source gives it nothing to read. The mod also raises the source's `generation` when a row is reported off screen and then back (`onScreen` `null`, then set), but `onScreen` stayed absent on every render throughout that check, so that step did not fire; it is kept because it is harmless. When Claude Code runs over ssh (`SSH_CONNECTION` or `SSH_TTY` set), the terminal cannot read this machine's files, so the PNG's bytes go in the element instead (see Known issues).

The mod does not keep a list of terminal names. In `auto` it asks Claude Code: it draws the first picture of a session as half-blocks with a 1x1 transparent `Image` beside it, then calls `$.ui.blit` on that `Image`. Claude Code refuses a blit to an `Image` that is drawing its alt, so a refusal means half-blocks and an accepted blit means pixels. The answer holds for the rest of the session.

## Options

Set them in `/config`, or under `pluginConfigs.images.options` in settings.

`showPathsInReplies` (default `true`): draw images a reply names by a bare path. Off, a reply draws only its markdown images.

`steerToSendUserFile` (default `true`): add a short section to the system prompt, on the terminal surface only, saying the terminal draws images and that a request to see one is answered with SendUserFile, which also shows it in remote and web views. Where the request does not offer SendUserFile, the section names Read and absolute paths instead. Off, the prompt is left as Claude Code composes it.

`renderer`:

| Value | Effect |
|---|---|
| `auto` (default) | Pixels where Claude Code draws kitty graphics, half-blocks elsewhere |
| `image` | Always the `Image` element. Shows only the alt text where the terminal cannot draw it |
| `halfblock` | Always half-blocks |

## Requirements

ImageMagick, as `magick` (version 7) or `convert` (version 6), on `PATH`. It decodes and resizes every picture. Without it, each site shows Claude Code's own row and one dim note.

The mod keeps resized pictures, and pasted or Read images written as data URI files, in `$XDG_RUNTIME_DIR/claude-images` (mode 0700). Without `XDG_RUNTIME_DIR` it uses `$TMPDIR/claude-images-$USER`, or `/tmp/claude-images-$USER`. It writes its own files there through `sh` (`cat`, `mv`), as ImageMagick writes its: an installed plugin's `$.fs` writes did not land there. When the directory takes no write, a Read or pasted image shows a dim note in place of the picture, and the session gets one dim line saying so.

## Install

The mod lives in the dotfiles store, in the `dotfiles` marketplace at `claude/mods` (deployed as `~/.local/share/claude-mods`). Register the marketplace once per host, then install:

```bash
claude plugin marketplace add ~/.local/share/claude-mods
```

```text
/plugin install images@dotfiles
```

`claude plugin install images@dotfiles` does the same from a shell.

## Known issues

- **Over ssh, a picture scrolled out of view stays blank when it comes back.** Claude Code frees an `Image`'s pixels when its row leaves the view, and a bytes source cannot be marked new, so nothing is sent again. Locally the mod draws from a file and raises its `generation`; over ssh the terminal cannot read that file, so the mod sends the bytes and has no way to resend them.

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
