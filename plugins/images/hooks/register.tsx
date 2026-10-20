// The images mod: draws pictures inline in the terminal transcript where
// Claude Code would otherwise leave a path.
//
// Sites, each drawn under the engine's own row:
//   - a SendUserFile result's image attachments;
//   - a Read that returned an image, from the bytes the model was shown: under
//     its ToolUse row, or under the folded ToolGroup line the call sits in
//     (Claude Code raises no ToolResult for a Read). Each Read draws in one of
//     the two: a call whose latest group render was folded is the group's,
//     any other its row's;
//   - the images a reply names by an absolute or `~/` path, as markdown
//     images or (with `showPathsInReplies`) bare, fenced code skipped;
//   - the images pasted with a prompt, caught as the prompt row is appended.
//
// With `steerToSendUserFile` (the default), a short system-prompt section on
// the terminal surface tells the model the terminal draws images, so a request
// to see one ends with SendUserFile (or a Read, or a path) and a picture.
//
// Two renderers. Where Claude Code draws kitty graphics with Unicode
// placeholders, the terminal's `Image` element carries the pixels, read by the
// terminal from the cached PNG file (base64 over ssh, where the terminal cannot
// read this machine's files). Claude Code frees an Image's pixels when its row
// scrolls out of view and sends nothing for a source equal to the last drawn,
// so a site that comes back into view (`onScreen` null, then set) gets a new
// `generation` and the terminal reads the file again. Elsewhere
// `Image` draws only its alt, so a `Raster` of half-block cells (`▀`, top
// pixel as foreground, bottom as background) stands in. In `auto`, the choice
// follows Claude Code's own capability, learned once per session by a probe:
// a 1x1 transparent `Image` drawn beside the first picture, then swapped by
// `$.ui.blit`, which refuses an Image that draws its alt. Until the probe
// settles, pictures draw as half-blocks, which every terminal shows.
//
// ImageMagick (`magick`, or `convert`) decodes and resizes; the mod keeps its
// files in a private cache directory, and writes its own there through a child
// process (`$.process`), as ImageMagick does: an installed plugin's `$.fs`
// writes did not land there. Without ImageMagick each site draws the
// engine's row and one dim note.
//
// Render log, off unless asked for: while a file named `debug` sits in the
// cache directory, each transcript component the mod can see appends one
// line to `render.log` beside it (component, requestId, tool, and the shape
// of its props, never their bytes).

import type { Elements, EngineInterface, ProcessRunResult, Register, RenderElement, RenderNode } from 'claude-code'

import {
  PROBE_PNG,
  TOOL_INDENT,
  availableColumns,
  cellsToBase64,
  describeShape,
  chooseRenderer,
  contentImages,
  dataUri,
  fitCells,
  fromBase64,
  hashKey,
  identifyArgv,
  REPLY_IMAGES_MAX,
  needsProbe,
  packHalfBlocks,
  parseDimensions,
  parseRendererOption,
  pngArgv,
  probeVerdict,
  replyImages,
  rgbaArgv,
  takeExisting,
  toolImages,
} from './lib.ts'
import type { ImageRef, Renderer, RendererOption } from './lib.ts'

const KITTY_GRAPHICS = { plugin: 'images', key: 'kittyGraphics' } as const
const PASTED = { plugin: 'images', key: 'pasted' } as const

const PROBE_KEY = 'images-probe'
const PROBE_TRIES = 4
const PROBE_DELAY_MS = 250
/** Largest PNG an `Image` takes as bytes. */
const IMAGE_BYTES_MAX = 2 * 1024 * 1024
const MEMO_MAX = 64
/** A cache file holding base64 bytes as a data URI. */
const URI_SUFFIX = '.uri'
const RUN_TIMEOUT_MS = 15_000
/** A reply worth scanning names an image file somewhere. */
const IMAGE_MENTION = /\.(png|jpe?g|gif|webp)\b/i
/** Indent of a reply's or a prompt's body under its bullet. */
const MESSAGE_INDENT = 2
/** How long the render log's on/off answer is kept before the flag is looked for again. */
const TRACE_RECHECK_MS = 2_000
/** The components a tool call or a message can be drawn through. */
/** The system-prompt section, by whether the request offers SendUserFile. */
const STEER_ID = 'images:show-images'
const STEER_WITH_SEND =
  'This terminal draws images inline. When the user asks to see or be shown an image, send it with SendUserFile, ' +
  'which shows it here and in remote and web views. A Read of an image, or an absolute image path in your reply, also draws it.'
