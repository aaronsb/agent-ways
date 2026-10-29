import { describe, expect, mock, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On, ProcessRunResult } from 'claude-code'

// The test's environment has a timer; Claude Code's declarations name none.
declare function setTimeout(fn: () => void, ms: number): unknown

import {
  DEFAULT_COLOR,
  LOWER_HALF,
  MIN_COLUMNS,
  SPACE,
  UPPER_HALF,
  cellsToBase64,
  chooseRenderer,
  contentImages,
  DEFAULT_OPTIONS,
  fileUrl,
  fitCells,
  frameInset,
  pngArgv,
  fromBase64,
  REPLY_IMAGES_MAX,
  needsProbe,
  optionsArgv,
  packHalfBlocks,
  parseCellPixels,
  parseDimensions,
  parseOptions,
  parseRendererOption,
  parseScale,
  probeVerdict,
  probesCells,
  replyImages,
  rgbaArgv,
  roomAt,
  shownPercent,
  takeExisting,
  toBase64,
  toolImages,
} from '../hooks/lib.ts'
import { register } from '../hooks/register.tsx'
import { passedOverHits, registrationsOf } from './passed-over.ts'
import { contractBreaches } from './picture-frame.ts'

describe('sizing', () => {
  const ROOM = { columns: 200, rows: 60 }

  test('a picture that fits shows pixel for pixel in the measured cells', () => {
    // 1600x400 in 10x20 cells: 160 columns, 160 * 400/1600 * 10/20 = 20 rows.
    expect(fitCells({ width: 1600, height: 400, renderer: 'image', room: ROOM, cell: { width: 10, height: 20 } })).toEqual({
      columns: 160,
      rows: 20,
      pixelWidth: 1600,
      pixelHeight: 400,
      isBlocky: false,
    })
    // 8x16 cells: 40 columns for 320 pixels, 40 * 200/320 * 8/16 = 12.5 rows.
    expect(fitCells({ width: 320, height: 200, renderer: 'image', room: ROOM, cell: { width: 8, height: 16 } })).toMatchObject({
      columns: 40,
      rows: 13,
    })
  })

  test('a picture larger than the room shrinks to it, its aspect kept', () => {
    // 8000x6000: the 60 rows hold it to 60 / (0.75 * 0.5) = 160 columns.
    expect(fitCells({ width: 8000, height: 6000, renderer: 'image', room: ROOM })).toEqual({
      columns: 160,
      rows: 60,
      pixelWidth: 1600,
      pixelHeight: 1200,
      isBlocky: false,
    })
    // 400x1600 in half-blocks: 30 rows give 30 / (4 * 0.5) = 15 columns.
    expect(fitCells({ width: 400, height: 1600, renderer: 'halfblock', room: { columns: 200, rows: 30 } })).toEqual({
      columns: 15,
      rows: 30,
      pixelWidth: 15,
      pixelHeight: 60,
      isBlocky: false,
    })
  })

  test('a small picture is enlarged to the narrowest width, nearest-neighbour', () => {
    const fit = fitCells({ width: 16, height: 16, renderer: 'image', room: ROOM })
    expect(fit).toEqual({ columns: MIN_COLUMNS, rows: 12, pixelWidth: 240, pixelHeight: 240, isBlocky: true })
    expect(pngArgv('magick', '/p/i.png', fit, '/c/k.png')).toEqual([
      'magick',
      '/p/i.png[0]',
      '-auto-orient',
      '-sample',
      '240x240',
      '-strip',
      'png:/c/k.png',
    ])
    const blocks = fitCells({ width: 4, height: 4, renderer: 'halfblock', room: ROOM })
    expect(blocks).toEqual({ columns: MIN_COLUMNS, rows: 12, pixelWidth: MIN_COLUMNS, pixelHeight: 24, isBlocky: true })
    expect(rgbaArgv('magick', '/p/i.png', blocks, '/c/k.rgba')).toContain('-sample')
  })

  test('an enlargement under NEAREST_FROM is left to the terminal to blend', () => {
    // 220 pixels in 24 columns of 10: 1.09 times, under 1.25.
    const fit = fitCells({ width: 220, height: 220, renderer: 'image', room: ROOM })
    expect(fit).toMatchObject({ columns: MIN_COLUMNS, pixelWidth: 220, pixelHeight: 220, isBlocky: false })
    expect(pngArgv('magick', '/p/i.png', fit, '/c/k.png')).toContain('220x220>')
    // 160 pixels: 1.5 times, so blocks.
    expect(fitCells({ width: 160, height: 160, renderer: 'image', room: ROOM }).isBlocky).toBe(true)
  })

  test('scale multiplies the natural size; the narrowest width still holds', () => {
    const at = (scale: number) => fitCells({ width: 320, height: 200, renderer: 'image', room: ROOM, scale })
    expect(at(1)).toMatchObject({ columns: 32, rows: 10 })
    // Twice the size is twice the pixels, each a 2x2 block.
    expect(at(2)).toMatchObject({ columns: 64, rows: 20, pixelWidth: 640, pixelHeight: 400, isBlocky: true })
    expect(at(0.5)).toMatchObject({ columns: MIN_COLUMNS, rows: 8 })
  })

  test('the room is the transcript width and a share of its rows', () => {
    expect(roomAt({ columns: 50, rows: 40 }, 5)).toEqual({ columns: 44, rows: 24 })
    // An unmeasured surface is taken as 80 columns and 30 rows.
    expect(roomAt(undefined, 5)).toEqual({ columns: 74, rows: 30 })
    // A sliver of room still draws one cell.
    expect(fitCells({ width: 100, height: 100, renderer: 'halfblock', room: { columns: 0, rows: 0 } })).toMatchObject({
      columns: 1,
      rows: 1,
    })
  })

  test('the scale option is a number held to its bounds', () => {
    expect(parseScale(1.5)).toBe(1.5)
    expect(parseScale('2')).toBe(2)
    expect(parseScale(10)).toBe(4)
    expect(parseScale(0.1)).toBe(0.25)
    for (const bad of [0, -1, 'x', '', undefined, null]) expect(parseScale(bad)).toBe(1)
  })

  test('the cell size comes from the terminal pixels; none reported is null', () => {
    expect(parseCellPixels('99 178 1780 1980\n')).toEqual({ width: 10, height: 20 })
    expect(parseCellPixels('24 80 0 0')).toBeNull()
    expect(parseCellPixels('')).toBeNull()
  })

  test('the caption percentage compares screen pixels with picture pixels', () => {
    expect(shownPercent(320, 32, { width: 10, height: 20 })).toBe(100)
    expect(shownPercent(1920, 160, { width: 10, height: 20 })).toBe(83)
    expect(fileUrl('/tmp/a b#1.png')).toBe('file:///tmp/a%20b%231.png')
  })

  test('the ImageMagick argv resizes exactly for half-blocks, and reads the first frame', () => {
    const fit = fitCells({ width: 1600, height: 400, renderer: 'halfblock', room: ROOM })
    expect(rgbaArgv('magick', '/p/a.gif', fit, '/c/k.rgba')).toEqual([
      'magick',
      '/p/a.gif[0]',
      '-auto-orient',
      '-resize',
      '200x50!',
      '-depth',
      '8',
      'rgba:/c/k.rgba',
    ])
    expect(parseDimensions('640 480')).toEqual({ width: 640, height: 480 })
    expect(parseDimensions('garbage')).toBeNull()
  })
})

