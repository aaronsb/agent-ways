// Pure helpers for the images mod: renderer choice, cell sizing, half-block
// packing, and finding image paths in tool calls and reply text. Nothing here
// touches `$`, so the tests drive it directly.

/** What the `renderer` option holds. */
export type RendererOption = 'auto' | 'image' | 'halfblock'

/** What a site draws with: the terminal's `Image` or a half-block `Raster`. */
export type Renderer = 'image' | 'halfblock'

export const RENDERER_OPTIONS: readonly RendererOption[] = ['auto', 'image', 'halfblock']

/** Widest picture, in columns, whatever the transcript allows. */
export const MAX_COLUMNS = 72
/** Tallest picture, in rows. */
export const MAX_ROWS = 30
/** Columns the engine's own row indent takes before a tool result's body. */
export const TOOL_INDENT = 5
/** Rough pixel width of a terminal cell, used to keep `Image` from upscaling. */
export const CELL_PIXEL_WIDTH = 8
/** Pixel box per cell a PNG for `Image` is shrunk to fit (never enlarged). */
export const IMAGE_PIXELS_PER_CELL = { width: 10, height: 20 } as const

/** The terminal's default colour in a Raster cell. */
export const DEFAULT_COLOR = 0x01000000
export const UPPER_HALF = 0x2580
export const LOWER_HALF = 0x2584
export const SPACE = 0x20

/** A 1x1 transparent PNG: the probe's picture. */
export const PROBE_PNG =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVQI12NgAAIAAAUAAeImBZsAAAAASUVORK5CYII='

export function parseRendererOption(value: unknown): RendererOption {
  return RENDERER_OPTIONS.includes(value as RendererOption) ? (value as RendererOption) : 'auto'
}

/**
 * Which renderer a site uses. `capability` is Claude Code's own answer to
 * whether this terminal draws kitty graphics with Unicode placeholders, as
 * the probe learned it: `null` while unknown. Unknown draws half-blocks, which
 * every terminal shows, so a wrong guess never leaves only an alt line.
 */
export function chooseRenderer(option: RendererOption, capability: boolean | null): Renderer {
  if (option === 'image' || option === 'halfblock') return option
  return capability === true ? 'image' : 'halfblock'
}

/** Whether a site should carry the capability probe. */
export function needsProbe(option: RendererOption, capability: boolean | null): boolean {
  return option === 'auto' && capability === null
}

/**
 * Reads `$.ui.blit`'s answer for the probe Image: taken means the terminal
 * draws the picture; a refusal naming the alt means it draws only the alt.
 * Any other refusal (not mounted yet, no surface) settles nothing.
 */
export function probeVerdict(deny: string | undefined): boolean | null {
  if (deny === undefined) return true
  return /\balt\b|placeholder/i.test(deny) ? false : null
}

export type CellFit = {
  columns: number
  rows: number
  /** Pixel size the source is resized to for this renderer. */
  pixelWidth: number
  pixelHeight: number
}

/**
 * Sizes a `width` x `height` picture in cells. A cell is taken as 1:2 (w:h),
 * so a half-block cell holds two square pixels stacked. The width is the
 * least of `MAX_COLUMNS`, the room the transcript leaves, and the picture's
 * natural width (one column per pixel for half-blocks, one per
 * `CELL_PIXEL_WIDTH` pixels for `Image`), so a small picture is not enlarged.
 * A picture too tall for `maxRows` is narrowed to keep its aspect.
 */
export function fitCells(args: {
  width: number
  height: number
  renderer: Renderer
  availableColumns: number
  maxColumns?: number
  maxRows?: number
}): CellFit {
  const { width, height, renderer } = args
  const maxColumns = Math.max(1, Math.min(args.maxColumns ?? MAX_COLUMNS, Math.floor(args.availableColumns)))
  const maxRows = Math.max(1, args.maxRows ?? MAX_ROWS)
  const w = Math.max(1, width)
  const h = Math.max(1, height)
  const natural = renderer === 'halfblock' ? w : Math.ceil(w / CELL_PIXEL_WIDTH)
  let columns = Math.max(1, Math.min(maxColumns, natural))
  let rows = Math.max(1, Math.round((columns * h) / w / 2))
  if (rows > maxRows) {
    rows = maxRows
    columns = Math.max(1, Math.min(columns, Math.round((rows * 2 * w) / h)))
  }
  if (renderer === 'halfblock') {
    return { columns, rows, pixelWidth: columns, pixelHeight: rows * 2 }
  }
  return {
    columns,
    rows,
    pixelWidth: Math.min(w, columns * IMAGE_PIXELS_PER_CELL.width),
    pixelHeight: Math.min(h, rows * IMAGE_PIXELS_PER_CELL.height),
  }
}

