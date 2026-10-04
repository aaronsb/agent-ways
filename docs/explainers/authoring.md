# Making explainers

An explainer is an animated page that plays in the manual and renders to video. Each one lives in `docs/explainers/animations/<name>/index.html` and is published with the site. `scripts/explainer/record.mjs` turns any of them into an MP4 or a set of stills.

## The contract

A page is an explainer when its whole state is a function of one clock:

| Piece | What the page provides |
|---|---|
| `window.__duration` | Length in seconds |
| `window.__seek(t)` | Draws the frame at time `t`, with no dependence on the frames before it |
| `?record=1` | Hides the page's own controls and fills a 1920×1080 viewport |
| `?t=<seconds>` | Opens paused at that moment |
| `?embed` | Drops the page margins, for an iframe in the manual |
| `window.__fonts` | Optional list of web font families; the recorder stops if any did not load |

Time is an input, so a render is frame-exact and repeatable (given the same fonts, which `window.__fonts` enforces): no screen capture, no dropped frames, and a still at second 42 always shows the same thing. Keep randomness seeded, and don't use CSS animations or transitions, which run on their own clock.

`a-turn-with-ways/index.html` is the worked example. Its beats are one table of `[start, end, title, caption]`, and `render(t)` sets every element's attributes from `t`.

## Rendering

The recorder needs `node` 22 or later, `chromium` and `ffmpeg`, and no npm packages. It drives headless Chromium over the DevTools protocol, seeks each frame, and pipes the frames to ffmpeg.

```bash
make explainer-video                                   # build/explainers/a-turn-with-ways.mp4
make explainer-video EXPLAINER=<name>                  # another explainer
make explainer-stills EXPLAINER=<name> T=5,20,40       # PNG stills at those seconds
node scripts/explainer/record.mjs <page.html> --fps 60 --out /tmp/x.mp4
```

Output lands in `build/explainers/`, which git ignores. Set `CHROMIUM` when the browser has another name.

Stills are the review loop. Render one per beat, tile them into a contact sheet with ffmpeg's `tile` filter, and read the whole piece at a glance before rendering the full video.

## Adding one

1. Copy `animations/a-turn-with-ways/` to `animations/<name>/` and replace the beats and the drawing.
2. Add a manual page that embeds it with an iframe, as `a-turn-with-ways.md` does, and list the page in `scripts/docs-site/nav.md`.
3. Check the facts on screen against the docs and the code. Mark anything invented, such as example prompts or scores, as illustrative on the page.
4. Render stills, then the video.