describe('half-block packing', () => {
  // A 2x4 RGBA picture, row-major, every pixel opaque.
  const px = (r: number, g: number, b: number) => [r, g, b, 255]
  const rgba = Uint8Array.from([
    ...px(255, 0, 0), ...px(0, 255, 0),     // y0
    ...px(0, 0, 255), ...px(255, 255, 0),   // y1
    ...px(0, 255, 255), ...px(255, 0, 255), // y2
    ...px(16, 32, 48), ...px(255, 255, 255), // y3
  ])

  test('a known 2x4 picture packs into exact cell triplets', () => {
    const cells = packHalfBlocks(rgba, 2, 4)
    expect([...cells]).toEqual([
      // row 0: (y0 over y1)
      UPPER_HALF, 0xff0000, 0x0000ff,
      UPPER_HALF, 0x00ff00, 0xffff00,
      // row 1: (y2 over y3)
      UPPER_HALF, 0x00ffff, 0x102030,
      UPPER_HALF, 0xff00ff, 0xffffff,
    ])
    expect(UPPER_HALF).toBe(0x2580)
  })

  test('the cells prop is padded base64 of little-endian u32s', () => {
    const one = cellsToBase64(Uint32Array.of(0x2588, 0xff8800, DEFAULT_COLOR))
    const bytes = fromBase64(one)
    expect([...bytes]).toEqual([0x88, 0x25, 0, 0, 0x00, 0x88, 0xff, 0, 0, 0, 0, 0x01])
    expect(one.length % 4).toBe(0)
    expect(cellsToBase64(packHalfBlocks(rgba, 2, 4)).length).toBe(Math.ceil((2 * 2 * 3 * 4) / 3) * 4)
  })

  test('clear pixels take the terminal default; an odd last row stands over clear', () => {
    const clear = [0, 0, 0, 0]
    // 2x3: (opaque over clear), (clear over opaque), then a lone last row.
    const picture = Uint8Array.from([
      ...px(1, 2, 3), ...clear,
      ...clear, ...px(4, 5, 6),
      ...clear, ...px(7, 8, 9),
    ])
    expect([...packHalfBlocks(picture, 2, 3)]).toEqual([
      UPPER_HALF, 0x010203, DEFAULT_COLOR,
      LOWER_HALF, 0x040506, DEFAULT_COLOR,
      SPACE, DEFAULT_COLOR, DEFAULT_COLOR,
      UPPER_HALF, 0x070809, DEFAULT_COLOR,
    ])
  })

  test('base64 round-trips every length', () => {
    for (const n of [0, 1, 2, 3, 4, 5, 64]) {
      const bytes = Uint8Array.from({ length: n }, (_, i) => (i * 37) & 255)
      expect([...fromBase64(toBase64(bytes))]).toEqual([...bytes])
    }
    expect(toBase64(Uint8Array.of(1, 2))).toBe('AQI=')
  })
})

describe('renderer choice', () => {
  test('auto follows the kitty graphics capability; unknown draws half-blocks', () => {
    expect(chooseRenderer('auto', true)).toBe('image')
    expect(chooseRenderer('auto', false)).toBe('halfblock')
    expect(chooseRenderer('auto', null)).toBe('halfblock')
  })

  test('a forced renderer wins over the capability', () => {
    expect(chooseRenderer('image', false)).toBe('image')
    expect(chooseRenderer('image', null)).toBe('image')
    expect(chooseRenderer('halfblock', true)).toBe('halfblock')
  })

  test('only auto with an unknown capability probes', () => {
    expect(needsProbe('auto', null)).toBe(true)
    expect(needsProbe('auto', false)).toBe(false)
    expect(needsProbe('auto', true)).toBe(false)
    expect(needsProbe('image', null)).toBe(false)
    expect(needsProbe('halfblock', null)).toBe(false)
  })

  test('the probe reads a taken blit as pixels, an alt refusal as none, anything else as unknown', () => {
    expect(probeVerdict(undefined)).toBe(true)
    expect(probeVerdict('the Image draws its alt there: no placeholder images')).toBe(false)
    expect(probeVerdict('not mounted')).toBeNull()
    expect(probeVerdict('no-surface')).toBeNull()
  })

  test('an unknown option value is auto', () => {
    expect(parseRendererOption('halfblock')).toBe('halfblock')
    expect(parseRendererOption('sixel')).toBe('auto')
    expect(parseRendererOption(undefined)).toBe('auto')
  })
})

