// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/svg-math.ts.

import { gapsOf, linear, parseTex, type Accent, type GridAlign, type MathNode, type Sym } from './math'
import type { Palette } from './skin'
import { CONTROL_SLOT, escape, HEADER_MID, riseEnd, svgCard } from './svg-kit'

// A display formula as a card, laid out the way TeX lays out boxes: each part a width,
// a height above the baseline and a depth below it, set side by side, stacked and
// shifted. Fonts differ between machines, so glyph widths are estimated and every glyph
// is centred in its estimated cell; radicals and tall delimiters are drawn as paths so
// they stretch to what they hold.

const HEADER_H = 56
const SIZE = 22
const PAD = 28
const MATH_FONT = `'Latin Modern Math', 'STIX Two Math', 'Cambria Math', 'Cambria', 'Times New Roman', serif`

// `w` wide, `a` above the baseline, `d` below it; `draw` puts it with its baseline at y.
type Box = { w: number; a: number; d: number; draw: (x: number, y: number) => string }

type Style = { s: number; display: boolean; script: boolean }

type Colors = { fg: string; op: string; word: string }

const round = (value: number): string => String(Math.round(value * 100) / 100)

const empty = (w = 0): Box => ({ w, a: 0, d: 0, draw: () => '' })

// The width of a glyph in em, close enough for the serif math fonts.
function glyphEm(char: string): number {
  const code = char.codePointAt(0) ?? 0

  if (/\p{Mn}|\p{Me}/u.test(char)) {
    return 0
  }

  if (code >= 0x1d400) {
    return 0.72
  }

  if (/[0-9]/.test(char)) {
    return 0.5
  }

  if (/[ijlt]/.test(char)) {
    return 0.32
  }

  if (/[f]/.test(char)) {
    return 0.36
  }

  if (/[mw]/.test(char)) {
    return 0.76
  }

  if (/[a-z]/.test(char)) {
    return 0.5
  }

  if (/[MW]/.test(char)) {
    return 0.9
  }

  if (/[I]/.test(char)) {
    return 0.38
  }

  if (/[A-Z]/.test(char)) {
    return 0.68
  }

  if (/[α-ω϶ϑϕϖϱϵ]/.test(char)) {
    return 0.56
  }

  if (/[Α-Ω]/.test(char)) {
    return 0.7
  }

  if (/[()[\]{}]/.test(char)) {
    return 0.36
  }

  if (/[|′'.,;:!]/.test(char)) {
    return 0.28
  }

  if (char === ' ') {
    return 0.28
  }

  return 0.74
}

const textEm = (text: string): number => [...text].reduce((sum, char) => sum + glyphEm(char), 0)

function glyph(text: string, size: number, fill: string, options: { italic?: boolean; bold?: boolean; shift?: number } = {}): Box {
  if (text === '') {
    return empty()
  }

  const w = textEm(text) * size + (options.italic === true ? 0.04 * size : 0)
  const shift = options.shift ?? 0
  const isDigit = /^[0-9.]+$/.test(text)
  const many = [...text].filter(char => glyphEm(char) > 0).length > 1
  const style = `fill:${fill}${options.italic === true ? ';font-style:italic' : ''}${options.bold === true ? ';font-weight:700' : ''}`

  return {
    w,
    a: (isDigit ? 0.68 : 0.72) * size - shift,
    d: (isDigit ? 0 : 0.22) * size + shift,
    // A run of letters is held to its estimated width; one glyph sits in the middle of it.
    draw: (x, y) =>
      many
        ? `<text x="${round(x)}" y="${round(y + shift)}" font-size="${round(size)}" textLength="${round(w)}" lengthAdjust="spacing" style="${style}">${escape(text)}</text>`
        : `<text x="${round(x + w / 2)}" y="${round(y + shift)}" text-anchor="middle" font-size="${round(size)}" style="${style}">${escape(text)}</text>`,
  }
}

function hbox(parts: readonly Box[]): Box {
  return {
    w: parts.reduce((sum, part) => sum + part.w, 0),
    a: Math.max(0, ...parts.map(part => part.a)),
    d: Math.max(0, ...parts.map(part => part.d)),
    draw: (x, y) => {
      let at = x

      return parts
        .map(part => {
          const drawn = part.draw(at, y)
          at += part.w

          return drawn
        })
        .join('')
    },
  }
}

// `box` moved down by `dy` (up when negative) and across by `dx` inside a wider cell.
const placed = (box: Box, dx: number, dy: number, w = box.w): Box => ({
  w,
  a: box.a - dy,
  d: box.d + dy,
  draw: (x, y) => box.draw(x + dx, y + dy),
})

function stack(parts: readonly { box: Box; dy: number }[], w: number, extra = ''): Box {
  const lines = parts.map(({ box, dy }) => placed(box, (w - box.w) / 2, dy, w))

  return {
    w,
    a: Math.max(0, ...lines.map(line => line.a)),
    d: Math.max(0, ...lines.map(line => line.d)),
    draw: (x, y) => lines.map(line => line.draw(x, y)).join('') + extra.replaceAll('{x}', round(x)).replaceAll('{y}', round(y)),
  }
}

const smaller = (style: Style, factor: number): Style => ({ s: Math.max(style.s * factor, SIZE * 0.5), display: false, script: true })

const stroke = (d: string, width: number, color: string): string =>
  `<path d="${d}" fill="none" stroke="${color}" stroke-width="${round(width)}" stroke-linecap="round" stroke-linejoin="round"/>`

// --- Parts --------------------------------------------------------------------------

function symBox(node: Sym, style: Style, colors: Colors): Box {
  const { s } = style

  if (node.role === 'big') {
    const size = s * (style.display ? 1.55 : 1.15)
    // Centred on the maths axis, a quarter em above the baseline.
    const shift = 0.25 * size - 0.25 * s
    return glyph(node.text, size, colors.op, { shift })
  }

  const fill = node.role === 'op' || node.role === 'rel' ? colors.op : node.role === 'word' ? colors.word : colors.fg

  if (node.role === 'word' || node.role === 'fn') {
    const text = node.role === 'word' ? node.text.replace(/^ +| +$/g, ' ') : node.text

    return glyph(text, s, fill)
  }

  return glyph(node.text, s, fill, { italic: node.italic, bold: node.bold })
}

function fracBox(num: MathNode, den: MathNode, bar: boolean, style: Style, colors: Colors): Box {
  const { s } = style
  const inner = style.display ? { s, display: false, script: style.script } : smaller(style, 0.78)
  const top = layout(num, inner, colors)
  const bottom = layout(den, inner, colors)
  const axis = 0.25 * s
  const gap = bar ? 0.16 * s : 0.08 * s
  const thickness = 0.055 * s
  const w = Math.max(top.w, bottom.w) + 0.24 * s
  const rule = bar ? `<line x1="{x+}" y1="{yb}" x2="{x-}" y2="{yb}" stroke="${colors.fg}" stroke-width="${round(thickness)}"/>` : ''
  const box = stack(
    [
      { box: top, dy: -(axis + gap + top.d) },
      { box: bottom, dy: -axis + gap + bottom.a },
    ],
    w,
  )

  return {
    ...box,
    draw: (x, y) =>
      box.draw(x, y) +
      rule.replace('{x+}', round(x + 0.06 * s)).replace('{x-}', round(x + w - 0.06 * s)).replaceAll('{yb}', round(y - axis)),
  }
}

function sqrtBox(bodyNode: MathNode, indexNode: MathNode | null, style: Style, colors: Colors): Box {
  const { s } = style
  const body = layout(bodyNode, style, colors)
  const thickness = 0.055 * s
  const clearance = 0.12 * s
  const top = -(Math.max(body.a, 0.7 * s) + clearance + thickness / 2)
  const bottom = Math.max(body.d, 0.2 * s)
  const height = bottom - top
  const radical = 0.55 * s + Math.max(0, height - 1.4 * s) * 0.08
  const index = indexNode === null ? null : layout(indexNode, smaller(style, 0.55), colors)
  const lead = index === null ? 0 : Math.max(0, index.w - radical * 0.45)
  const knee = bottom - height * 0.42
  const w = lead + radical + body.w + 0.14 * s

  return {
    w,
    a: Math.max(-top + thickness / 2, index === null ? 0 : -(knee - 0.12 * s) + index.a),
    d: bottom,
    draw: (x, y) => {
      const at = x + lead
      const path = [
        `M${round(at + radical * 0.05)} ${round(y + knee + 0.04 * s)}`,
        `L${round(at + radical * 0.22)} ${round(y + knee - 0.06 * s)}`,
        `L${round(at + radical * 0.5)} ${round(y + bottom)}`,
        `L${round(at + radical)} ${round(y + top)}`,
        `L${round(at + radical + body.w + 0.1 * s)} ${round(y + top)}`,
      ].join(' ')
      const drawnIndex = index === null ? '' : index.draw(x + Math.max(0, radical * 0.45 - index.w), y + knee - 0.12 * s)

      return stroke(path, thickness, colors.fg) + drawnIndex + body.draw(at + radical + 0.04 * s, y)
    },
  }
}

function scriptsBox(baseNode: MathNode, supNode: MathNode | null, subNode: MathNode | null, style: Style, colors: Colors): Box {
  const { s } = style
  const base = layout(baseNode, style, colors)
  const inner = smaller(style, 0.7)
  const sup = supNode === null ? null : layout(supNode, inner, colors)
  const sub = subNode === null ? null : layout(subNode, inner, colors)

  // ∑ and lim in display: limits over and under, centred on the operator.
  if (style.display && baseNode.kind === 'sym' && baseNode.limits === true) {
    const gap = 0.14 * s
    const w = Math.max(base.w, sup?.w ?? 0, sub?.w ?? 0)

    return stack(
      [
        { box: base, dy: 0 },
        ...(sup === null ? [] : [{ box: sup, dy: -(base.a + gap + sup.d) }]),
        ...(sub === null ? [] : [{ box: sub, dy: base.d + gap + sub.a }]),
      ],
      w,
    )
  }

  const isBig = baseNode.kind === 'sym' && baseNode.role === 'big'
  const italic = baseNode.kind === 'sym' && baseNode.italic
  let up = sup === null ? 0 : Math.max(0.4 * s, base.a - (isBig ? 0.5 : 0.28) * s)
  const down = sub === null ? 0 : Math.max(0.24 * s, base.d - (isBig ? 0.1 : -0.06) * s)

  // Both scripts: keep a gap between the superscript's foot and the subscript's head.
  if (sup !== null && sub !== null) {
    const clash = 0.14 * s - ((up - sup.d) - (sub.a - down))
    up += Math.max(0, clash)
  }

  const kern = 0.04 * s
  const supX = base.w + kern + (italic ? 0.04 * s : 0)
  const subX = base.w + kern - (isBig ? 0.2 * s : 0)
  const w = Math.max(supX + (sup?.w ?? 0), subX + (sub?.w ?? 0)) + 0.03 * s

  return {
    w,
    a: Math.max(base.a, sup === null ? 0 : up + sup.a),
    d: Math.max(base.d, sub === null ? 0 : down + sub.d),
    draw: (x, y) => base.draw(x, y) + (sup?.draw(x + supX, y - up) ?? '') + (sub?.draw(x + subX, y + down) ?? ''),
  }
}

// A delimiter `half` tall either side of the axis, as a stroke; null for one with no shape.
function delimiterPath(delimiter: string, w: number, top: number, bottom: number, s: number): ((x: number, y: number) => string) | null {
  const mirror = /[)\]}⟩⌋⌉]/.test(delimiter)
  const shape = mirror ? ({ ')': '(', ']': '[', '}': '{', '⟩': '⟨', '⌋': '⌊', '⌉': '⌈' } as Record<string, string>)[delimiter] : delimiter
  const mid = (top + bottom) / 2
  const height = bottom - top
  const inner = w * 0.18
  const outer = w * 0.82

  const points = (x: number, y: number): string | null => {
    const at = (px: number): number => x + (mirror ? w - px : px)

    switch (shape) {
      case '(':
        return `M${round(at(outer))} ${round(y + top)} Q${round(at(inner - w * 0.12))} ${round(y + mid)} ${round(at(outer))} ${round(y + bottom)}`
      case '[':
        return `M${round(at(outer))} ${round(y + top)} L${round(at(inner))} ${round(y + top)} L${round(at(inner))} ${round(y + bottom)} L${round(at(outer))} ${round(y + bottom)}`
      case '⌊':
        return `M${round(at(inner))} ${round(y + top)} L${round(at(inner))} ${round(y + bottom)} L${round(at(outer))} ${round(y + bottom)}`
      case '⌈':
        return `M${round(at(outer))} ${round(y + top)} L${round(at(inner))} ${round(y + top)} L${round(at(inner))} ${round(y + bottom)}`
      case '{': {
        const centre = w * 0.5
        const curl = Math.min(0.28 * s, height * 0.12)

        return [
          `M${round(at(outer))} ${round(y + top)}`,
          `Q${round(at(centre))} ${round(y + top)} ${round(at(centre))} ${round(y + top + curl)}`,
          `L${round(at(centre))} ${round(y + mid - curl)}`,
          `Q${round(at(centre))} ${round(y + mid)} ${round(at(inner - w * 0.08))} ${round(y + mid)}`,
          `Q${round(at(centre))} ${round(y + mid)} ${round(at(centre))} ${round(y + mid + curl)}`,
          `L${round(at(centre))} ${round(y + bottom - curl)}`,
          `Q${round(at(centre))} ${round(y + bottom)} ${round(at(outer))} ${round(y + bottom)}`,
        ].join(' ')
      }
      case '⟨':
        return `M${round(at(outer))} ${round(y + top)} L${round(at(inner))} ${round(y + mid)} L${round(at(outer))} ${round(y + bottom)}`
      case '|':
      case '∣':
        return `M${round(x + w / 2)} ${round(y + top)} L${round(x + w / 2)} ${round(y + bottom)}`
      case '‖':
      case '∥':
        return `M${round(x + w * 0.35)} ${round(y + top)} L${round(x + w * 0.35)} ${round(y + bottom)} M${round(x + w * 0.65)} ${round(y + top)} L${round(x + w * 0.65)} ${round(y + bottom)}`
      default:
        return null
    }
  }

  return points(0, 0) === null ? null : (x, y) => points(x, y) ?? ''
}

