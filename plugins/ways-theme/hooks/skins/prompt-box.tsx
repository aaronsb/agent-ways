// The person's message drawn on the theme's gradient. The engine's own box
// takes one background colour, so the mod lays the text out itself: line by
// line, each line padded to the box's width, every run of cells on the ground
// the gradient gives it. A message of VERTICAL_FROM_LINES lines or more runs
// the gradient down its lines; a shorter one runs it across the cells of each
// line, padding rows included, so one line is as smooth as many. The theme's
// own axis plays no part here. Colours mix in OKLab. A theme with no gradient
// hands one stop, its selection colour, for a flat box. The text takes the
// theme's body colour, which the theme lifts to its contrast floor on the
// gradient's grounds.

import type { ElementTable } from 'claude-code'

import { gradientAt } from './config.ts'
import type { Axis } from './config.ts'
import { IS_SEGMENTED, graphemes, widthOf } from './markdown.ts'

export { graphemes }

/** Cells of ground left and right of the text. */
export const PAD_X = 2
/** The narrowest box worth drawing; a narrower view takes the plain frame. */
export const MIN_WIDTH = 12

/** Lines of text from which the gradient runs down the box, not across. */
export const VERTICAL_FROM_LINES = 4

/** The way the gradient runs for a message of `textLines` lines. */
export const axisFor = (textLines: number): Axis => (textLines >= VERTICAL_FROM_LINES ? 'vertical' : 'horizontal')

/** One stretch of a line on one ground. */
export type Run = { text: string; ground: string }

/** A joiner, an emoji variation selector, a skin-tone modifier or a regional
 *  indicator: a sequence whose code points do not add up to its width. */
const EMOJI_SEQUENCE = /[\u200d\ufe0f\u{1f3fb}-\u{1f3ff}\u{1f1e6}-\u{1f1ff}]/u

/** A grapheme's cells: two for an emoji sequence, else its code points'. */
export const cellsOf = (grapheme: string): number => (EMOJI_SEQUENCE.test(grapheme) ? 2 : widthOf(grapheme))

const textCells = (text: string): number => graphemes(text).reduce((sum, g) => sum + cellsOf(g), 0)

/** Whether the box can lay `text` out: always where the engine segments
 *  graphemes; without that, only text that holds no emoji sequence. */
export const canMeasure = (text: string, isSegmented = IS_SEGMENTED): boolean => isSegmented || !EMOJI_SEQUENCE.test(text)

/** `paragraph` cut into lines of at most `width` cells: at spaces where it
 *  can, inside a word longer than a line where it must, never inside a
 *  grapheme. The paragraph keeps its indentation; a line a wrap began drops
 *  the spaces it would open with. */
export function wrapCells(paragraph: string, width: number): string[] {
  const lines: string[] = []
  let line = ''
  let used = 0
  let isWrapped = false
  const push = () => {
    lines.push(line.replace(/ +$/, ''))
    line = ''
    used = 0
    isWrapped = true
  }
  for (const word of paragraph.split(/(?<= )/)) {
    if (used > 0 && used + textCells(word.trimEnd()) > width) push()
    for (const g of graphemes(word)) {
      const c = cellsOf(g)
      if (used + c > width) push()
      if (used === 0 && isWrapped && g === ' ') continue
      line += g
      used += c
    }
  }
  if (line !== '' || lines.length === 0) push()
  return lines
}

/** The text's lines inside a box `width` cells wide, a blank line of ground
 *  above and below. */
export function boxLines(text: string, width: number): string[] {
  const inner = width - PAD_X * 2
  const body = text
    .replace(/\t/g, '  ')
    .split('\n')
    .flatMap(p => wrapCells(p, inner))
  const pad = ' '.repeat(PAD_X)
  return ['', ...body, ''].map(line => `${pad}${line}${' '.repeat(Math.max(0, inner - textCells(line)))}${pad}`)
}

/** Each line as runs of ground: one run a line down a vertical gradient,
 *  a run per change of colour across a horizontal one, a grapheme on the
 *  ground of its first cell. */
export function gradientRuns(lines: readonly string[], width: number, stops: readonly string[], axis: Axis): Run[][] {
  // Each line or cell takes the colour at its centre, so the box spans the
  // gradient's middle rather than landing on its two end stops.
  if (axis === 'vertical') {
    return lines.map((text, i) => [{ text, ground: gradientAt(stops, (i + 0.5) / lines.length) }])
  }
  const cells = Array.from({ length: width }, (_, j) => gradientAt(stops, (j + 0.5) / width))
  return lines.map(line => {
    const runs: Run[] = []
    let at = 0
    for (const g of graphemes(line)) {
      const ground = cells[Math.min(at, width - 1)]!
      const tail = runs[runs.length - 1]
      if (tail !== undefined && tail.ground === ground) tail.text += g
      else runs.push({ text: g, ground })
      at += cellsOf(g)
    }
    return runs
  })
}

/** The drawn box. */
export function gradientBox(ui: Pick<ElementTable, 'Box' | 'Text'>, runs: readonly Run[][], textColor: string) {
  const { Box, Text } = ui
  return (
    <Box flexDirection="column" marginY={1}>
      {runs.map(line => (
        <Text wrap="truncate-end">
          {line.map(run => (
            <Text color={textColor} backgroundColor={run.ground}>
              {run.text}
            </Text>
          ))}
        </Text>
      ))}
    </Box>
  )
}