describe('finding images', () => {
  test('markdown images with an absolute or ~ path, code skipped', () => {
    const text = [
      'Here: ![chart](/tmp/out/chart.png) and ![home](~/pics/cat.JPG "a cat")',
      'Relative ![no](pics/a.png), remote ![no](https://x/a.png), not an image ![no](/tmp/a.txt)',
      'Inline `![no](/tmp/code.png)` is code.',
      '```md',
      '![no](/tmp/fenced.png)',
      '```',
      '~~~',
      '![no](/tmp/tilde-fenced.png)',
      '~~~',
      'Angle ![spaced](</tmp/a b.webp>)',
    ].join('\n')
    expect(replyImages(text, '/home/u', { bare: false })).toEqual([
      { file: '/tmp/out/chart.png', alt: 'chart' },
      { file: '/home/u/pics/cat.JPG', alt: 'home' },
      { file: '/tmp/a b.webp', alt: 'spaced' },
    ])
    expect(replyImages('![x](~/a.png)', undefined, { bare: false })).toEqual([])
  })

  const bare = (text: string) => replyImages(text, '/home/u', { bare: true }).map(r => r.file)

  test('a bare path loses trailing punctuation', () => {
    expect(bare('saved the chart to /tmp/chart.png.')).toEqual(['/tmp/chart.png'])
    expect(bare('see /a/b.jpg, /a/c.jpeg; /a/d.gif: and (/a/e.webp) [/a/f.png]')).toEqual([
      '/a/b.jpg',
      '/a/c.jpeg',
      '/a/d.gif',
      '/a/e.webp',
      '/a/f.png',
    ])
    expect(bare('done!/x.png? no; done: /x.png!')).toEqual(['/x.png'])
  })

  test('a bare path in backticks or quotes counts, inline code included', () => {
    expect(bare('saved to `~/Pictures/x.jpg`')).toEqual(['/home/u/Pictures/x.jpg'])
    expect(bare(`wrote "/tmp/q.png" and '/tmp/s.PNG'.`)).toEqual(['/tmp/q.png', '/tmp/s.PNG'])
  })

  test('~ expands from home, and drops with no home', () => {
    expect(bare('at ~/a/b.png')).toEqual(['/home/u/a/b.png'])
    expect(replyImages('at ~/a/b.png and /c.png', undefined, { bare: true }).map(r => r.file)).toEqual(['/c.png'])
    expect(bare('not ~user/a.png nor ~a.png')).toEqual([])
  })

  test('a fenced block is skipped; relative paths, URLs and other extensions are ignored', () => {
    const text = ['before /in/prose.png', '```sh', 'cp x /in/fence.png', '```', 'after'].join('\n')
    expect(bare(text)).toEqual(['/in/prose.png'])
    expect(bare('pics/a.png https://x.dev/a.png file:///b.png')).toEqual([])
    expect(bare('/tmp/a.txt /tmp/b.svg /tmp/c.pdf /tmp/d.png.bak /tmp/e.pngx')).toEqual([])
  })

  test('a path named twice draws once, at its first mention, markdown and bare alike', () => {
    expect(replyImages('first /p/a.png then ![A](/p/a.png) and /p/b.png, /p/a.png', '/h', { bare: true })).toEqual([
      { file: '/p/a.png', alt: 'a.png' },
      { file: '/p/b.png', alt: 'b.png' },
    ])
    expect(replyImages('![Alt](/p/a.png)', '/h', { bare: true })).toEqual([{ file: '/p/a.png', alt: 'Alt' }])
  })

  test('the cap keeps the first four files that exist, in order', async () => {
    const named = replyImages('/1.png /2.png /gone.png /3.png /4.png /5.png', '/h', { bare: true })
    const exists = async (path: string) => path !== '/gone.png'
    const kept = await takeExisting(named, exists, REPLY_IMAGES_MAX)
    expect(kept.map(r => r.file)).toEqual(['/1.png', '/2.png', '/3.png', '/4.png'])
    expect(REPLY_IMAGES_MAX).toBe(4)
  })

  test('a Read that returned an image yields its bytes; a text Read yields nothing', () => {
    const image = { type: 'image', file: { base64: 'AAAA', type: 'image/png', originalSize: 3 } }
    expect(toolImages('Read', { file_path: '/p/shot.png' }, image, 't1')).toEqual([
      { data: 'AAAA', mime: 'image/png', id: 't1', alt: 'shot.png', path: '/p/shot.png' },
    ])
    // A relative path names no file to link to.
    expect(toolImages('Read', { file_path: 'shot.png' }, image, 't1')).toEqual([
      { data: 'AAAA', mime: 'image/png', id: 't1', alt: 'shot.png' },
    ])
    const text = { type: 'text', file: { filePath: '/p/a.ts', content: '' } }
    expect(toolImages('Read', { file_path: '/p/a.ts' }, text, 't2')).toEqual([])
  })

  test('SendUserFile yields its image attachments, or its input files before a result', () => {
    const output = {
      attachments: [
        { path: '/out/a.png', size: 1, isImage: true },
        { path: '/out/report.pdf', size: 1, isImage: false },
        { path: '/out/b.jpeg', size: 1, isImage: false },
      ],
    }
    expect(toolImages('SendUserFile', {}, output, 't').map(r => ('file' in r ? r.file : ''))).toEqual([
      '/out/a.png',
      '/out/b.jpeg',
    ])
    const input = { files: ['/out/c.webp', 'rel.png', '/out/d.csv'], status: 'normal' }
    expect(toolImages('SendUserFile', input, undefined, 't')).toEqual([{ file: '/out/c.webp', alt: 'c.webp', path: '/out/c.webp' }])
    expect(toolImages('Bash', {}, {}, 't')).toEqual([])
  })

  test('a prompt row yields its base64 image blocks', () => {
    const content = [
      { type: 'text', text: 'look [Image #1]' },
      { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'QUJD' } },
      { type: 'image', source: { type: 'url', url: 'https://x' } },
    ]
    expect(contentImages(content, 'u1')).toEqual([
      { data: 'QUJD', mime: 'image/png', id: 'u1-1', alt: 'pasted image 1' },
    ])
  })
})

// The engine beneath the plugin, for the drawing tests.

const ok = (stdout = ''): ProcessRunResult => ({
  exitCode: 0,
  stdout,
  stderr: '',
  isStdoutTruncated: false,
  isStderrTruncated: false,
})

const PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVQI12NgAAIAAAUAAeImBZsAAAAASUVORK5CYII='

type World = {
  hasMagick: boolean
  blit: string | undefined
  ran: string[][]
  missing?: string[]
  /** `<cache>/debug` exists: the render log is on. */
  debug?: boolean
  /** Every file the mod wrote through `sh -c cat`, as [path, text]. */
  writes?: [string, string][]
  /** The cache directory takes no write: `sh -c cat` fails. */
  unwritable?: boolean
  /** Every `$.ui.log` line. */
  logs?: string[]
  /** The host refuses a child started while another is still running. */
  oneAtATime?: boolean
  /** The session's working directory is gone: a child that inherits it cannot start. */
  cwdGone?: boolean
  /** How many `mkdir` runs fail before one succeeds. */
  mkdirFailures?: number
  /** What the terminal size child prints: `rows cols xpixels ypixels`. */
  winsize?: string
  /** What `uname -s` prints; Linux when left out. */
  uname?: string
  /** More environment, over the defaults. */
  env?: Record<string, string>
  /** The `mods.images.*` values `ways settings list` resolves, over the defaults; `fail` exits 2, `garbage` prints no JSON. */
  settings?: Record<string, unknown> | 'fail' | 'garbage'
  /** Every `ways` argv the mod ran. */
  ways?: string[][]
  /** Every `$.ui.log` line sent to the debug log. */
  debugLines?: string[]
  /** The row beneath the mod is a frame, as ways-theme draws one. */
  frame?: boolean
  /** What ImageMagick's `info:` prints for every picture: `width height`. */
  dims?: string
}

