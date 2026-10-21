import { describe, expect, mock, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On, ProcessRunResult } from 'claude-code'

import {
  DEFAULT_COLOR,
  LOWER_HALF,
  MAX_COLUMNS,
  MAX_ROWS,
  SPACE,
  UPPER_HALF,
  availableColumns,
  cellsToBase64,
  chooseRenderer,
  contentImages,
  fitCells,
  fromBase64,
  REPLY_IMAGES_MAX,
  needsProbe,
  packHalfBlocks,
  parseDimensions,
  parseRendererOption,
  probeVerdict,
  replyImages,
  rgbaArgv,
  takeExisting,
  toBase64,
  toolImages,
} from '../hooks/lib.ts'

describe('sizing', () => {
  test('a wide picture fills the column cap, its aspect kept', () => {
    // 1600x400: 72 columns; 72 * 400/1600 / 2 = 9 rows.
    expect(fitCells({ width: 1600, height: 400, renderer: 'halfblock', availableColumns: 200 })).toEqual({
      columns: 72,
      rows: 9,
      pixelWidth: 72,
      pixelHeight: 18,
    })
  })

  test('a tall picture is held to the row cap and narrowed to keep its aspect', () => {
    // 400x1600: 72 columns would be 144 rows; 30 rows gives 30*2*400/1600 = 15 columns.
    const fit = fitCells({ width: 400, height: 1600, renderer: 'halfblock', availableColumns: 200 })
    expect(fit).toEqual({ columns: 15, rows: MAX_ROWS, pixelWidth: 15, pixelHeight: 60 })
  })

  test('a tiny picture is not enlarged: one column per pixel for half-blocks', () => {
    expect(fitCells({ width: 4, height: 4, renderer: 'halfblock', availableColumns: 200 })).toEqual({
      columns: 4,
      rows: 2,
      pixelWidth: 4,
      pixelHeight: 4,
    })
    // A 1x1 picture still takes one cell.
    expect(fitCells({ width: 1, height: 1, renderer: 'halfblock', availableColumns: 200 })).toMatchObject({
      columns: 1,
      rows: 1,
    })
  })

  test('a tiny picture under Image takes one column per eight pixels', () => {
    const fit = fitCells({ width: 16, height: 16, renderer: 'image', availableColumns: 200 })
    expect(fit.columns).toBe(2)
    expect(fit.rows).toBe(1)
    // The PNG is never resized up past its own size.
    expect(fit.pixelWidth).toBe(16)
    expect(fit.pixelHeight).toBe(16)
  })

  test('a huge picture is capped in cells, and its PNG box in pixels', () => {
    const fit = fitCells({ width: 8000, height: 6000, renderer: 'image', availableColumns: 200 })
    expect(fit.columns).toBe(MAX_COLUMNS)
    expect(fit.rows).toBe(27) // 72 * 6000/8000 / 2
    expect(fit.pixelWidth).toBe(720)
    expect(fit.pixelHeight).toBe(540)
  })

  test('the transcript width clamps the picture below the column cap', () => {
    const room = availableColumns(50, 5)
    expect(room).toBe(44)
    const fit = fitCells({ width: 1000, height: 500, renderer: 'halfblock', availableColumns: room })
    expect(fit.columns).toBe(44)
    expect(fit.rows).toBe(11)
    // An unmeasured surface is taken as 80 columns.
    expect(availableColumns(undefined, 5)).toBe(74)
    // A sliver of room still draws one column.
    expect(fitCells({ width: 100, height: 100, renderer: 'halfblock', availableColumns: 0 }).columns).toBe(1)
  })

  test('the ImageMagick argv resizes exactly for half-blocks, and reads the first frame', () => {
    const fit = fitCells({ width: 1600, height: 400, renderer: 'halfblock', availableColumns: 200 })
    expect(rgbaArgv('magick', '/p/a.gif', fit, '/c/k.rgba')).toEqual([
      'magick',
      '/p/a.gif[0]',
      '-auto-orient',
      '-resize',
      '72x18!',
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
    expect(toolImages('SendUserFile', input, undefined, 't')).toEqual([{ file: '/out/c.webp', alt: 'c.webp' }])
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

/** 4x4 RGBA: red over blue, row by row. */
const RGBA = toBase64(
  Uint8Array.from({ length: 64 }, (_, i) => {
    const y = Math.floor(i / 16)
    const c = i % 4
    return c === 3 ? 255 : y % 2 === 0 ? (c === 0 ? 255 : 0) : c === 2 ? 255 : 0
  }),
)
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
  /** More environment, over the defaults. */
  env?: Record<string, string>
}

function engine(on: On, world: World) {
  mock.env(on, { HOME: '/home/t', XDG_RUNTIME_DIR: '/run/user/1000', USER: 't', ...world.env })
  const written = new Set<string>()
  let inFlight = 0
  on('process.run', async ($, e) => {
    world.ran.push([...e.argv])
    const [tool] = e.argv
    if (world.cwdGone === true && e.init?.cwd === undefined) throw new Error(`spawn ${tool} ENOENT`)
    if (world.oneAtATime === true) {
      if (inFlight > 0) throw new Error('process.run refused: another child is running')
      inFlight++
      await new Promise(resolve => setTimeout(resolve, 2))
      inFlight--
    }
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
      if (e.argv[1] === '-version') return { value: ok('Version: ImageMagick 7.1') }
      if (e.argv.at(-1) === 'info:') return { value: ok('4 4') }
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
    return { value: { base64: path.endsWith('.png') ? PNG : RGBA } }
  })
  on('ui.log', ($, e) => {
    world.logs?.push(String((e as { text?: unknown }).text ?? ''))
    return { value: undefined }
  })
  on('ui.blit', () => ({ value: world.blit === undefined ? {} : { deny: world.blit } }))
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    return h(Text, {}, 'engine-row') as never
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

  test('renderer image forces the Image element', { options: { renderer: 'image' } }, async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    const found = await drawn($)
    expect(found.image).toBeDefined()
    expect(found.raster).toBeUndefined()
    expect(found.probe).toBeUndefined()
  })

  test('renderer halfblock forces the Raster, with no probe', { options: { renderer: 'halfblock' } }, async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
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

  test('showPathsInReplies off: only markdown images draw', { options: { showPathsInReplies: false } }, async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    const ui = await $.ui.mount(REPLY as never)
    expect(await ui.find({ type: 'Raster' })).toBeUndefined()
    expect(await ui.find({ text: 'engine-row' })).toBeDefined()
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

  test('steerToSendUserFile off: no section', { options: { steerToSendUserFile: false } }, async ($, on) => {
    on('prompt.compose', () => BASE as never)
    const { sections } = await $.prompt.compose({ ...FACTS, surfaces: ['terminal'], tools: TOOLS })
    expect(sections.map(s => s.id)).toEqual(['intro'])
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

  test('an Image reads the cached PNG file, generation 0', { options: { renderer: 'image' } }, async ($, on) => {
    const world: World = { hasMagick: true, blit: undefined, ran: [] }
    engine(on, world)
    const source = await imageSource($, SHOWN)
    expect(source?.format).toBe('png')
    expect(String(source?.file)).toMatch(/^\/run\/user\/1000\/claude-images\/[0-9a-f]+\.png$/)
    expect(source?.generation).toBe(0)
    expect(source?.png).toBeUndefined()
  })

  test('off screen, then back: the next render raises the generation once', { options: { renderer: 'image' } }, async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [] })
    expect((await imageSource($, SHOWN))?.generation).toBe(0)
    expect((await imageSource($, { first: 2, last: 5, of: 6 }))?.generation).toBe(0)
    expect((await imageSource($, null))?.generation).toBe(0)
    expect((await imageSource($, SHOWN))?.generation).toBe(1)
    expect((await imageSource($, SHOWN))?.generation).toBe(1)
    expect((await imageSource($, undefined))?.generation).toBe(1)
    expect((await imageSource($, null))?.generation).toBe(1)
    expect((await imageSource($, SHOWN))?.generation).toBe(2)
  })

  test('over ssh: the PNG travels as bytes', { options: { renderer: 'image' } }, async ($, on) => {
    engine(on, { hasMagick: true, blit: undefined, ran: [], env: { SSH_CONNECTION: '10.0.0.2 5000 10.0.0.1 22' } })
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
