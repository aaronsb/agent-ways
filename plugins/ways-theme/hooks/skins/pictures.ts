// Another mod's picture in a row the theme frames. The images mod (images@dotfiles)
// draws an `Image` or a `Raster` and a caption line under the row Claude Code
// drew; the runtime stamps each Image and Raster with the plugin whose hook drew
// it, so the theme finds one without knowing the mod.
//
// Where such a mod sits beneath the theme in the chain, it draws under the row
// `next(e)` gave it, the engine's own: `withRow` puts the theme's row in that
// row's place, so the theme's row, the picture and its caption sit together in
// the frame. A rewritten `viewport` does not reach the mod (the runtime skips a
// hook that changes the envelope), so it sizes the picture to the transcript's
// width; `fitted` narrows the picture to the frame's room, its height in
// proportion. Where the mod sits above the theme, it draws around the theme's
// frame and none of this applies.

import type { RenderElement } from 'claude-code'

const OWN = 'ways-theme'
/** Bytes of one Raster cell: code point, foreground, background, u32 each. */
const CELL_BYTES = 12

type Node = {
  type?: unknown
  props?: Record<string, unknown>
  children?: unknown[]
  image?: { plugin?: unknown }
  raster?: { plugin?: unknown }
}

/** Whether a reply names an image file, as a picture a mod draws under it would. */
export const namesImage = (text: string): boolean => /\.(png|jpe?g|gif|webp)\b/i.test(text)

const nodeOf =(tree: unknown): Node | null => (typeof tree === 'object' && tree !== null ? (tree as Node) : null)

const isPicture = (node: Node): boolean =>
  (node.type === 'Image' && node.image?.plugin !== OWN) || (node.type === 'Raster' && node.raster?.plugin !== OWN)

const holdsEngine = (tree: unknown): boolean => {
  const node = nodeOf(tree)
  return node !== null && (node.type === 'engine' || (node.children ?? []).some(holdsEngine))
}

/** Whether the tree holds a picture another plugin drew. */
export function holdsPicture(tree: unknown): boolean {
  const node = nodeOf(tree)
  return node !== null && (isPicture(node) || (node.children ?? []).some(holdsPicture))
}

/** `tree` with the engine's own drawing in it replaced by `row`, when the tree
 *  holds another plugin's picture and the engine's drawing; `row` alone
 *  otherwise. A second engine node is dropped, so the row draws once. */
export function withRow(tree: RenderElement, row: RenderElement): RenderElement {
  if (!holdsPicture(tree) || !holdsEngine(tree)) return row
  let isPlaced = false
  const swap = (node: unknown): unknown => {
    const at = nodeOf(node)
    if (at === null) return node
    if (at.type === 'engine') {
      if (isPlaced) return null
      isPlaced = true
      return row
    }
    return at.children === undefined ? node : { ...at, children: at.children.map(swap).filter(child => child !== null) }
  }
  return swap(tree) as RenderElement
}

const cells = (props: Record<string, unknown>, ...names: string[]): number => {
  for (const name of names) if (typeof props[name] === 'number') return props[name] as number
  return 0
}

/** Columns a Box takes from its children's width: border, padding, margin. */
function insetOf(node: Node): number {
  if (node.type !== 'Box' || node.props === undefined) return 0
  const p = node.props
  const border = typeof p.borderStyle === 'string' ? 2 : 0
  return (
    border +
    cells(p, 'paddingLeft', 'paddingX', 'padding') +
    cells(p, 'paddingRight', 'paddingX', 'padding') +
    Math.max(0, cells(p, 'marginLeft', 'marginX', 'margin')) +
    Math.max(0, cells(p, 'marginRight', 'marginX', 'margin'))
  )
}

/** A Raster's decoded cells resampled to `to`, nearest cell, as base64. */
function resample(source: string, from: { columns: number; rows: number }, to: { columns: number; rows: number }): string {
  let out = ''
  for (let y = 0; y < to.rows; y++) {
    const row = Math.floor((y * from.rows) / to.rows) * from.columns
    for (let x = 0; x < to.columns; x++) {
      const at = (row + Math.floor((x * from.columns) / to.columns)) * CELL_BYTES
      out += source.slice(at, at + CELL_BYTES)
    }
  }
  return btoa(out)
}

/** A picture narrowed to `room` columns, its rows in proportion. */
function narrowed(node: Node, room: number): Node {
  const props = node.props ?? {}
  const columns = cells(props, 'columns')
  const rows = cells(props, 'rows')
  if (columns <= room || room < 1 || rows < 1) return node
  const to = { columns: room, rows: Math.max(1, Math.round((rows * room) / columns)) }
  if (node.type === 'Image') return { ...node, props: { ...props, ...to } }
  const source = atob(typeof props.cells === 'string' ? props.cells : '')
  if (source.length !== columns * rows * CELL_BYTES) return node
  return { ...node, props: { ...props, ...to, cells: resample(source, { columns, rows }, to) } }
}

/** `tree` with each picture another plugin drew narrowed to the `columns` it
 *  has inside the Boxes around it. */
export function fitted(tree: RenderElement, columns: number): RenderElement {
  const fit = (node: unknown, room: number): unknown => {
    const at = nodeOf(node)
    if (at === null) return node
    if (isPicture(at)) return narrowed(at, room)
    if (at.children === undefined) return node
    const inner = room - insetOf(at)
    return { ...at, children: at.children.map(child => fit(child, inner)) }
  }
  return holdsPicture(tree) ? (fit(tree, columns) as RenderElement) : tree
}