/** `ways settings list mods.images --json --effective` answering `values` over the defaults. */
function settingsJson(values: Record<string, unknown>): string {
  const all: Record<string, unknown> = {
    install: true,
    renderer: 'auto',
    scale: 1,
    show_captions: true,
    show_paths_in_replies: true,
    steer_to_send_user_file: true,
    ...values,
  }
  const keys = Object.fromEntries(Object.entries(all).map(([k, value]) => [`mods.images.${k}`, { value, layer: 'default' }]))
  return JSON.stringify({ view: 'effective', fragment: { config: { mods: { images: all } } }, keys })
}

function engine(on: On, world: World) {
  mock.env(on, { HOME: '/home/t', XDG_RUNTIME_DIR: '/run/user/1000', USER: 't', ...world.env })
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('session.root', () => ({ value: '/work/proj' }) as never)
  const written = new Set<string>()
  /** The raw RGBA size each `rgba:` output was resized to. */
  const resized = new Map<string, number>()
  let inFlight = 0
  on('process.run', async ($, e) => {
    world.ran.push([...e.argv])
    const [tool] = e.argv
    if (world.cwdGone === true && e.init?.cwd === undefined) throw new Error(`spawn ${tool} ENOENT`)
    if (world.oneAtATime === true) {
      if (inFlight > 0) throw new Error('process.run refused: another child is running')
      inFlight++
      await new Promise<void>(resolve => setTimeout(resolve, 2))
      inFlight--
    }
    if (tool === 'ways') {
      world.ways?.push([...e.argv])
      if (world.settings === 'fail') return { value: { ...ok(), exitCode: 2 } }
      if (world.settings === 'garbage') return { value: ok('mods.images.scale=2\n') }
      return { value: ok(settingsJson(world.settings ?? {})) }
    }
    if (tool === 'uname') return { value: ok(`${world.uname ?? 'Linux'}\n`) }
    if (tool === 'python3') return { value: ok(world.winsize ?? '') }
    if (tool === 'mkdir' && (world.mkdirFailures ?? 0) > 0) {
      world.mkdirFailures = (world.mkdirFailures ?? 0) - 1
      return { value: { ...ok(), exitCode: 1 } }
    }
    if (tool === 'sh' && e.argv[1] === '-c' && /\bcat\b/.test(e.argv[2] ?? '')) {
      if (world.unwritable === true) return { value: { ...ok(), exitCode: 1 } }
      const path = e.argv[4] ?? ''
      written.add(path)
      world.writes?.push([path, e.init?.stdin ?? ''])
      return { value: ok() }
    }
    if (tool === 'magick' || tool === 'convert') {
      if (!world.hasMagick) return { value: { ...ok(), exitCode: 127 } }
      const out = e.argv.at(-1) ?? ''
      const box = /^(\d+)x(\d+)!$/.exec(e.argv.find(arg => /^\d+x\d+!$/.test(arg)) ?? '')
      if (out.startsWith('rgba:') && box !== null) resized.set(out.slice(5), Number(box[1]) * Number(box[2]) * 4)
      if (e.argv[1] === '-version') return { value: ok('Version: ImageMagick 7.1') }
      if (e.argv.at(-1) === 'info:') return { value: ok(world.dims ?? '4 4') }
    }
    return { value: ok() }
  })
  on('fs.stat', ($, e) => {
    const path = String((e as { path?: unknown }).path ?? '')
    if (world.missing?.includes(path)) throw new Error(`ENOENT: ${path}`)
    const isThere = path.endsWith('/claude-images/debug')
      ? world.debug === true
      : path.endsWith('.uri') || path.endsWith('.log')
        ? written.has(path)
        : true
    if (!isThere) throw new Error(`ENOENT: ${path}`)
    return { value: { kind: 'file', size: 10, mtimeMs: 1, isLink: false } }
  })
  // An installed plugin's $.fs writes did not land: the mod must not use them.
  on('fs.write', ($, e) => ({ deny: `fs.write is not for this plugin: ${e.path}` }))
  on('fs.exists', ($, e) => ({ deny: `fs.exists is not for this plugin: ${e.path}` }))
  on('fs.read', ($, e) => {
    const path = String((e as { path?: unknown }).path ?? '')
    return { value: { base64: path.endsWith('.png') ? PNG : toBase64(new Uint8Array(resized.get(path) ?? 0).fill(255)) } }
  })
  on('ui.log', ($, e) => {
    const { text, to } = e as { text?: unknown; to?: unknown }
    ;(to === 'debug' ? world.debugLines : world.logs)?.push(String(text ?? ''))
    return { value: undefined }
  })
  on('ui.blit', () => ({ value: world.blit === undefined ? {} : { deny: world.blit } }))
  on('ui.render', ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const row = h(Text, {}, 'engine-row')
    // The frame as ways-theme's framedRow draws it around a row.
    if (world.frame === true) return h(Box, { marginTop: 1, borderStyle: 'round', paddingX: 1, borderColor: 'suggestion' }, h(Box, { marginTop: -1 }, row)) as never
    return row as never
  })
}

const SENT = {
  plugin: 'images',
  surface: 'terminal',
  component: 'ToolResult',
  requestId: 'tu-1',
  viewport: { columns: 100, rows: 40 },
  props: {
    tool_use_id: 'tu-1',
    tool: 'SendUserFile',
    isErrored: false,
    output: { attachments: [{ path: '/pics/cat.png', size: 10, isImage: true }] },
  },
} as const

async function drawn($: Engine) {
  const ui = await $.ui.mount(SENT as never)
  const found = {
    row: await ui.find({ text: 'engine-row' }),
    raster: await ui.find({ type: 'Raster' }),
    image: await ui.find({ key: 'images-0', type: 'Image' }),
    probe: await ui.find({ key: 'images-probe' }),
    note: await ui.find({ text: /ImageMagick/ }),
  }
  await ui.unmount()
  return found
}