const STEER_WITHOUT_SEND =
  'This terminal draws images inline. When the user asks to see or be shown an image, Read it or name its absolute ' +
  'path in your reply, and it is drawn here.'
const TRACED = ['ToolUse', 'ToolResult', 'ToolGroup', 'ToolProgress', 'AssistantMessage', 'UserMessage', 'CommandOutput'] as const

type Picture =
  | { kind: 'image'; file: string; png: string | null; columns: number; rows: number; alt: string }
  | { kind: 'halfblock'; cells: string; columns: number; rows: number }
type Outcome = Picture | { kind: 'note'; text: string } | null

type Terminal = Elements['terminal']

// Per load: a reload evaluates the module again and starts these over; the
// files on disk stay.
let option: RendererOption = 'auto'
let showPathsInReplies = true
let steerToSendUserFile = true
let magick: string | null | undefined
let cacheDir: string | null | undefined
let isProbing = false
const memo = new Map<string, Picture>()
/** Per site (requestId): its Images' generation, and whether it is off screen. */
const views = new Map<string, { generation: number; hidden: boolean }>()
const VIEWS_MAX = 512
let overSsh: boolean | undefined
/** Per tool_use_id: true when the latest ToolGroup that held it was folded. */
const folded = new Map<string, boolean>()
const FOLDED_MAX = 512
let traceCheck: Promise<boolean> = Promise.resolve(false)
let traceCheckedAt = -Infinity
/** Set once a write to the cache failed and that was said, so it is said once. */
let toldUnwritable = false

async function isRegularFile($: EngineInterface, path: string): Promise<boolean> {
  const stat = await $.fs.stat(path).catch(() => null)
  return stat?.kind === 'file'
}

function run($: EngineInterface, argv: string[]): Promise<ProcessRunResult | null> {
  return $.process.run(argv, { timeoutMs: RUN_TIMEOUT_MS }).catch(() => null)
}

async function findMagick($: EngineInterface): Promise<string | null> {
  if (magick !== undefined) return magick
  for (const tool of ['magick', 'convert']) {
    const ran = await run($, [tool, '-version'])
    if (ran?.exitCode === 0 && /ImageMagick/.test(ran.stdout)) return (magick = tool)
  }
  return (magick = null)
}

// A directory of this user's alone: under XDG_RUNTIME_DIR where there is
// one, else under the temp directory with the user's name, mode 0700.
async function cacheDirectory($: EngineInterface): Promise<string | null> {
  if (cacheDir !== undefined) return cacheDir
  const runtime = await $.env.get('XDG_RUNTIME_DIR')
  const tmp = (await $.env.get('TMPDIR')) || '/tmp'
  const user = (await $.env.get('USER')) || 'user'
  const dir = runtime
    ? `${runtime.replace(/\/$/, '')}/claude-images`
    : `${tmp.replace(/\/$/, '')}/claude-images-${user}`
  const made = await run($, ['mkdir', '-p', '-m', '700', '--', dir])
  const locked = made?.exitCode === 0 ? await run($, ['chmod', '700', '--', dir]) : null
  return (cacheDir = locked?.exitCode === 0 ? dir : null)
}

/** The writes in flight, one after another: renders arrive in pairs within a millisecond. */
let writes: Promise<unknown> = Promise.resolve()