/**
 * Packs `width` x `height` RGBA pixels into Raster cells, two pixel rows per
 * cell row: `▀` with the top pixel as foreground and the bottom one as
 * background. A pixel under half alpha is the terminal's default colour; a
 * cell whose top alone is clear draws `▄` in the bottom colour, and a cell
 * with both clear is a blank. An odd last pixel row stands over a clear one.
 */
export function packHalfBlocks(rgba: Uint8Array, width: number, height: number): Uint32Array {
  const rows = Math.ceil(height / 2)
  const cells = new Uint32Array(width * rows * 3)
  const pixel = (x: number, y: number): number | null => {
    if (y >= height) return null
    const i = (y * width + x) * 4
    if ((rgba[i + 3] ?? 0) < 128) return null
    return ((rgba[i] ?? 0) << 16) | ((rgba[i + 1] ?? 0) << 8) | (rgba[i + 2] ?? 0)
  }
  for (let r = 0; r < rows; r++) {
    for (let x = 0; x < width; x++) {
      const top = pixel(x, r * 2)
      const bottom = pixel(x, r * 2 + 1)
      const at = (r * width + x) * 3
      if (top !== null) {
        cells[at] = UPPER_HALF
        cells[at + 1] = top
        cells[at + 2] = bottom ?? DEFAULT_COLOR
      } else if (bottom !== null) {
        cells[at] = LOWER_HALF
        cells[at + 1] = bottom
        cells[at + 2] = DEFAULT_COLOR
      } else {
        cells[at] = SPACE
        cells[at + 1] = DEFAULT_COLOR
        cells[at + 2] = DEFAULT_COLOR
      }
    }
  }
  return cells
}

/** The Raster `cells` prop: little-endian u32s, padded base64. */
export function cellsToBase64(cells: Uint32Array): string {
  const bytes = new Uint8Array(cells.length * 4)
  const view = new DataView(bytes.buffer)
  cells.forEach((word, i) => view.setUint32(i * 4, word >>> 0, true))
  return toBase64(bytes)
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

/** Standard padded base64 of `bytes`. */
export function toBase64(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i] ?? 0
    const b = bytes[i + 1]
    const c = bytes[i + 2]
    const n = (a << 16) | ((b ?? 0) << 8) | (c ?? 0)
    out += B64[(n >> 18) & 63]! + B64[(n >> 12) & 63]!
    out += b === undefined ? '=' : B64[(n >> 6) & 63]!
    out += c === undefined ? '=' : B64[n & 63]!
  }
  return out
}

/** Bytes of standard base64 text; whitespace ignored. */
export function fromBase64(text: string): Uint8Array {
  const clean = text.replace(/[^A-Za-z0-9+/]/g, '')
  const out = new Uint8Array(Math.floor((clean.length * 3) / 4))
  let o = 0
  for (let i = 0; i < clean.length; i += 4) {
    const n =
      (B64.indexOf(clean[i] ?? 'A') << 18) |
      (B64.indexOf(clean[i + 1] ?? 'A') << 12) |
      ((clean[i + 2] === undefined ? 0 : B64.indexOf(clean[i + 2]!)) << 6) |
      (clean[i + 3] === undefined ? 0 : B64.indexOf(clean[i + 3]!))
    if (o < out.length) out[o++] = (n >> 16) & 255
    if (o < out.length) out[o++] = (n >> 8) & 255
    if (o < out.length) out[o++] = n & 255
  }
  return out
}

const IMAGE_EXTENSION = /\.(png|jpe?g|gif|webp)$/i

export function isImagePath(path: string): boolean {
  return IMAGE_EXTENSION.test(path)
}

export type ReplyImage = { file: string; alt: string }

/** Most pictures drawn under one reply. */
export const REPLY_IMAGES_MAX = 4