describe('drawing', () => {
  test('auto: half-blocks under the row, with a probe; an alt refusal settles on half-blocks', async ($, on) => {
    const world: World = { hasMagick: true, blit: 'the Image draws its alt there: no placeholder images', ran: [] }
    const clock = mock.clock(on)
    engine(on, world)
    const first = await drawn($)
    expect(first.row).toBeDefined()
    expect(first.raster).toBeDefined()
    expect(first.probe).toBeDefined()
    expect(world.ran).toContainEqual(['mkdir', '-p', '-m', '700', '--', '/run/user/1000/claude-images'])

    await clock.advance(300)
    const settled = await drawn($)
    expect(settled.raster).toBeDefined()
    expect(settled.probe).toBeUndefined()
  })

  test('auto: a taken probe blit switches later pictures to Image', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [] }
    const clock = mock.clock(on)
    engine(on, world)
    expect((await drawn($)).raster).toBeDefined()
    await clock.advance(300)
    const settled = await drawn($)
    expect(settled.image).toBeDefined()
    expect(settled.raster).toBeUndefined()
    expect(settled.probe).toBeUndefined()
  })

  test('an unmounted probe tries again, then leaves it unknown', async ($, on) => {
    const world: World = { hasMagick: true, blit: 'not mounted', ran: [] }
    const clock = mock.clock(on)
    engine(on, world)
    await drawn($)
    await clock.advance(2_000)
    const later = await drawn($)
    expect(later.raster).toBeDefined()
    expect(later.probe).toBeDefined()
  })

  test('renderer image forces the Image element', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { renderer: 'image' } })
    const found = await drawn($)
    expect(found.image).toBeDefined()
    expect(found.raster).toBeUndefined()
    expect(found.probe).toBeUndefined()
  })

  test('renderer halfblock forces the Raster, with no probe', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { renderer: 'halfblock' } })
    const found = await drawn($)
    expect(found.raster).toBeDefined()
    expect(found.probe).toBeUndefined()
  })

  test('without ImageMagick: the engine row and one note, no picture', async ($, on) => {
    engine(on, { hasMagick: false, blit: undefined, ran: [] })
    const found = await drawn($)
    expect(found.row).toBeDefined()
    expect(found.note).toBeDefined()
    expect(found.raster).toBeUndefined()
    expect(found.image).toBeUndefined()
  })

  test('a reply with a markdown image draws it under the text', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [] }
    engine(on, world)
    const ui = await $.ui.mount({
      plugin: 'images',
      surface: 'terminal',
      component: 'AssistantMessage',
      viewport: { columns: 100, rows: 40 },
      props: { text: 'Here it is: ![cat](~/pics/cat.png)', isFirstOfReply: true },
    } as never)
    expect(await ui.find({ type: 'Raster' })).toBeDefined()
    expect(world.ran.some(argv => argv[1] === '/home/t/pics/cat.png[0]')).toBe(true)
    await ui.unmount()
  })

  const REPLY = {
    plugin: 'images',
    surface: 'terminal',
    component: 'AssistantMessage',
    viewport: { columns: 100, rows: 40 },
    props: { text: 'Saved the chart to `/tmp/chart.png`, and /tmp/missing.png.', isFirstOfReply: true },
  } as const

  test('a bare path in a reply draws when the file is there, and nothing when it is not', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], missing: ['/tmp/missing.png'] }
    engine(on, world)
    const ui = await $.ui.mount(REPLY as never)
    expect(await ui.findAll({ type: 'Raster' })).toHaveLength(1)
    expect(world.ran.some(argv => argv[1] === '/tmp/missing.png[0]')).toBe(false)
    await ui.unmount()
  })

  test('show_paths_in_replies off: only markdown images draw', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { show_paths_in_replies: false } })
    const ui = await $.ui.mount(REPLY as never)
    expect(await ui.find({ type: 'Raster' })).toBeUndefined()
    expect(await ui.find({ text: 'engine-row' })).toBeDefined()
    await ui.unmount()
  })

  test('a caption names the file, linked to it, with its size and the share shown', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    const ui = await $.ui.mount(SENT as never)
    const link = await ui.find({ type: 'Link' })
    expect(link?.props.href).toBe('file:///pics/cat.png')
    expect(link?.props.label).toBe('cat.png')
    // 4x4 enlarged to 24 columns of 10 pixels.
    expect(await ui.find({ text: ' · 4×4 · 6000%' })).toBeDefined()
    await ui.unmount()
  })

  test('show_captions off: the picture alone', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { show_captions: false } })
    const ui = await $.ui.mount(SENT as never)
    expect(await ui.find({ type: 'Raster' })).toBeDefined()
    expect(await ui.find({ type: 'Link' })).toBeUndefined()
    await ui.unmount()
  })

  test('the measured cell size sizes the picture', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], winsize: '40 100 800 640' }
    engine(on, world)
    const ui = await $.ui.mount(SENT as never)
    // 8x16 cells: 24 columns of 8 pixels show 4 pixels at 4800%.
    expect(await ui.find({ text: ' · 4×4 · 4800%' })).toBeDefined()
    await ui.unmount()
  })

  test('a row on another surface is left to the engine', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    const ui = await $.ui.mount({ ...SENT, surface: 'desktop' } as never)
    expect(await ui.find({ text: 'engine-row' })).toBeDefined()
    await ui.unmount()
  })
})

describe('render log', () => {
  const LOG = '/run/user/1000/claude-images/render.log'

  test('off by default: nothing is written to render.log', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [] }
    engine(on, world)
    await drawn($)
    expect(world.writes?.some(([path]) => path === LOG)).toBe(false)
  })

  test('with the debug flag: one line per render, shapes and no bytes', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [], debug: true }
    mock.clock(on)
    engine(on, world)
    await drawn($)
    const logged = world.writes?.filter(([path]) => path === LOG).at(-1)?.[1] ?? ''
    expect(logged).toContain(' terminal ToolResult tu-1 onScreen=absent tool=SendUserFile props={')
    expect(logged).toContain('isErrored:false, output:{attachments:[1 x {path:str(13), size:number, isImage:true}]}')
    expect(logged).not.toContain('/pics/cat.png')
  })
})

describe('prompt section', () => {
  const BASE = { sections: [{ id: 'intro', text: 'base', scope: 'shared' }] } as const
  const TOOLS = ['Read', 'SendUserFile']
  const FACTS = { model: 'claude-opus-5-5', promptModel: 'claude-opus-5-5', outputStyle: null, traits: [] } as const

  test('on the terminal: one session section, added last, naming SendUserFile', async ($, on) => {
    on('prompt.compose', () => BASE as never)
    const { sections } = await $.prompt.compose({ ...FACTS, surfaces: ['terminal'], tools: TOOLS })
    expect(sections).toHaveLength(2)
    expect(sections[1]).toMatchObject({ id: 'images:show-images', scope: 'session' })
    expect(sections[1]?.text).toContain('send it with SendUserFile')
    expect(sections[1]?.text).not.toMatch(/MUST/)
  })

  test('without SendUserFile offered: Read or a path, nothing about SendUserFile', async ($, on) => {
    on('prompt.compose', () => BASE as never)
    const { sections } = await $.prompt.compose({ ...FACTS, surfaces: ['terminal'], tools: ['Read'] })
    expect(sections[1]?.text).toContain('Read it or name its absolute path')
    expect(sections[1]?.text).not.toContain('SendUserFile')
  })

  test('on another surface: no section', async ($, on) => {
    on('prompt.compose', () => BASE as never)
    const { sections } = await $.prompt.compose({ ...FACTS, surfaces: ['desktop'], tools: TOOLS })
    expect(sections.map(s => s.id)).toEqual(['intro'])
  })

  test('steer_to_send_user_file off: no section', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], ways: [], settings: { steer_to_send_user_file: false } }
    engine(on, world)
    on('prompt.compose', () => BASE as never)
    await $.session.start({ cwd: '/work/proj', surface: 'terminal', isInteractive: true })
    const { sections } = await $.prompt.compose({ ...FACTS, surfaces: ['terminal'], tools: TOOLS })
    expect(sections.map(s => s.id)).toEqual(['intro'])
    // Composing reads nothing: the session's start read them once.
    expect(world.ways).toHaveLength(1)
  })
})

