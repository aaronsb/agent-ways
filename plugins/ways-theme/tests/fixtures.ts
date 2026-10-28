// `ways settings theme export --mod` for the themes the tests and the
// screenshots use: the roles and words as the ways build of
// feature/ways-theme-controller derived them. TAMPA's words are dropped, to
// test a theme with none.

export const MIAMI_NIGHTS = {
  theme: 'miami-nights',
  kind: 'dark',
  roles: {
    bg: '#160634',
    body: '#f4eaff',
    accent: '#ff4d93',
    accent_dim: '#ff46aa',
    info: '#2de2e6',
    ok: '#3dffa2',
    warn: '#ffd319',
    err: '#ff5b3a',
    alt: '#bb6eff',
    hot: '#ff9729',
    muted: '#8f7cb3',
    rule: '#644c8b',
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
    accent_dim: '#3b699c',
    info: '#00727a',
    ok: '#28762a',
    warn: '#ab4300',
    err: '#c8102e',
    alt: '#6c2c91',
    hot: '#a30003',
    muted: '#6f6a66',
    rule: '#b2a178',
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