/** One `sh` run that writes `text` to `path`: its exit code, or null when the run was refused. */
async function shWrite($: EngineInterface, path: string, text: string, how: 'whole' | 'append'): Promise<number | null> {
  const script = how === 'append' ? 'cat >> "$1"' : 'cat > "$1.$$" && mv -f "$1.$$" "$1"'
  const ran = await $.process
    .run(['sh', '-c', script, 'sh', path], { stdin: text, timeoutMs: RUN_TIMEOUT_MS })
    .catch(() => null)
  return ran === null ? null : ran.exitCode
}

/**
 * Writes `text` to `path` through a child process, whole (written aside, then
 * moved into place) or appended, queued behind the writes before it. A run the
 * host refused is tried once more; false only for a non-zero exit, or a second
 * refusal.
 */
function writeFile($: EngineInterface, path: string, text: string, how: 'whole' | 'append'): Promise<boolean> {
  const done = writes.then(async () => {
    const first = await shWrite($, path, text, how)
    const code = first === null ? await shWrite($, path, text, how) : first
    return code === 0
  })
  writes = done.catch(() => undefined)
  return done
}

/** Says once per load, as a dim transcript line, that the cache took no write. */
function tellUnwritable($: EngineInterface, what: string): void {
  if (toldUnwritable) return
  toldUnwritable = true
  // $.ui.log names the plugin itself.
  $.ui.log(`could not write ${what} to the cache directory; see the README's Requirements`)
}

/** True while `<cache>/debug` exists, looked for at most every TRACE_RECHECK_MS. */
async function tracing($: EngineInterface): Promise<boolean> {
  const now = await $.clock.now()
  if (now - traceCheckedAt < TRACE_RECHECK_MS) return traceCheck
  traceCheckedAt = now
  // Renders that arrive while the flag is being looked for wait on the same look.
  traceCheck = cacheDirectory($).then(dir => (dir === null ? false : isRegularFile($, `${dir}/debug`)))
  return traceCheck
}

/** Appends one line describing a render to `<cache>/render.log`. */
async function trace($: EngineInterface, e: { component: string; requestId: string; surface: string; props: unknown }): Promise<void> {
  if (!(await tracing($))) return
  const dir = await cacheDirectory($)
  if (dir === null) return
  const log = `${dir}/render.log`
  const props = (typeof e.props === 'object' && e.props !== null ? e.props : {}) as Record<string, unknown>
  const { onScreen, ...shown } = props
  const tool = typeof props.tool === 'string' ? props.tool : Array.isArray(props.calls) ? 'calls' : '-'
  const at = new Date(await $.clock.now()).toISOString()
  const screen =
    onScreen === undefined
      ? 'absent'
      : onScreen === null
        ? 'null'
        : typeof onScreen === 'object'
          ? `${(onScreen as { first?: number }).first}-${(onScreen as { last?: number }).last}/${(onScreen as { of?: number }).of}`
          : '?'
  const line = `${at} ${e.surface} ${e.component} ${e.requestId} onScreen=${screen} tool=${tool} props=${describeShape(shown, 4)}\n`
  if (!(await writeFile($, log, line, 'append'))) tellUnwritable($, 'render.log')
}

/** Writes base64 bytes to the cache as a data URI file, once per id; null when it could not. */
async function stash($: EngineInterface, ref: Extract<ImageRef, { data: string }>, dir: string): Promise<string | null> {
  const uri = `${dir}/${hashKey(`${ref.id}|${ref.data.length}`)}${URI_SUFFIX}`
  if (await isRegularFile($, uri)) return uri
  // A write reported failed but on disk all the same still draws.
  if ((await writeFile($, uri, dataUri(ref.mime, ref.data), 'whole')) || (await isRegularFile($, uri))) return uri
  tellUnwritable($, 'an image')
  return null
}