// The shapes Claude Code 2.1.304 hands the mod for a Read of a PNG, from a
// render log: no ToolResult is raised; the picture's bytes arrive on the
// ToolUse row once the call resolves, and on the ToolGroup that holds it.
describe('Read', () => {
  const ID = 'toolu_01WHr5AdhiDLomZZawhLdfgX'
  const READ_OUTPUT = {
    type: 'image',
    file: {
      base64: PNG,
      type: 'image/png',
      originalSize: 70,
      dimensions: { originalWidth: 1, originalHeight: 1, displayWidth: 1, displayHeight: 1 },
    },
  } as const
  const CALL = {
    tool_use_id: ID,
    tool: 'Read',
    input: { file_path: '/usr/share/pixmaps/archlinux-logo.png' },
    isRunning: false,
    isErrored: false,
    isInterrupted: false,
  } as const

  function row(props: Record<string, unknown> = {}) {
    return {
      plugin: 'images',
      surface: 'terminal',
      component: 'ToolUse',
      requestId: ID,
      viewport: { columns: 100, rows: 40 },
      props: { ...CALL, isExpanded: false, output: READ_OUTPUT, ...props },
    } as never
  }

  function group(isExpanded: boolean, call: Record<string, unknown> = {}) {
    return {
      plugin: 'images',
      surface: 'terminal',
      component: 'ToolGroup',
      requestId: 'collapsed-0d24f7f7-c8d1-4fd5-86e0-000000000000',
      viewport: { columns: 100, rows: 40 },
      props: { calls: [{ ...CALL, output: READ_OUTPUT, ...call }], isActive: false, isExpanded, isCallsOnly: true },
    } as never
  }

  async function rasters($: Engine, event: never): Promise<number> {
    const ui = await $.ui.mount(event)
    const found = await ui.findAll({ type: 'Raster' })
    await ui.unmount()
    return found.length
  }

  test('a lone Read draws under its ToolUse row once the output is there', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    expect(await rasters($, row({ input: {}, output: undefined }))).toBe(0)
    expect(await rasters($, row({ output: undefined }))).toBe(0)
    expect(await rasters($, row())).toBe(1)
  })

  test('a running, errored or interrupted Read draws nothing', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    expect(await rasters($, row({ isRunning: true }))).toBe(0)
    expect(await rasters($, row({ isErrored: true }))).toBe(0)
    expect(await rasters($, row({ isInterrupted: true }))).toBe(0)
  })

  test('in a folded group: under the group line, and not again under the row', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    expect(await rasters($, group(false, { output: undefined }))).toBe(0)
    expect(await rasters($, group(false))).toBe(1)
    expect(await rasters($, row())).toBe(0)
  })

  test('in an unfolded group: under the row, not the group', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    expect(await rasters($, group(true))).toBe(0)
    expect(await rasters($, row({ isExpanded: true }))).toBe(1)
  })

  test('a group that unfolds hands the picture back to the row', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    expect(await rasters($, group(false))).toBe(1)
    expect(await rasters($, row())).toBe(0)
    expect(await rasters($, group(true))).toBe(0)
    expect(await rasters($, row({ isExpanded: true }))).toBe(1)
  })

  test('a Bash row is left to the engine', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    const bash = row({ tool: 'Bash', input: { command: 'ls' }, output: { stdout: 'x.png', stderr: '', isImage: false } })
    expect(await rasters($, bash)).toBe(0)
  })
})

// The mod draws Images from the cached file, and raises `generation` when a
// render reports a site off screen and then back. These tests pin that
// bookkeeping only: in Claude Code 2.1.304 the rows sent no `onScreen`, and
// what brought pictures back after a scroll was the file source itself.
describe('scroll-back', () => {
  async function imageSource($: Engine, onScreen: unknown): Promise<Record<string, unknown> | undefined> {
    const ui = await $.ui.mount({ ...SENT, props: { ...SENT.props, onScreen } } as never)
    const found = await ui.find({ key: 'images-0', type: 'Image' })
    await ui.unmount()
    return found?.props.source as Record<string, unknown> | undefined
  }
  const SHOWN = { first: 0, last: 5, of: 6 }

  test('an Image reads the cached PNG file, generation 0', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], settings: { renderer: 'image' } }
    engine(on, world)
    const source = await imageSource($, SHOWN)
    expect(source?.format).toBe('png')
    expect(String(source?.file)).toMatch(/^\/run\/user\/1000\/claude-images\/[0-9a-f]+\.png$/)
    expect(source?.generation).toBe(0)
    expect(source?.png).toBeUndefined()
  })

  test('off screen, then back: the next render raises the generation once', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { renderer: 'image' } })
    expect((await imageSource($, SHOWN))?.generation).toBe(0)
    expect((await imageSource($, { first: 2, last: 5, of: 6 }))?.generation).toBe(0)
    expect((await imageSource($, null))?.generation).toBe(0)
    expect((await imageSource($, SHOWN))?.generation).toBe(1)
    expect((await imageSource($, SHOWN))?.generation).toBe(1)
    expect((await imageSource($, undefined))?.generation).toBe(1)
    expect((await imageSource($, null))?.generation).toBe(1)
    expect((await imageSource($, SHOWN))?.generation).toBe(2)
  })

  test('over ssh: the PNG travels as bytes', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { renderer: 'image' }, env: { SSH_CONNECTION: '10.0.0.2 5000 10.0.0.1 22' } })
    const source = await imageSource($, SHOWN)
    expect(source?.png).toBe(PNG)
    expect(source?.file).toBeUndefined()
  })

  test('the render log says where the site is', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [], debug: true }
    mock.clock(on)
    engine(on, world)
    await imageSource($, null)
    const logged = world.writes?.filter(([path]) => path.endsWith('/render.log')).at(-1)?.[1] ?? ''
    expect(logged).toContain(' ToolResult tu-1 onScreen=null ')
  })
})

