// The copy controls (ADR-514 item 15). A press copies one form of a block:
//
// - `markdown`, the block as written;
// - `native`, its own form: a table as CSV, a fence's code as written, a command as typed;
// - `text`, the text as drawn, with no border glyph or padding;
// - `new`, a diff's code as it stands after the change.
//
// A block offers the forms whose copied text differs, the first of each kept: a table its
// markdown, CSV and text; a fence its markdown and code, whose text is the code again; a
// command or a long span one copy. Each form is labelled by the format it copies, from
// LABELS: `copy_style` draws the offer as words, `Copy as: markdown · csv · text`, or as
// glyphs. A block with one copy draws `Copy`, or the copy glyph, alone.

import type { Copy } from './commands'
import type { Code, Segment, Table } from './markdown'
import { proseText, tableCsv, tableText } from './markdown'
import type { Look } from './rows'

export type CopyForm = 'markdown' | 'native' | 'text' | 'new'

/** A format's label: the verbose word, and the compact glyph from a Nerd Font and in ASCII. */
export type Label = { verbose: string; nerd: string; ascii: string }

/** Every label a copy control draws, keyed by the format copied; `copy` and `reply` lead a
 *  control. A fence's own form is keyed by its language, which falls back to `code`'s
 *  glyphs with the language as its word. */
export const LABELS = {
  markdown: { verbose: 'markdown', nerd: '\ue73e', ascii: 'md' },
  text: { verbose: 'text', nerd: '\uf036', ascii: 'txt' },
  csv: { verbose: 'csv', nerd: '\uf0ce', ascii: 'csv' },
  code: { verbose: 'code', nerd: '\uf121', ascii: 'code' },
  new: { verbose: 'new only', nerd: '\uf067', ascii: 'new' },
  copy: { verbose: 'Copy', nerd: '\uf0c5', ascii: 'cp' },
  reply: { verbose: 'Copy reply', nerd: '\uf0c5', ascii: 'reply' },
} as const satisfies Readonly<Record<string, Label>>

/** What leads a control: a block's copy, or the reply's. */
export type Lead = 'copy' | 'reply'

/** A format's label: its row, or for a language not in the table, `code`'s glyphs under
 *  the language's name. A language naming a lead is read as `code`. */
function labelOf(format: string): Label {
  const row = Object.hasOwn(LABELS, format) && format !== 'copy' && format !== 'reply' ? LABELS[format as keyof typeof LABELS] : undefined

  return row ?? { ...LABELS.code, verbose: format }
}

/** One form a press copies: the text, and the format whose label it draws. */
export type CopyAs = { form: CopyForm; text: string; format: string }

// A table as markdown again, for one that has no source of its own.
const tableMarkdown = (table: Table): string =>
  [table.header, table.header.map(() => '---'), ...table.rows].map(cells => `| ${cells.join(' | ')} |`).join('\n')

/** The forms whose text differs, the first of each kept. */
export const distinct = (copies: readonly CopyAs[]): CopyAs[] => copies.filter((copy, i) => copies.findIndex(other => other.text === copy.text) === i)

/** A block with one copy: `text`. */
export const single = (text: string): CopyAs[] => [{ form: 'native', text, format: 'code' }]

/** What a block offers to copy. */
export function copiesOf(block: Copy | Code | Table): CopyAs[] {
  if (block.kind === 'table') {
    return distinct([
      { form: 'markdown', text: block.raw ?? tableMarkdown(block), format: 'markdown' },
      { form: 'native', text: tableCsv(block), format: 'csv' },
      { form: 'text', text: tableText(block), format: 'text' },
    ])
  }

  if (block.kind === 'copy' && block.form !== 'code') {
    return single(block.text)
  }

  const code = block.kind === 'code' ? block.code : block.text
  const offered = distinct([
    { form: 'markdown', text: block.raw, format: 'markdown' },
    { form: 'native', text: code, format: block.lang === '' ? 'code' : block.lang },
    { form: 'text', text: code, format: 'text' },
  ])
  // A language whose label another offered form already draws, as ```markdown, reads `code`.
  const taken = offered.filter(copy => copy.form !== 'native').map(copy => labelOf(copy.format).verbose)

  return offered.map(copy => (copy.form === 'native' && taken.includes(labelOf(copy.format).verbose) ? { ...copy, format: 'code' } : copy))
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
    { form: 'markdown', text: whole, format: 'markdown' },
    { form: 'text', text: replyText(segments), format: 'text' },
  ])

/** The control for `copies`, padded by a space each side to sit on a border line;
 *  nothing where nothing can copy. `lead` is `reply` for the reply's control. */
export function copyControl(look: Look, key: string, copies: readonly CopyAs[], lead: Lead = 'copy') {
  const { Box, Button, Text } = look.ui
  const accent = look.skin.palette.user
  const isCompact = look.prefs.copyStyle === 'compact'
  const drawn = (label: Label): string => (!isCompact ? label.verbose : look.prefs.icons === 'ascii' ? label.ascii : label.nerd)
  const copy = look.copy

  if (copy === undefined || copies.length === 0) {
    return undefined
  }

  if (copies.length === 1) {
    const label = drawn(LABELS[lead])
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
      <Text color={accent}>{isCompact ? ` ${drawn(LABELS[lead])} ` : ` ${LABELS[lead].verbose} as: `}</Text>
      {copies.flatMap(({ form, text, format }, i) => [
        ...(i === 0 ? [] : [<Text color={look.skin.palette.muted}>{separator}</Text>]),
        <Button key={`${key}-${form}`} label={drawn(labelOf(format))} plain onPress={() => copy(text, labelOf(format).verbose)} />,
      ])}
      <Text> </Text>
    </Box>
  )
}

/** The control on a line of its own under a block or a card, flush right. */
export function copyRow(look: Look, key: string, copies: readonly CopyAs[], lead?: Lead) {
  const { Box } = look.ui
  const control = copyControl(look, key, copies, lead)

  return control === undefined ? (
    ''
  ) : (
    <Box flexDirection="row" justifyContent="flex-end">
      {control}
    </Box>
  )
}