/** The source argument ImageMagick reads, and what names its content. */
async function sourceOf(
  $: EngineInterface,
  ref: ImageRef,
  dir: string,
): Promise<{ src: string; tag: string } | 'unwritable' | null> {
  const file = 'file' in ref ? ref.file : await stash($, ref, dir)
  if (file === null) return 'unwritable'
  const stat = await $.fs.stat(file).catch(() => null)
  if (stat === null || stat.kind !== 'file') return null
  // A stashed data URI reads through ImageMagick's `inline:` coder.
  const src = file.endsWith(URI_SUFFIX) ? `inline:${file}` : file
  return { src, tag: `${file}|${stat.mtimeMs}|${stat.size}` }
}

/** True when Claude Code runs over ssh, where the terminal cannot read our files. */
async function isOverSsh($: EngineInterface): Promise<boolean> {
  if (overSsh === undefined) overSsh = Boolean((await $.env.get('SSH_CONNECTION')) || (await $.env.get('SSH_TTY')))
  return overSsh
}

/**
 * The generation a site's Images draw with: raised each time the site comes
 * back into view after a render reported it off screen (`onScreen` null), so
 * the terminal is sent the picture again. Renders that do not say where the
 * site is keep the last generation.
 */
function generationFor(requestId: string, onScreen: unknown): number {
  const view = views.get(requestId) ?? { generation: 0, hidden: false }
  if (onScreen === null) view.hidden = true
  else if (onScreen !== undefined && view.hidden) {
    view.hidden = false
    view.generation += 1
  }
  views.delete(requestId)
  views.set(requestId, view)
  if (views.size > VIEWS_MAX) views.delete(views.keys().next().value as string)
  return view.generation
}

async function picture($: EngineInterface, ref: ImageRef, renderer: Renderer, room: number): Promise<Outcome> {
  const tool = await findMagick($)
  if (tool === null) {
    return { kind: 'note', text: 'images: ImageMagick (magick or convert) is not installed; no picture drawn' }
  }
  const dir = await cacheDirectory($)
  if (dir === null) return { kind: 'note', text: 'images: no private cache directory could be made; no picture drawn' }
  const source = await sourceOf($, ref, dir)
  if (source === 'unwritable') return { kind: 'note', text: 'images: the cache directory took no write; no picture drawn' }
  if (source === null) return null

  const sized = await run($, identifyArgv(tool, source.src))
  const dims = sized?.exitCode === 0 ? parseDimensions(sized.stdout) : null
  if (dims === null) return null
  const fit = fitCells({ ...dims, renderer, availableColumns: room })

  const key = hashKey(`${source.tag}|${renderer}|${fit.columns}x${fit.rows}`)
  const held = memo.get(key)
  if (held !== undefined) return held

  let made: Picture
  if (renderer === 'image') {
    const out = `${dir}/${key}.png`
    if ((await run($, pngArgv(tool, source.src, fit, out)))?.exitCode !== 0) return null
    // The terminal reads the file itself; only over ssh do the bytes travel.
    let png: string | null = null
    if (await isOverSsh($)) {
      png = (await $.fs.read(out, { as: 'bytes' })).base64
      if ((png.length * 3) / 4 > IMAGE_BYTES_MAX) return picture($, ref, 'halfblock', room)
    }
    made = { kind: 'image', file: out, png, columns: fit.columns, rows: fit.rows, alt: `image: ${ref.alt}` }
  } else {
    const out = `${dir}/${key}.rgba`
    if ((await run($, rgbaArgv(tool, source.src, fit, out)))?.exitCode !== 0) return null
    const { base64 } = await $.fs.read(out, { as: 'bytes' })
    const rgba = fromBase64(base64)
    if (rgba.length !== fit.pixelWidth * fit.pixelHeight * 4) return null
    const cells = cellsToBase64(packHalfBlocks(rgba, fit.pixelWidth, fit.pixelHeight))
    made = { kind: 'halfblock', cells, columns: fit.columns, rows: fit.rows }
  }
  memo.set(key, made)
  if (memo.size > MEMO_MAX) memo.delete(memo.keys().next().value as string)
  return made
}

// Blits the probe Image a site drew until the answer settles: taken, the
// terminal draws kitty graphics; refused for its alt, it does not. Any other
// refusal (not mounted yet) tries again, then leaves it to the next picture.
function probe($: EngineInterface, requestId: string): void {
  if (isProbing) return
  isProbing = true
  probeAttempt($, requestId, 0)
}