// An installed plugin's `$.fs` writes did not reach the cache directory; the
// mod writes through a child process instead, and says once when that fails.
describe('cache writes', () => {
  const ID = 'toolu_cache'
  const READ = {
    plugin: 'images',
    surface: 'terminal',
    component: 'ToolUse',
    requestId: ID,
    viewport: { columns: 100, rows: 40 },
    props: {
      tool_use_id: ID,
      tool: 'Read',
      input: { file_path: '/pics/a.png' },
      isRunning: false,
      isErrored: false,
      isInterrupted: false,
      isExpanded: false,
      output: { type: 'image', file: { base64: PNG, type: 'image/png', originalSize: 70 } },
    },
  } as never

  test('a Read writes its bytes to the cache through sh, not $.fs, and draws', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [], logs: [] }
    engine(on, world)
    const ui = await $.ui.mount(READ)
    expect(await ui.find({ type: 'Raster' })).toBeDefined()
    await ui.unmount()
    const [path, text] = world.writes?.find(([p]) => p.endsWith('.uri')) ?? ['', '']
    expect(path).toMatch(/^\/run\/user\/1000\/claude-images\/[0-9a-f]+\.uri$/)
    expect(text).toBe(`data:image/png;base64,${PNG}`)
    expect(world.logs).toEqual([])
  })

  test('a cache that takes no write: a dim note at the site, one log line for the session', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [], logs: [], unwritable: true }
    engine(on, world)
    for (let n = 0; n < 2; n++) {
      const ui = await $.ui.mount(READ)
      expect(await ui.find({ type: 'Raster' })).toBeUndefined()
      expect(await ui.find({ text: /cache directory took no write/ })).toBeDefined()
      expect(await ui.find({ text: 'engine-row' })).toBeDefined()
      await ui.unmount()
    }
    expect(world.logs).toHaveLength(1)
    expect(world.logs?.[0]).toBe("could not write an image to the cache directory; see the README's Requirements")
  })

  test('the render log appends through sh; one log line when it cannot', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [], logs: [], debug: true, unwritable: true }
    mock.clock(on)
    engine(on, world)
    await drawn($)
    await drawn($)
    expect(world.logs).toEqual(["could not write render.log to the cache directory; see the README's Requirements"])
  })
})

describe('children', () => {
  test('renders in the same millisecond each log their line; none is called a failure', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], writes: [], logs: [], debug: true, oneAtATime: true }
    mock.clock(on)
    engine(on, world)
    const sites = ['a', 'b', 'c', 'd', 'e'].map(id => ({ ...SENT, requestId: `tu-${id}`, props: { ...SENT.props, tool_use_id: `tu-${id}` } }))
    await Promise.all(
      sites.map(async site => {
        const ui = await $.ui.mount(site as never)
        await ui.unmount()
      }),
    )
    const lines = (world.writes ?? []).filter(([path]) => path.endsWith('/render.log')).map(([, text]) => text)
    for (const id of ['a', 'b', 'c', 'd', 'e']) expect(lines.some(line => line.includes(` ToolResult tu-${id} `))).toBe(true)
    expect(world.logs).toEqual([])
  })
  test('first renders in the same millisecond each draw, with no note', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], oneAtATime: true }
    engine(on, world)
    const sites = ['a', 'b', 'c', 'd', 'e'].map(id => ({ ...SENT, requestId: `tu-${id}`, props: { ...SENT.props, tool_use_id: `tu-${id}` } }))
    const found = await Promise.all(
      sites.map(async site => {
        const ui = await $.ui.mount(site as never)
        const seen = { raster: await ui.find({ type: 'Raster' }), note: await ui.find({ text: /^images:/ }) }
        await ui.unmount()
        return seen
      }),
    )
    for (const seen of found) {
      expect(seen.raster).toBeDefined()
      expect(seen.note).toBeUndefined()
    }
  })

  test('a session whose working directory is gone still draws', async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], cwdGone: true })
    const found = await drawn($)
    expect(found.raster).toBeDefined()
    expect(found.note).toBeUndefined()
  })

  test('a cache directory that could not be made is tried again by the next picture', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], mkdirFailures: 1 }
    engine(on, world)
    const ui = await $.ui.mount(SENT as never)
    expect(await ui.find({ text: /no private cache directory/ })).toBeDefined()
    await ui.unmount()

    expect((await drawn($)).raster).toBeDefined()
  })
})

describe('options from ways settings', () => {
  test('the settings JSON gives each option; a missing or wrong-kind key takes its default', () => {
    expect(parseOptions(settingsJson({}))).toEqual(DEFAULT_OPTIONS)
    expect(
      parseOptions(settingsJson({ renderer: 'halfblock', scale: 2, show_captions: false, show_paths_in_replies: false, steer_to_send_user_file: false })),
    ).toEqual({ renderer: 'halfblock', scale: 2, showCaptions: false, showPathsInReplies: false, steerToSendUserFile: false })
    expect(parseOptions(settingsJson({ renderer: 'sixel', scale: 99, show_captions: 'no' }))).toMatchObject({ renderer: 'auto', scale: 4, showCaptions: true })
    expect(parseOptions('mods.images.scale=2')).toBe('printed no settings JSON')
    expect(parseOptions('{"view":"effective"}')).toBe('printed settings JSON without its keys')
  })

  test('the read names the project, whose settings win over the user and target files', () => {
    expect(optionsArgv('/work/proj')).toEqual(['ways', 'settings', 'list', 'mods.images', '--json', '--effective', '--project', '/work/proj'])
    expect(optionsArgv(undefined)).toEqual(['ways', 'settings', 'list', 'mods.images', '--json', '--effective'])
  })

  test('an interactive session reads them once at its start', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], ways: [] }
    mock.clock(on)
    engine(on, world)
    await $.session.start({ cwd: '/work/proj', surface: 'terminal', isInteractive: true })
    expect(world.ways).toEqual([['ways', 'settings', 'list', 'mods.images', '--json', '--effective', '--project', '/work/proj']])
  })

  test('a headless session reads nothing at its start', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], ways: [] }
    engine(on, world)
    await $.session.start({ cwd: '/work/proj', surface: null, isInteractive: false })
    expect(world.ways).toEqual([])
  })

  test('a change reaches the next picture once the last read is over 5 s old, with no restart', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], ways: [], settings: { renderer: 'halfblock' }, dims: '320 100' }
    const clock = mock.clock(on)
    engine(on, world)
    const columns = async () => {
      const ui = await $.ui.mount(SENT as never)
      const found = await ui.find({ type: 'Raster' })
      await ui.unmount()
      return found?.props.columns
    }
    expect(await columns()).toBe(94)
    world.settings = { renderer: 'halfblock', scale: 0.25 }
    await clock.advance(1_000)
    expect(await columns()).toBe(94)
    expect(world.ways).toHaveLength(1)
    await clock.advance(5_000)
    // 320 pixels at a quarter: 80 columns.
    expect(await columns()).toBe(80)
    expect(world.ways).toHaveLength(2)
  })

  test('a failing ways keeps the last good options and logs one debug line per reason', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], ways: [], logs: [], debugLines: [], settings: { renderer: 'image' } }
    const clock = mock.clock(on)
    engine(on, world)
    expect((await drawn($)).image).toBeDefined()
    world.settings = 'fail'
    for (let n = 0; n < 3; n++) {
      await clock.advance(6_000)
      expect((await drawn($)).image).toBeDefined()
    }
    world.settings = 'garbage'
    await clock.advance(6_000)
    expect((await drawn($)).image).toBeDefined()
    expect(world.ways).toHaveLength(5)
    expect(world.debugLines).toHaveLength(2)
    expect(world.debugLines?.[0]).toMatch(/^images: kept the options as they were: `ways settings list mods\.images .*` exited 2$/)
    expect(world.debugLines?.[1]).toBe('images: kept the options as they were: printed no settings JSON')
    expect(world.logs).toEqual([])
  })

  test('with no good read yet, a failing ways leaves the defaults', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], debugLines: [], settings: 'fail' }
    engine(on, world)
    const found = await drawn($)
    expect(found.raster).toBeDefined()
    expect(found.probe).toBeDefined()
    expect(world.debugLines).toHaveLength(1)
  })
})