function fenceBox(open: string, close: string, bodyNode: MathNode, style: Style, colors: Colors): Box {
  const { s } = style
  const body = layout(bodyNode, style, colors)
  const axis = 0.25 * s

  // Short enough for the font's own glyphs.
  if (body.a + body.d <= 1.2 * s) {
    return hbox([glyph(open, s, colors.fg), body, glyph(close, s, colors.fg)])
  }

  const half = Math.max(body.a - axis, body.d + axis) + 0.12 * s
  const top = -axis - half
  const bottom = -axis + half
  const width = 0.42 * s + Math.min(0.2 * s, (bottom - top) * 0.03)
  const thickness = 0.07 * s

  const side = (delimiter: string): Box => {
    if (delimiter === '') {
      return empty(0.06 * s)
    }

    const path = delimiterPath(delimiter, width, top, bottom, s)

    if (path === null) {
      return glyph(delimiter, s, colors.fg)
    }

    return { w: width, a: -top, d: bottom, draw: (x, y) => stroke(path(x, y), thickness, colors.fg) }
  }

  const pad = empty(0.08 * s)

  return hbox([side(open), pad, body, pad, side(close)])
}

function gridBox(rows: readonly MathNode[][], align: GridAlign, style: Style, colors: Colors): Box {
  const { s } = style
  const inner: Style = { s, display: align === 'pairs', script: false }
  const cells = rows.map(row => row.map(cell => layout(cell, inner, colors)))
  const columns = Math.max(0, ...cells.map(row => row.length))
  const widths = Array.from({ length: columns }, (_, c) => Math.max(0, ...cells.map(row => row[c]?.w ?? 0)))
  const gap = (c: number): number => (c === 0 ? 0 : align === 'pairs' ? (c % 2 === 1 ? 0 : 1.6 * s) : align === 'left' ? 1.1 * s : 0.9 * s)
  const side = (c: number): number => (align === 'pairs' ? (c % 2 === 0 ? 1 : 0) : align === 'left' ? 0 : 0.5)
  const heights = cells.map(row => ({
    a: Math.max(0.72 * s, ...row.map(cell => cell.a)),
    d: Math.max(0.22 * s, ...row.map(cell => cell.d)),
  }))
  const lead = align === 'pairs' ? 0.42 * s : 0.3 * s
  const total = heights.reduce((sum, row) => sum + row.a + row.d, 0) + lead * Math.max(0, rows.length - 1)
  const axis = 0.25 * s
  const w = widths.reduce((sum, width, c) => sum + width + gap(c), 0)

  return {
    w,
    a: axis + total / 2,
    d: total / 2 - axis,
    draw: (x, y) => {
      let baseline = y - axis - total / 2
      const drawn: string[] = []

      cells.forEach((row, r) => {
        const height = heights[r] ?? { a: 0, d: 0 }
        baseline += height.a
        let at = x

        widths.forEach((width, c) => {
          at += gap(c)
          const cell = row[c]

          if (cell !== undefined) {
            drawn.push(cell.draw(at + (width - cell.w) * side(c), baseline))
          }

          at += width
        })

        baseline += height.d + lead
      })

      return drawn.join('')
    },
  }
}