function probeAttempt($: EngineInterface, requestId: string, tries: number): void {
  $.clock.after(PROBE_DELAY_MS, async () => {
    const blitted = await $.ui
      .blit({ requestId, key: PROBE_KEY, source: { png: PROBE_PNG } })
      .catch(() => ({ deny: 'failed' }))
    const verdict = probeVerdict(blitted.deny)
    if (verdict !== null) {
      await $.state.set(KITTY_GRAPHICS, verdict)
      isProbing = false
    } else if (tries + 1 < PROBE_TRIES) {
      probeAttempt($, requestId, tries + 1)
    } else {
      isProbing = false
    }
  })
}

/** The pictures for `refs` in one column; null when none could be drawn. */
async function pictures(
  $: EngineInterface,
  el: Terminal,
  requestId: string,
  refs: readonly ImageRef[],
  room: number,
  indent: number,
  generation: number,
): Promise<RenderNode | null> {
  const { Box, Image, Raster, Text } = el
  const { value: capability = null } = await $.state.get(KITTY_GRAPHICS)
  const renderer = chooseRenderer(option, capability)
  const drawn: RenderNode[] = []
  let note: string | null = null
  for (const [n, ref] of refs.entries()) {
    const made = await picture($, ref, renderer, room)
    if (made === null) continue
    if (made.kind === 'note') {
      note = made.text
      continue
    }
    drawn.push(
      made.kind === 'image' ? (
        <Image
          key={`images-${n}`}
          source={made.png === null ? { file: made.file, format: 'png', generation } : { png: made.png }}
          columns={made.columns}
          rows={made.rows}
          alt={made.alt}
        />
      ) : (
        <Raster key={`images-${n}`} cells={made.cells} columns={made.columns} rows={made.rows} />
      ),
    )
  }
  if (drawn.length > 0 && needsProbe(option, capability)) {
    drawn.push(<Image key={PROBE_KEY} source={{ png: PROBE_PNG }} columns={1} rows={1} alt=" " />)
    probe($, requestId)
  }
  if (note !== null) drawn.push(<Text dimColor>{note}</Text>)
  if (drawn.length === 0) return null
  return (
    <Box flexDirection="column" paddingLeft={indent}>
      {drawn}
    </Box>
  )
}

/** The engine's row with the pictures for `refs` under it. */
async function withPictures(
  $: EngineInterface,
  el: Terminal,
  requestId: string,
  row: Promise<RenderElement>,
  refs: readonly ImageRef[],
  viewportColumns: number | undefined,
  indent: number,
  onScreen: unknown,
): Promise<RenderElement> {
  const room = availableColumns(viewportColumns, indent)
  const generation = generationFor(requestId, onScreen)
  const [own, drawn] = await Promise.all([row, pictures($, el, requestId, refs, room, indent, generation)])
  if (drawn === null) return own
  const { Box } = el
  return (
    <Box flexDirection="column">
      {own}
      {drawn}
    </Box>
  )
}

