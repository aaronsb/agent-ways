// The frame contract between the images mod and ways-theme (ADR-517 item 6).
// Both mods' tests hold this file, one text, which scripts/check-plugins.sh
// keeps: a test's inline plugin closes over nothing of its file, so neither
// mod's tests can load the other's register(), and each checks its own side
// against these rules.
//
//   - images draws each picture with its caption in one Box keyed
//     `images-picture-<n>`, which ways-theme finds it by;
//   - the picture, an `Image` or a half-block `Raster`, fits the columns it
//     is drawn in: the transcript's width less every border, padding and
//     margin of the Boxes around it, a frame's among them;
//   - the caption keeps the file's name, linked to the file where there is
//     one, its size in pixels and the percentage it is shown at, inside the
//     frame. The percentage is the picture's columns of `cellWidth` pixels
//     against its own width, to the nearest point, give or take one where a
//     picture was narrowed after it was sized.

export const PICTURE_KEY = 'images-picture-'

/** Bytes of one Raster cell: code point, foreground, background, u32 each. */
const CELL_BYTES = 12
const DETAIL = /^ · (\d+)×(\d+) · (\d+)%$/

type Node = { type?: unknown; props?: Record<string, unknown>; children?: unknown[] }

const nodeOf = (tree: unknown): Node | null => (typeof tree === 'object' && tree !== null && !Array.isArray(tree) ? (tree as Node) : null)
const kids = (node: Node): unknown[] => (node.children ?? []).flat(Infinity) as unknown[]
const num = (p: Record<string, unknown>, ...names: string[]): number => {
  for (const name of names) if (typeof p[name] === 'number') return p[name] as number
  return 0
}

/** Columns a Box takes from its children's width. */
function insetOf(node: Node): number {
  if (node.type !== 'Box') return 0
  const p = node.props ?? {}
  return (
    (typeof p.borderStyle === 'string' ? 2 : 0) +
    num(p, 'paddingLeft', 'paddingX', 'padding') +
    num(p, 'paddingRight', 'paddingX', 'padding') +
    Math.max(0, num(p, 'marginLeft', 'marginX', 'margin')) +
    Math.max(0, num(p, 'marginRight', 'marginX', 'margin'))
  )
}

const textOf = (tree: unknown): string => {
  if (typeof tree === 'string') return tree
  const node = nodeOf(tree)
  return node === null ? '' : kids(node).map(textOf).join('')
}

/** One marked picture site: its Box, the columns the Boxes around its picture take, and whether a bordered Box holds it. */
export type Site = { box: Node; picture: Node | null; inset: number; isFramed: boolean; pictures: number }

/** Every Box keyed `images-picture-<n>` in `tree`. */
export function pictureSites(tree: unknown): Site[] {
  const out: Site[] = []
  const walk = (at: unknown, inset: number, isFramed: boolean, site: Site | null) => {
    const node = nodeOf(at)
    if (node === null) return
    const own = insetOf(node)
    const key = node.props?.key
    let here = site
    if (node.type === 'Box' && typeof key === 'string' && key.startsWith(PICTURE_KEY)) {
      here = { box: node, picture: null, inset: 0, isFramed, pictures: 0 }
      out.push(here)
    }
    if (here !== null && (node.type === 'Image' || node.type === 'Raster')) {
      here.pictures += 1
      here.picture = node
      here.inset = inset
    }
    const framed = isFramed || (node.type === 'Box' && typeof node.props?.borderStyle === 'string')
    for (const child of kids(node)) walk(child, inset + own, framed, here)
  }
  walk(tree, 0, false, null)
  return out
}

export type Expect = {
  /** The transcript's width. */
  columns: number
  /** The cell width the picture was sized with, in pixels. */
  cellWidth: number
  /** The file the caption links to; omitted for a picture with no file. */
  href?: string
  /** The caption's file name. */
  name: string
  /** Each picture sits inside a bordered Box. */
  isFramed?: boolean
}

/** What `tree` breaks of the contract, one line each; empty when it holds. */
export function contractBreaches(tree: unknown, want: Expect): string[] {
  const sites = pictureSites(tree)
  if (sites.length === 0) return ['no Box keyed images-picture-<n>']
  const out: string[] = []
  for (const site of sites) {
    const key = String(site.box.props?.key)
    if (site.pictures !== 1 || site.picture === null) {
      out.push(`${key} holds ${site.pictures} pictures, not one`)
      continue
    }
    const p = site.picture.props ?? {}
    const columns = num(p, 'columns')
    if (columns + site.inset > want.columns) out.push(`${key}: ${columns} columns inside ${site.inset} of insets overrun ${want.columns}`)
    if (site.picture.type === 'Raster' && atob(String(p.cells ?? '')).length !== columns * num(p, 'rows') * CELL_BYTES) {
      out.push(`${key}: the Raster's cells do not fill ${columns}x${num(p, 'rows')}`)
    }
    if (want.isFramed === true && !site.isFramed) out.push(`${key} is outside the frame`)
    const below = kidsDeep(site.box)
    const links = below.filter(n => n.type === 'Link')
    if (want.href !== undefined && !links.some(n => n.props?.href === want.href && n.props?.label === want.name)) {
      out.push(`${key}: no link to ${want.href} labelled ${want.name}`)
    }
    if (want.href === undefined && !textOf(site.box).includes(want.name)) out.push(`${key}: the caption does not name ${want.name}`)
    const detail = below.map(n => DETAIL.exec(textOf(n))).find(m => m !== null)
    if (detail === undefined || detail === null) {
      out.push(`${key}: no ' · W×H · P%' caption`)
      continue
    }
    const shown = Math.round(((columns * want.cellWidth) / Number(detail[1])) * 100)
    if (Math.abs(Number(detail[3]) - shown) > 1) out.push(`${key}: the caption says ${detail[3]}% for ${columns} columns, which show ${shown}%`)
  }
  return out
}

function kidsDeep(node: Node): Node[] {
  const out: Node[] = []
  for (const child of kids(node)) {
    const at = nodeOf(child)
    if (at === null) continue
    out.push(at, ...kidsDeep(at))
  }
  return out
}