function accentBox(mark: Accent, bodyNode: MathNode, style: Style, colors: Colors): Box {
  const { s } = style
  const body = layout(bodyNode, style, colors)
  const thickness = 0.05 * s
  const lift = Math.max(body.a, 0.62 * s) + 0.1 * s
  const isWide = body.w > 0.8 * s
  const span = isWide ? body.w : Math.min(body.w, 0.5 * s)
  const italicNudge = bodyNode.kind === 'sym' && bodyNode.italic ? 0.06 * s : 0

  const drawMark = (x: number, y: number): string => {
    const left = x + (body.w - span) / 2 + italicNudge
    const right = left + span
    const centre = (left + right) / 2
    const at = y - lift

    switch (mark) {
      case 'hat':
        return stroke(`M${round(left)} ${round(at)} L${round(centre)} ${round(at - 0.16 * s)} L${round(right)} ${round(at)}`, thickness, colors.fg)
      case 'bar':
        return stroke(`M${round(x + 0.04 * s)} ${round(at - 0.04 * s)} L${round(x + body.w - 0.02 * s)} ${round(at - 0.04 * s)}`, thickness, colors.fg)
      case 'vec': {
        const from = isWide ? x : left - 0.06 * s
        const to = isWide ? x + body.w : right + 0.06 * s

        return stroke(`M${round(from)} ${round(at - 0.08 * s)} L${round(to)} ${round(at - 0.08 * s)} M${round(to - 0.12 * s)} ${round(at - 0.17 * s)} L${round(to)} ${round(at - 0.08 * s)} L${round(to - 0.12 * s)} ${round(at + 0.01 * s)}`, thickness, colors.fg)
      }
      case 'tilde': {
        const quarter = span / 4

        return stroke(`M${round(left)} ${round(at - 0.04 * s)} Q${round(left + quarter)} ${round(at - 0.16 * s)} ${round(centre)} ${round(at - 0.08 * s)} T${round(right)} ${round(at - 0.1 * s)}`, thickness, colors.fg)
      }
      case 'dot':
        return `<circle cx="${round(centre)}" cy="${round(at - 0.08 * s)}" r="${round(0.055 * s)}" fill="${colors.fg}"/>`
      case 'ddot':
        return [-0.12, 0.12].map(dx => `<circle cx="${round(centre + dx * s)}" cy="${round(at - 0.08 * s)}" r="${round(0.05 * s)}" fill="${colors.fg}"/>`).join('')
      case 'under':
        return stroke(`M${round(x)} ${round(y + body.d + 0.1 * s)} L${round(x + body.w)} ${round(y + body.d + 0.1 * s)}`, thickness, colors.fg)
    }
  }

  return {
    w: body.w,
    a: mark === 'under' ? body.a : lift + 0.2 * s,
    d: mark === 'under' ? body.d + 0.16 * s : body.d,
    draw: (x, y) => body.draw(x, y) + drawMark(x, y),
  }
}

