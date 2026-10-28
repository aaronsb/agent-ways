// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/icons.ts.

import type { Kind, Palette } from './skin'

// Vector icons and spinners for the surfaces that draw `Svg` (the desktop app). Each is a
// whole SVG document; animation is CSS, which plays in the image the desktop draws.
// Every loop lasts a whole second (the thinking orb's breath two), so when a
// redraw starts an icon's animation over it lands back in phase instead of jumping.

// Outline paths on a 24-unit grid.
const PATHS: Readonly<Record<Kind, string>> = {
  run: '<path d="M5 7l5 5-5 5"/><path d="M12 18h7"/>',
  write: '<path d="M4 20h4L18.5 9.5a2.83 2.83 0 0 0-4-4L4 16v4z"/><path d="M13.5 6.5l4 4"/>',
  read: '<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/><path d="M9 13h6M9 17h4"/>',
  search: '<circle cx="10" cy="10" r="6.5"/><path d="M20 20l-5.2-5.2"/>',
  web: '<circle cx="12" cy="12" r="8.5"/><path d="M3.8 9.5h16.4M3.8 14.5h16.4"/><path d="M11.5 3.5a15 15 0 0 0 0 17M12.5 3.5a15 15 0 0 1 0 17"/>',
  mcp: '<path d="M9.8 6.2l8 8-2 2a5.66 5.66 0 0 1-8-8z"/><path d="M4 20l3.6-3.6M15 4l-3.4 3.4M20 9l-3.4 3.4"/>',
  other: '<circle cx="12" cy="12" r="3"/>',
}

const SPIN = [
  '.spin{transform-box:view-box;transform-origin:12px 12px;animation:spin 1s linear infinite}',
  '@keyframes spin{to{transform:rotate(360deg)}}',
].join('')

const svg = (size: number, style: string, body: string): string =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24"><style>${style}</style>${body}</svg>`

// A tool's icon; while it runs, an arc circles it.
export function toolIcon(kind: Kind, color: string, isRunning: boolean): string {
  const stroke = `<g fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${PATHS[kind]}</g>`

  if (!isRunning) {
    return svg(16, '', stroke)
  }

  const shrunk = `<g transform="translate(5 5) scale(.58)">${stroke}</g>`
  const arc = `<circle class="spin" cx="12" cy="12" r="10.5" fill="none" stroke="${color}" stroke-width="1.6" stroke-linecap="round" stroke-dasharray="18 48"/>`

  return svg(16, SPIN, shrunk + arc)
}

export type SpinnerMode = 'requesting' | 'responding' | 'thinking' | 'tool-input' | 'tool-use'

// One motion per thing the turn is doing: a breathing orb while it thinks, a spinning
// arc while a tool runs, an equaliser while it writes, bouncing dots while it waits.
export function spinnerIcon(mode: SpinnerMode, color: string): string {
  const fill = `fill="${color}"`

  switch (mode) {
    case 'thinking':
      return svg(
        20,
        [
          '.core,.wave{transform-box:view-box;transform-origin:12px 12px}',
          // A slower, shallower breath and a ripple that eases out, for a calmer orb.
          '.core{animation:breathe 2s cubic-bezier(.45,0,.55,1) infinite}',
          '.wave{animation:wave 2s cubic-bezier(.2,.6,.3,1) infinite}',
          '@keyframes breathe{0%,100%{transform:scale(.82);opacity:.78}50%{transform:scale(1);opacity:1}}',
          '@keyframes wave{from{transform:scale(1);opacity:.5}to{transform:scale(2.4);opacity:0}}',
        ].join(''),
        `<circle class="wave" cx="12" cy="12" r="4" fill="none" stroke="${color}" stroke-width="1"/><circle class="core" cx="12" cy="12" r="4" ${fill}/>`,
      )
    case 'tool-use':
      return svg(
        20,
        SPIN,
        `<circle cx="12" cy="12" r="8" fill="none" stroke="${color}" stroke-opacity=".18" stroke-width="2"/><circle class="spin" cx="12" cy="12" r="8" fill="none" stroke="${color}" stroke-width="2" stroke-linecap="round" stroke-dasharray="14 40"/>`,
      )
    case 'responding':
      return svg(
        20,
        [
          '.bar{transform-box:fill-box;transform-origin:center bottom;animation:eq 1s ease-in-out infinite}',
          '.b2{animation-delay:.15s}.b3{animation-delay:.3s}',
          '@keyframes eq{0%,100%{transform:scaleY(.35)}50%{transform:scaleY(1)}}',
        ].join(''),
        [5, 10.5, 16]
          .map((x, i) => `<rect class="bar b${i + 1}" x="${x}" y="5" width="3" height="14" rx="1.5" ${fill}/>`)
          .join(''),
      )
    default:
      return svg(
        20,
        [
          '.dot{transform-box:fill-box;transform-origin:center;animation:hop 1s ease-in-out infinite}',
          '.d2{animation-delay:.15s}.d3{animation-delay:.3s}',
          '@keyframes hop{0%,60%,100%{transform:translateY(0);opacity:.45}30%{transform:translateY(-4px);opacity:1}}',
        ].join(''),
        [6, 12, 18].map((x, i) => `<circle class="dot d${i + 1}" cx="${x}" cy="13" r="2.2" ${fill}/>`).join(''),
      )
  }
}

// The spinner's colour says what the turn is doing. A skin with its own activity
// colours lends them (thinking in its MCP colour, a tool in its run colour, and so on); a
// monochrome skin, whose activity colours are all one, gets a hue per activity instead, darker
// on a light background.
const MODE_SLOT: Readonly<Record<SpinnerMode, Kind>> = {
  thinking: 'mcp',
  responding: 'write',
  'tool-use': 'run',
  'tool-input': 'read',
  requesting: 'search',
}

const OWN_DARK: Readonly<Record<SpinnerMode, string>> = {
  thinking: '#b392f0',
  responding: '#79b8ff',
  'tool-use': '#f2c462',
  'tool-input': '#7fd19b',
  requesting: '#9aa5b8',
}

const OWN_LIGHT: Readonly<Record<SpinnerMode, string>> = {
  thinking: '#7048c8',
  responding: '#1f6feb',
  'tool-use': '#b07800',
  'tool-input': '#1a7f37',
  requesting: '#57606a',
}

// Relative brightness of a #rgb or #rrggbb colour, 0 to 1; 1 for anything else.
const brightness = (color: string): number => {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim())?.[1]
  if (hex === undefined) return 1
  const full = hex.length === 3 ? [...hex].map(ch => ch + ch).join('') : hex
  const [r, g, b] = [0, 2, 4].map(at => parseInt(full.slice(at, at + 2), 16) / 255)
  return 0.2126 * (r ?? 1) + 0.7152 * (g ?? 1) + 0.0722 * (b ?? 1)
}

export function spinnerColor(mode: SpinnerMode, palette: Palette): string {
  const activities = new Set((['read', 'write', 'run', 'search', 'web', 'mcp'] as const).map(kind => palette[kind]))
  if (activities.size > 1) return palette[MODE_SLOT[mode]] ?? palette.user
  return (brightness(palette.user) < 0.5 ? OWN_LIGHT : OWN_DARK)[mode]
}