describe('cell size by host', () => {
  test('only Linux runs the probe', () => {
    expect(probesCells('Linux\n')).toBe(true)
    expect(probesCells('Darwin\n')).toBe(false)
    expect(probesCells('FreeBSD')).toBe(false)
  })

  test('on a Mac no python3 runs, and pictures are sized from 10x20 cells', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], uname: 'Darwin', winsize: '40 100 800 640' }
    engine(on, world)
    const ui = await $.ui.mount(SENT as never)
    // 24 columns of 10 pixels show 4 pixels at 6000%, whatever the terminal would report.
    expect(await ui.find({ text: ' · 4×4 · 6000%' })).toBeDefined()
    await ui.unmount()
    expect(world.ran.some(argv => argv[0] === 'python3')).toBe(false)
    expect(world.ran.filter(argv => argv[0] === 'uname')).toHaveLength(1)
  })

  test('on Linux the probe runs and its cells size the picture', async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [], winsize: '40 100 800 640' }
    engine(on, world)
    const ui = await $.ui.mount(SENT as never)
    expect(await ui.find({ text: ' · 4×4 · 4800%' })).toBeDefined()
    await ui.unmount()
    expect(world.ran.some(argv => argv[0] === 'python3')).toBe(true)
  })
})

// The images side of the frame contract (picture-frame.ts): beneath a plain
// row, or inside the frame the row beneath the mod already is, as it is when
// the mod sits above ways-theme in the chain.
describe('the frame contract', () => {
  const WANT = { columns: 100, cellWidth: 10, href: 'file:///pics/cat.png', name: 'cat.png' }

  test('a frame reads as its border, padding and margin', () => {
    expect(frameInset({ type: 'Box', props: { borderStyle: 'round', paddingX: 1 } })).toBe(4)
    expect(frameInset({ type: 'Box', props: { borderStyle: 'single', paddingLeft: 2, marginRight: 1 } })).toBe(5)
    expect(frameInset({ type: 'Box', props: { paddingX: 1 } })).toBeNull()
    expect(frameInset({ type: 'Text', props: {} })).toBeNull()
  })

  for (const renderer of ['halfblock', 'image'] as const) {
    test(`${renderer}: under a plain row, the picture and its caption keep the contract`, async ($, on) => {
      engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { renderer }, dims: '1020 300' })
      const ui = await $.ui.mount(SENT as never)
      const tree = await ui.drawn()
      await ui.unmount()
      expect(contractBreaches(tree, WANT)).toEqual([])
    })

    test(`${renderer}: in a frame, the picture goes inside it, sized to its room, caption and all`, async ($, on) => {
      engine(on, { hasMagick: true, blit: undefined, ran: [], settings: { renderer }, dims: '1020 300', frame: true })
      const ui = await $.ui.mount(SENT as never)
      const tree = (await ui.drawn()) as { type?: unknown; props?: Record<string, unknown> }
      const found = await ui.find({ type: renderer === 'image' ? 'Image' : 'Raster' })
      expect(await ui.find({ text: 'engine-row' })).toBeDefined()
      await ui.unmount()
      // The frame stays the root: one border, the pictures within it.
      expect(tree.type).toBe('Box')
      expect(tree.props?.borderStyle).toBe('round')
      expect(contractBreaches(tree, { ...WANT, isFramed: true })).toEqual([])
      // 100 columns, less the frame's 4, the indent of 5 and one spare.
      expect(found?.props.columns).toBe(90)
    })
  }

  test('the contract check fails a picture drawn past the frame (control)', () => {
    const raster = { type: 'Raster', props: { columns: 98, rows: 1, cells: btoa('x'.repeat(98 * 12)) } }
    const caption = { type: 'Box', children: [{ type: 'Link', props: { href: WANT.href, label: WANT.name } }, { type: 'Text', props: {}, children: [' · 1020×300 · 96%'] }] }
    const site = { type: 'Box', props: { key: 'images-picture-0' }, children: [raster, caption] }
    const tree = { type: 'Box', props: { borderStyle: 'round', paddingX: 1 }, children: [site] }
    expect(contractBreaches(tree, { ...WANT, isFramed: true })).toEqual(['images-picture-0: 98 columns inside 4 of insets overrun 100'])
  })
})

describe('what the mod hooks reaches it on every host', () => {
  test("the one hook an organization's policy plugin passes over is the prompt section, and none is classic", () => {
    const regs = registrationsOf(register as never)
    // The recording saw the module's hooks: the control for the lists below.
    expect(regs.map(r => r.pattern)).toContain('ui.render')
    expect(regs.map(r => r.pattern)).toContain('session.start')
    expect(regs.filter(r => r.pattern.startsWith('classic.'))).toEqual([])
    // Where the policy plugin sits, the steer section is left out; every picture still draws.
    expect(passedOverHits(regs)).toEqual(['prompt.compose'])
  })
})