function layout(node: MathNode, style: Style, colors: Colors): Box {
  switch (node.kind) {
    case 'sym':
      return symBox(node, style, colors)
    case 'space':
      return empty(node.em * style.s)
    case 'row': {
      const gaps = gapsOf(node.items, style.script)

      return hbox(node.items.flatMap((item, i) => [empty((gaps[i] ?? 0) * style.s), layout(item, style, colors)]))
    }
    case 'frac':
      return fracBox(node.num, node.den, node.bar, style, colors)
    case 'sqrt':
      return sqrtBox(node.body, node.index, style, colors)
    case 'scripts':
      return scriptsBox(node.base, node.sup, node.sub, style, colors)
    case 'fence':
      return fenceBox(node.open, node.close, node.body, style, colors)
    case 'grid':
      return gridBox(node.rows, node.align, style, colors)
    case 'accent':
      return accentBox(node.mark, node.body, style, colors)
  }
}

// `hasControl` leaves the header's right corner free for a Copy button laid over it.
export function mathSvg(tex: string, palette: Palette, width: number, hasControl = false): { source: string; width: number; height: number; alt: string; entranceMs: number } {
  const node = parseTex(tex)
  const colors: Colors = { fg: palette.fg, op: palette.user, word: palette.muted }
  const formula = layout(node, { s: SIZE, display: true, script: false }, colors)
  const room = width - PAD * 2
  const scale = formula.w > room ? room / formula.w : 1
  const cardW = Math.min(width, Math.max(480, Math.ceil(formula.w * scale + PAD * 2 + 48)))
  const bodyH = (formula.a + formula.d) * scale
  const height = Math.ceil(HEADER_H + PAD + bodyH + PAD)
  const x = (cardW - formula.w * scale) / 2
  const y = HEADER_H + PAD + formula.a * scale
  const lines = node.kind === 'grid' ? node.rows.length : 1
  const header = [
    `<text x="16" y="${HEADER_MID + 4}" font-size="11" style="fill:${palette.muted};letter-spacing:.1em;font-weight:600">MATH</text>`,
    lines > 1 ? `<text x="${cardW - 16 - (hasControl ? CONTROL_SLOT : 0)}" y="${HEADER_MID + 4}" text-anchor="end" font-size="11" style="fill:${palette.muted}">${lines} lines</text>` : '',
    `<line x1="0" y1="${HEADER_H - 0.5}" x2="${cardW}" y2="${HEADER_H - 0.5}" stroke="${palette.muted}" stroke-opacity=".3"/>`,
  ].join('')
  const body = `<g class="rise" style="animation-delay:80ms"><g font-family="${MATH_FONT}" transform="translate(${round(x)} ${round(y)}) scale(${round(scale)})">${formula.draw(0, 0)}</g></g>`

  const source = svgCard(cardW, height, palette, '', header + body)

  return { source, width: cardW, height, alt: `math:\n${linear(node)}`, entranceMs: riseEnd(source) }
}
