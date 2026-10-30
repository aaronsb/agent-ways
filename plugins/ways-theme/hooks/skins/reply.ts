// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/reply.ts.

import type { RenderSurface } from 'claude-code'

import type { Prefs } from '../../types'
import { drawsBlocks, hasBlocks } from './blocks'
import { copyPieces, fenceCopy, holdsCopy } from './commands'
import { diffFenceTag, fenceDiff } from './diff-fence'
import { isShell, splitReply } from './markdown'
import type { Code, Segment } from './markdown'
import { inlineMath } from './math'
import { parseMermaid } from './mermaid'
import { artKind } from './mermaid-art'

// Which markdown the skin draws itself: a reply's blocks, or null to leave the text to
// Claude Code's own drawing. On the terminal the text the person copies is a block of its
// own (commands.ts).

// Markdown takes at most 10000 characters a block; a longer one keeps Claude Code's own
// drawing.
export const MAX_MARKDOWN = 9000

// A fence the skin draws as a picture: a Mermaid chart, or a diff with changes in it.
const isPicture = (segment: Code, prefs: Prefs): boolean =>
  (segment.lang === 'mermaid' && prefs.charts) || (diffFenceTag(segment.lang) !== null && fenceDiff(segment.code) !== null)

// `width` is the columns the reply's prose is drawn in, for the code spans too wide for it.
export function replySegments(text: string, prefs: Prefs, surface: RenderSurface, width?: number): Segment[] | null {
  const cards = prefs.tables !== 'off' && /\||```|~~~/.test(text)
  // The terminal draws every paragraph itself; elsewhere only alerts and task lists.
  const blocks = prefs.markdown && (surface === 'terminal' || hasBlocks(text))
  // With cards off, a Mermaid fence the parser reads is still split out to draw.
  const charts = prefs.charts && /mermaid/i.test(text)
  // Display formulas become cards, inline TeX Unicode.
  const math = prefs.math && /\$|```math|\\\[|\\\(/.test(text)
  const copies = surface === 'terminal' && holdsCopy(text, width)

  if (!cards && !blocks && !charts && !math && !copies) {
    return null
  }

  const charted = (lang: string, code: string) =>
    lang === 'mermaid' && (parseMermaid(code) !== null || (surface === 'terminal' && artKind(code) !== null))
  const split: Segment[] = cards
    ? splitReply(text, { tables: true, fence: () => true, math })
    : charts || math
      ? splitReply(text, { tables: false, fence: (lang, code) => charts && charted(lang, code), math })
      : [{ kind: 'text', text }]
  const lifted = (segment: Segment): Segment[] =>
    segment.kind === 'text' ? copyPieces(segment.text, width) : segment.kind === 'code' && !isPicture(segment, prefs) ? [fenceCopy(segment.lang, segment.code, segment.raw) ?? segment] : [segment]
  const segments = copies ? split.flatMap(lifted) : split
  const fits = segments.every(segment => segment.kind === 'table' || (segment.kind === 'text' ? segment.text : segment.raw).length <= MAX_MARKDOWN)
  // Off the terminal a shell fence keeps the app's drawing, for its Run button; a reply
  // with nothing else to draw is left to the app whole.
  const kept = surface === 'terminal' ? segments : segments.filter(segment => !isShell(segment))
  const drawn = kept.some(
    segment =>
      segment.kind !== 'text' ||
      (prefs.markdown && drawsBlocks(segment.text, surface)) ||
      (math && inlineMath(segment.text) !== segment.text),
  )

  return fits && drawn ? segments : null
}