export const register: Register = (on, options) => {
  option = parseRendererOption(options.renderer)
  showPathsInReplies = options.showPathsInReplies !== false
  steerToSendUserFile = options.steerToSendUserFile !== false

  // One section, added last on the session side, where the terminal draws.
  if (steerToSendUserFile) {
    on('prompt.compose', async ($, e, next) => {
      const composed = await next(e)
      if (!e.surfaces.includes('terminal')) return composed
      const text = e.tools.includes('SendUserFile') ? STEER_WITH_SEND : STEER_WITHOUT_SEND
      return { sections: [...composed.sections, { id: STEER_ID, text, scope: 'session' }] }
    })
  }

  // The render log sees each component before the drawing hooks below do.
  for (const component of TRACED) {
    on('ui.render', { component }, async ($, e, next) => {
      await trace($, e).catch(() => undefined)
      return next(e)
    })
  }

  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => {
    if (e.surface !== 'terminal' || e.props.isErrored) return next(e)
    const refs = toolImages(e.props.tool, undefined, e.props.output, e.props.tool_use_id)
    if (refs.length === 0) return next(e)
    return withPictures($, $.ui.resolve(e), e.requestId, next(e), refs, e.viewport?.columns, TOOL_INDENT, e.props.onScreen)
  })

  // A Read's picture goes under its own row unless a folded group holds it.
  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    const { tool, input, output, isRunning, isErrored, isInterrupted } = e.props
    if (e.surface !== 'terminal' || tool !== 'Read' || isRunning || isErrored || isInterrupted) return next(e)
    if (folded.get(e.props.tool_use_id) === true) return next(e)
    const refs = toolImages(tool, input, output, e.props.tool_use_id)
    if (refs.length === 0) return next(e)
    return withPictures($, $.ui.resolve(e), e.requestId, next(e), refs, e.viewport?.columns, TOOL_INDENT, e.props.onScreen)
  })

  // Reads fold into one count line; the pictures go under it. An unfolded
  // group draws each call as its own row, where the ToolUse hook serves.
  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    if (e.surface !== 'terminal') return next(e)
    for (const call of e.props.calls) {
      if (call.tool_use_id === undefined) continue
      folded.delete(call.tool_use_id)
      folded.set(call.tool_use_id, !e.props.isExpanded)
      if (folded.size > FOLDED_MAX) folded.delete(folded.keys().next().value as string)
    }
    if (e.props.isExpanded) return next(e)
    const refs = e.props.calls
      .filter(call => !call.isRunning && !call.isErrored && !call.isInterrupted)
      .flatMap((call, n) => toolImages(call.tool, call.input, call.output, call.tool_use_id ?? `${e.requestId}-${n}`))
    if (refs.length === 0) return next(e)
    return withPictures($, $.ui.resolve(e), e.requestId, next(e), refs, e.viewport?.columns, TOOL_INDENT, e.props.onScreen)
  })

  on('ui.render', { component: 'AssistantMessage' }, async ($, e, next) => {
    if (e.surface !== 'terminal' || e.props.isSummary === true || !IMAGE_MENTION.test(e.props.text)) return next(e)
    const named = replyImages(e.props.text, await $.env.get('HOME'), { bare: showPathsInReplies })
    // A path that names no file draws nothing, and says nothing.
    const refs = await takeExisting(named, path => isRegularFile($, path), REPLY_IMAGES_MAX)
    if (refs.length === 0) return next(e)
    return withPictures($, $.ui.resolve(e), e.requestId, next(e), refs, e.viewport?.columns, MESSAGE_INDENT, e.props.onScreen)
  })

  // A pasted image reaches the prompt row as base64 content; its bytes go to
  // the cache as a data URI file, listed under the row's uuid.
  on('session.append', { door: 'prompt' }, async ($, e, next) => {
    const stored = await next(e)
    const refs = contentImages(e.message.content, e.uuid)
    if (refs.length === 0) return stored
    const dir = await cacheDirectory($)
    if (dir === null) return stored
    const files: string[] = []
    for (const ref of refs) {
      const file = 'data' in ref ? await stash($, ref, dir) : null
      if (file !== null) files.push(file)
    }
    await $.state.set({ ...PASTED, id: stored.uuid ?? e.uuid }, files)
    return stored
  }).catch(($, e, next) => next(e))

  on('ui.render', { component: 'UserMessage' }, async ($, e, next) => {
    if (e.surface !== 'terminal') return next(e)
    const { value: files = [] } = await $.state.get({ ...PASTED, id: e.requestId })
    if (files.length === 0) return next(e)
    const refs = files.map((file, n): ImageRef => ({ file, alt: `pasted image ${n + 1}` }))
    return withPictures($, $.ui.resolve(e), e.requestId, next(e), refs, e.viewport?.columns, MESSAGE_INDENT, e.props.onScreen)
  })
}