const MARKDOWN_IMAGE = /!\[([^\]]*)\]\(\s*(?:<([^>]+)>|([^)\s]+))(?:\s+"[^"]*")?\s*\)/g
// A bare path: starts a word or follows a backtick, quote or opening
// bracket; ends at its image extension, before trailing punctuation and the
// end of the word. No spaces, quotes, backticks or brackets inside.
const BARE_PATH = /(?<=^|[\s`'"(\[<])(~\/|\/)[^\s`'"<>()[\]]*?\.(?:png|jpe?g|gif|webp)(?=[.,;:!?]*(?:$|[\s`'")\]>]))/gi

/**
 * The image paths a reply names, in the order it names them, each once.
 *
 * Markdown images `![alt](/abs/x.png)` or `![alt](<~/a b.png> "title")`
 * count outside inline code; with `bare`, so does an absolute or `~/` path
 * written plainly, in backticks or in quotes (`saved it to /tmp/chart.png.`).
 * Fenced code blocks are skipped. `home` expands `~`; with no home a `~`
 * path is dropped.
 */
export function replyImages(text: string, home: string | undefined, options: { bare: boolean }): ReplyImage[] {
  const found: Array<ReplyImage & { at: number }> = []
  let fence: string | null = null
  let offset = 0
  for (const line of text.split('\n')) {
    const at = offset
    offset += line.length + 1
    const marker = /^\s{0,3}(`{3,}|~{3,})/.exec(line)?.[1]
    if (fence !== null) {
      if (marker !== undefined && marker[0] === fence[0] && marker.length >= fence.length) fence = null
      continue
    }
    if (marker !== undefined) {
      fence = marker
      continue
    }
    // Code spans blanked to keep positions: markdown in code is not an image.
    const prose = line.replace(/(`+)[^`]*?\1/g, span => ' '.repeat(span.length))
    for (const m of prose.matchAll(MARKDOWN_IMAGE)) {
      const file = expandHome((m[2] ?? m[3] ?? '').trim(), home)
      if (file !== undefined && isImagePath(file)) found.push({ file, alt: m[1] ?? '', at: at + (m.index ?? 0) })
    }
    if (!options.bare) continue
    for (const m of line.matchAll(BARE_PATH)) {
      const file = expandHome(m[0], home)
      if (file !== undefined) found.push({ file, alt: basename(file), at: at + (m.index ?? 0) })
    }
  }
  found.sort((a, b) => a.at - b.at)
  const seen = new Set<string>()
  const once: ReplyImage[] = []
  for (const { file, alt } of found) {
    if (seen.has(file)) continue
    seen.add(file)
    once.push({ file, alt })
  }
  return once
}

/** The first `max` of `refs` whose file `isFile` confirms, in order. */
export async function takeExisting<T extends { file: string }>(
  refs: readonly T[],
  isFile: (path: string) => Promise<boolean>,
  max: number,
): Promise<T[]> {
  const kept: T[] = []
  for (const ref of refs) {
    if (kept.length >= max) break
    if (await isFile(ref.file)) kept.push(ref)
  }
  return kept
}

/** An absolute path, `~` expanded; undefined for anything else. */
export function expandHome(path: string, home: string | undefined): string | undefined {
  if (path.startsWith('/')) return path
  if (path === '~' || path.startsWith('~/')) {
    return home === undefined || home === '' ? undefined : home.replace(/\/$/, '') + path.slice(1)
  }
  return undefined
}

/**
 * A picture to draw: a file on disk, or base64 bytes the engine holds (what a
 * Read showed the model, an image pasted with a prompt), named by a stable id.
 */
export type ImageRef =
  | { file: string; alt: string }
  | { data: string; mime: string; id: string; alt: string }

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null

const IMAGE_MIME = /^image\/(png|jpeg|gif|webp)$/

/**
 * The pictures a finished tool call showed: the image a Read handed the
 * model (its own bytes, so the row shows what Claude saw), or the image
 * attachments of a SendUserFile. `id` is the call's tool_use_id.
 */
export function toolImages(tool: string, input: unknown, output: unknown, id: string): ImageRef[] {
  if (tool === 'Read') {
    const file = isObj(output) && output.type === 'image' && isObj(output.file) ? output.file : null
    if (file === null || typeof file.base64 !== 'string' || typeof file.type !== 'string') return []
    if (!IMAGE_MIME.test(file.type)) return []
    const path = isObj(input) && typeof input.file_path === 'string' ? input.file_path : 'image'
    return [{ data: file.base64, mime: file.type, id, alt: basename(path) }]
  }
  if (tool === 'SendUserFile') {
    const attachments = isObj(output) && Array.isArray(output.attachments) ? output.attachments : null
    const paths =
      attachments !== null
        ? attachments
            .filter(isObj)
            .filter(a => typeof a.path === 'string' && (a.isImage === true || isImagePath(a.path)))
            .map(a => a.path as string)
        : (isObj(input) && Array.isArray(input.files) ? input.files : []).filter(
            (f): f is string => typeof f === 'string' && isImagePath(f),
          )
    return paths.filter(p => p.startsWith('/')).map(file => ({ file, alt: basename(file) }))
  }
  return []
}

/**
 * The images in a prompt row's content blocks (`{ type: 'image', source: {
 * type: 'base64', media_type, data } }`), each named `<uuid>-<n>`.
 */
export function contentImages(content: readonly unknown[], uuid: string): ImageRef[] {
  const found: ImageRef[] = []
  content.forEach((block, n) => {
    if (!isObj(block) || block.type !== 'image' || !isObj(block.source)) return
    const { type, media_type: mime, data } = block.source
    if (type !== 'base64' || typeof mime !== 'string' || typeof data !== 'string') return
    if (!IMAGE_MIME.test(mime)) return
    found.push({ data, mime, id: `${uuid}-${n}`, alt: `pasted image ${found.length + 1}` })
  })
  return found
}

/** The `inline:` file ImageMagick reads base64 bytes from: a data URI. */
export function dataUri(mime: string, base64: string): string {
  return `data:${mime};base64,${base64}`
}

/** `%w %h` as ImageMagick's `info:` prints it. */
export function parseDimensions(stdout: string): { width: number; height: number } | null {
  const m = /^\s*(\d+)\s+(\d+)/.exec(stdout)
  if (m === null) return null
  const width = Number(m[1])
  const height = Number(m[2])
  return width > 0 && height > 0 ? { width, height } : null
}

/** argv to read `src`'s first frame's size, upright. */
export function identifyArgv(tool: string, src: string): string[] {
  return [tool, `${src}[0]`, '-auto-orient', '-format', '%w %h', 'info:']
}

/** argv to write `src`'s first frame as exact-size raw RGBA for half-blocks. */
export function rgbaArgv(tool: string, src: string, fit: CellFit, out: string): string[] {
  return [tool, `${src}[0]`, '-auto-orient', '-resize', `${fit.pixelWidth}x${fit.pixelHeight}!`, '-depth', '8', `rgba:${out}`]
}

/** argv to write `src`'s first frame as a PNG no larger than the cell box. */
export function pngArgv(tool: string, src: string, fit: CellFit, out: string): string[] {
  return [tool, `${src}[0]`, '-auto-orient', '-resize', `${fit.pixelWidth}x${fit.pixelHeight}>`, '-strip', `png:${out}`]
}

/** A short stable hash (FNV-1a, hex) for cache file names. */
export function hashKey(text: string): string {
  let h = 0x811c9dc5
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i)
    h = Math.imul(h, 0x01000193) >>> 0
  }
  return h.toString(16).padStart(8, '0')
}

/** The basename of a path, for an alt text. */
export function basename(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1) || path
}

/** Columns a picture may take at a site indented by `indent`. */
export function availableColumns(viewportColumns: number | undefined, indent: number): number {
  return Math.max(1, (viewportColumns ?? 80) - indent - 1)
}

/**
 * The shape of a value for the render log: object keys with their shapes,
 * arrays by length and first element, strings by length only, so no image
 * bytes or text are written; booleans as their value, which says which way a
 * row is drawn. Nesting stops at `depth`.
 */
export function describeShape(value: unknown, depth = 3): string {
  if (value === null) return 'null'
  if (typeof value === 'string') return `str(${value.length})`
  if (typeof value === 'boolean') return String(value)
  if (typeof value !== 'object') return typeof value
  if (Array.isArray(value)) {
    if (value.length === 0) return '[]'
    return depth <= 0 ? `[${value.length}]` : `[${value.length} x ${describeShape(value[0], depth - 1)}]`
  }
  const keys = Object.keys(value)
  if (depth <= 0) return `{${keys.join(',')}}`
  return `{${keys.map(k => `${k}:${describeShape((value as Record<string, unknown>)[k], depth - 1)}`).join(', ')}}`
}
