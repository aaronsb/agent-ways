// `ways settings theme export --mod` as the contract states it, for the
// themes the tests and the screenshots use. The role values stand in for the
// exporter's derivation until `ways` ships the verb.

export const MIAMI_NIGHTS = {
  theme: 'miami-nights',
  kind: 'dark',
  roles: {
    bg: '#160634',
    body: '#f4eaff',
    accent: '#ff2a8a',
    accent_dim: '#b0397f',
    info: '#2de2e6',
    ok: '#3dffa2',
    warn: '#ffd319',
    err: '#ff5b3a',
    alt: '#b967ff',
    hot: '#ff972a',
    muted: '#8f7cb3',
    rule: '#4a2d78',
    selection_bg: '#3d1466',
    subtle: '#3a1d63',
  },
  gradient: { axis: 'vertical', stops: ['#0d0221', '#241046', '#4a0e4e'] },
  words: {
    spinner: ['Cruising', 'Gliding', 'Glowing', 'Pulsing', 'Racing', 'Drifting'],
    done: ['Cruised', 'Glowed', 'Docked', 'Landed'],
  },
  render: { rail: true, tables: 'on', shimmer: true, charts: true, markdown: true, icons: 'unicode', frames: true, prompt_box: 'gradient', copy: true },
} as const

export const TAMPA = {
  theme: 'tampa',
  kind: 'light',
  roles: {
    bg: '#fff6e8',
    body: '#22262e',
    accent: '#0057b8',
    accent_dim: '#4f7fbf',
    info: '#00818a',
    ok: '#2c7a2e',
    warn: '#c24600',
    err: '#c8102e',
    alt: '#6c2c91',
    hot: '#d43117',
    muted: '#6f6a66',
    rule: '#d9cdb8',
    selection_bg: '#d8e8f8',
    subtle: '#f5d98a',
  },
  gradient: { axis: 'horizontal', stops: ['#e8f3fb', '#fff6e8', '#ffe4d2'] },
  words: null,
  render: { rail: true, tables: 'on', shimmer: true, charts: true, markdown: true, icons: 'unicode', frames: true, prompt_box: 'gradient', copy: true },
} as const

export const TERMINAL_THEME = {
  theme: 'terminal',
  kind: 'dark',
  roles: null,
  gradient: null,
  words: null,
  render: { rail: true, tables: 'on', shimmer: true, charts: true, markdown: true, icons: 'unicode', frames: true, prompt_box: 'frame', copy: true },
} as const
