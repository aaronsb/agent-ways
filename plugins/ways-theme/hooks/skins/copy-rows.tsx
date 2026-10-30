// The copy controls (ADR-514 item 15). A press copies one form of a block:
//
// - `markdown`, the block as written;
// - `native`, its own form: a table as CSV, a fence's code as written, a command as typed;
// - `text`, the text as drawn, with no border glyph or padding;
// - `new`, a diff's code as it stands after the change.
//
// A block offers the forms whose copied text differs, the first of each kept: a table its
// markdown, CSV and text; a fence its markdown and code, whose text is the code again; a
// command or a long span one copy. `copy_style` draws the offer as words, `Copy as:
// markdown · native · text`, or as glyphs (skin.ts, `CopyGlyphs`). A block with one copy
// draws `Copy`, or the copy glyph, alone.

import type { Copy } from './commands'
import type { Code, Segment, Table } from './markdown'
import { proseText, tableCsv, tableText } from './markdown'
import type { Look } from './rows'
import type { CopyGlyph } from './skin'

export type CopyForm = 'markdown' | 'native' | 'text' | 'new'

/** One form a press copies: the text, and the glyph the compact style draws for it. */
export type CopyAs = { form: CopyForm; text: string; glyph: CopyGlyph }

/** The word the verbose style draws for each form, and the toast names. */
const WORD: Readonly<Record<CopyForm, string>> = { markdown: 'markdown', native: 'native', text: 'text', new: 'new only' }

// A table as markdown again, for one that has no source of its own.
const tableMarkdown = (table: Table): string =>
  [table.header, table.header.map(() => '---'), ...table.rows].map(cells => `| ${cells.join(' | ')} |`).join('\n')

/** The forms whose text differs, the first of each kept. */
export const distinct = (copies: readonly CopyAs[]): CopyAs[] => copies.filter((copy, i) => copies.findIndex(other => other.text === copy.text) === i)

/** A block with one copy: `text`. */
export const single = (text: string): CopyAs[] => [{ form: 'native', text, glyph: 'code' }]

/** What a block offers to copy. */
export function copiesOf(block: Copy | Code | Table): CopyAs[] {
  if (block.kind === 'table') {
    return distinct([
      { form: 'markdown', text: block.raw ?? tableMarkdown(block), glyph: 'markdown' },
      { form: 'native', text: tableCsv(block), glyph: 'table' },
      { form: 'text', text: tableText(block), glyph: 'text' },
    ])
  }

  if (block.kind === 'copy' && block.form !== 'code') {
    return single(block.text)
  }

  const code = block.kind === 'code' ? block.code : block.text

  return distinct([
    { form: 'markdown', text: block.raw, glyph: 'markdown' },
    { form: 'native', text: code, glyph: 'code' },
    { form: 'text', text: code, glyph: 'text' },
  ])
}

// A reply as its text: prose without its markdown, a table as `tableText`, a fence, a
// formula and a copy block as their content.
function replyText(segments: readonly Segment[]): string {
  const textOf = (segment: Segment): string =>
    segment.kind === 'text' ? proseText(segment.text) : segment.kind === 'table' ? tableText(segment) : segment.kind === 'code' ? segment.code : segment.kind === 'math' ? segment.tex : segment.text

  return segments
    .map(segment => textOf(segment).trim())
    .filter(text => text !== '')
    .join('\n\n')
}

/** What the reply's control offers: the reply as written, and as its text. */
export const replyCopies = (whole: string, segments: readonly Segment[]): CopyAs[] =>
  distinct([
    { form: 'markdown', text: whole, glyph: 'markdown' },
    { form: 'text', text: replyText(segments), glyph: 'text' },
  ])

/** The control for `copies`, padded by a space each side to sit on a border line;
 *  nothing where nothing can copy. `of` names what is copied where it is not the block
 *  above: `reply` draws `Copy reply` and `Copy reply as:`. */
export function copyControl(look: Look, key: string, copies: readonly CopyAs[], of?: string) {
  const { Box, Button, Text } = look.ui
  const accent = look.skin.palette.user
  const glyphs = look.icons.copy
  const isCompact = look.prefs.copyStyle === 'compact'
  const copy = look.copy
  const name = of === undefined ? 'Copy' : `Copy ${of}`

  if (copy === undefined || copies.length === 0) {
    return undefined
  }

  if (copies.length === 1) {
    const label = isCompact ? glyphs.copy : name
    const text = copies[0]!.text

    return (
      <Box flexDirection="row">
        <Text> </Text>
        <Button key={key} label={label} plain onPress={() => copy(text)}>
          <Text color={accent} bold>
            {label}
          </Text>
        </Button>
        <Text> </Text>
      </Box>
    )
  }

  const separator = look.prefs.icons === 'ascii' ? ' | ' : ' · '

  return (
    <Box flexDirection="row">
      <Text color={accent}>{isCompact ? ` ${glyphs.copy} ` : ` ${name} as: `}</Text>
      {copies.flatMap(({ form, text, glyph }, i) => [
        ...(i === 0 ? [] : [<Text color={look.skin.palette.muted}>{separator}</Text>]),
        <Button key={`${key}-${form}`} label={isCompact ? glyphs[glyph] : WORD[form]} plain onPress={() => copy(text, WORD[form])} />,
      ])}
      <Text> </Text>
    </Box>
  )
}

/** The control on a line of its own under a block or a card, flush right. */
export function copyRow(look: Look, key: string, copies: readonly CopyAs[], of?: string) {
  const { Box } = look.ui
  const control = copyControl(look, key, copies, of)

  return control === undefined ? (
    ''
  ) : (
    <Box flexDirection="row" justifyContent="flex-end">
      {control}
    </Box>
  )
}
