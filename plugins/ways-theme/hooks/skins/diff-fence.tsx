// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/diff-fence.tsx.

import { highlight, languageOf, roleColors } from './highlight'
import type { Token } from './highlight'
import { onBorder } from './rows'
import type { Look, Ui } from './rows'
import { diffLines } from './svg-diff'
import type { DiffInput, Hunk } from './svg-diff'

// A ```diff fence drawn as a diff: old and new line numbers, added and removed lines in the
// skin's colours, and the code as it stands after the change for `copy new only`.

const HEADER = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/
const FILE_LINE = /^(?:diff |index |--- |\+\+\+ |similarity |rename |new file|deleted file)/

// The fence's hunks, or null when it holds no added or removed line. A fence without
// `@@` headers is one hunk numbered from 1.
export function fenceDiff(code: string): DiffInput | null {
  const lines = code.replace(/\n$/, '').split('\n')
  const path = /^\+\+\+ (?:b\/)?(.+)$/m.exec(code)?.[1]?.trim() ?? ''
  const hunks: Hunk[] = []
  let current: { oldStart: number; newStart: number; lines: string[] } | null = null

  for (const line of lines) {
    const header = HEADER.exec(line)

    if (header !== null) {
      current = { oldStart: Number(header[1]), newStart: Number(header[2]), lines: [] }
      hunks.push(current)
      continue
    }

    if (FILE_LINE.test(line) && (current === null || line.startsWith('diff '))) {
      continue
    }

    if (current === null) {
      current = { oldStart: 1, newStart: 1, lines: [] }
      hunks.push(current)
    }

    current.lines.push(/^[+\- \\]/.test(line) ? line : ` ${line}`)
  }

  const changes = hunks.some(hunk => hunk.lines.some(line => line.startsWith('+') || line.startsWith('-')))

  return changes ? { path: path === '/dev/null' ? '' : path, hunks, isNewFile: false } : null
}

// The code after the change: context and added lines, markers off.
export const newOnly = (diff: DiffInput): string =>
  diff.hunks
    .flatMap(hunk => hunk.lines.filter(line => !line.startsWith('-') && !line.startsWith('\\')).map(line => line.slice(1)))
    .join('\n')

// Whether a fence tag is a diff, and the language it names: ```diff and ```patch name none,
// ```diff-fsharp names F#, as Prism reads `diff-<language>`. Null for any other tag.
export function diffFenceTag(tag: string): { lang: string | undefined } | null {
  if (tag === 'diff' || tag === 'patch') {
    return { lang: undefined }
  }

  return tag.startsWith('diff-') ? { lang: languageOf(tag.slice('diff-'.length)) } : null
}

// The language a diff is read in: the one its fence tag names, else the one its path names
// by its extension (`src/Foo.fs` reads as F#). Undefined when neither names one the
// highlighter knows.
export const diffLanguage = (diff: DiffInput, fenceLang?: string): string | undefined => {
  const extension = /\.([\w#+-]+)$/.exec(diff.path)?.[1]

  return fenceLang ?? (extension === undefined ? undefined : languageOf(extension))
}

// Each line's tokens by its place in `lines`, in the language the path names. The old side
// (context and removed lines) and the new side (context and added lines) are each read in
// order, so a comment or string that spans lines carries; a context line takes the new
// side's reading. Undefined when there is nothing to highlight in.
export function diffTokens(lines: readonly ReturnType<typeof diffLines>[number][], lang: string | undefined): (Token[] | undefined)[] | undefined {
  if (lang === undefined) {
    return undefined
  }

  const tokens: (Token[] | undefined)[] = lines.map(() => undefined)
  // `without` is the side's opposite: leaving out removed lines reads the new side.
  const readSide = (without: 'add' | 'del') => {
    const at = lines.flatMap((line, i) => (line.kind === 'gap' || line.kind === without ? [] : [i]))
    const scanned = highlight(at.map(i => (lines[i] as { text: string }).text).join('\n'), lang)

    at.forEach((i, k) => {
      tokens[i] ??= scanned[k]
    })
  }

  readSide('del')
  readSide('add')

  return tokens
}

// The terminal's drawing: a frame like a code block's, the numbers in a gutter.
export function diffRows(
  look: Look,
  diff: DiffInput,
  control: ReturnType<Ui['Button']> | ReturnType<Ui['Box']> | undefined,
  fenceLang?: string,
) {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const lines = diffLines(diff.hunks)
  const digits = String(Math.max(1, ...lines.map(line => ('kind' in line && line.kind !== 'gap' ? Math.max(line.old ?? 0, line.new ?? 0) : 0)))).length
  const number = (value: number | undefined) => (value === undefined ? ' '.repeat(digits) : String(value).padStart(digits))
  const added = lines.filter(line => line.kind === 'add').length
  const removed = lines.filter(line => line.kind === 'del').length
  const syntax = look.prefs.highlight ? diffTokens(lines, diffLanguage(diff, fenceLang)) : undefined
  const colors = roleColors(palette)

  return (
    <Box flexDirection="column" marginY={1}>
      {onBorder(
        look,
        <Box flexDirection="column" borderStyle="round" borderColor={palette.muted} paddingX={1}>
          {lines.map((line, i) => {
            if (line.kind === 'gap') return <Text color={palette.muted}>{`${' '.repeat(digits * 2 + 2)}⋯ line ${line.at}`}</Text>
            const sign = line.kind === 'add' ? '+' : line.kind === 'del' ? '-' : ' '
            const signColor = line.kind === 'add' ? palette.ok : line.kind === 'del' ? palette.err : palette.muted

            // The numbers and the sign in a gutter of their own, so a wrapped line goes on
            // under its code.
            return (
              <Box flexDirection="row">
                <Box flexShrink={0}>
                  <Text>
                    <Text color={palette.muted}>{`${number(line.old)} ${number(line.new)} `}</Text>
                    <Text color={signColor}>{`${sign} `}</Text>
                  </Text>
                </Box>
                <Box flexShrink={1} flexGrow={1}>
                  {syntax?.[i] !== undefined && line.text !== '' ? (
                    // The sign keeps the change's colour; the code takes the language's.
                    <Text>
                      {(syntax[i] ?? []).map(token => (
                        <Text color={colors[token.role]} dimColor={line.kind === 'del'}>
                          {token.text}
                        </Text>
                      ))}
                    </Text>
                  ) : (
                    <Text color={signColor}>{line.text === '' ? ' ' : line.text}</Text>
                  )}
                </Box>
              </Box>
            )
          })}
        </Box>,
        {
          label: (
            <Text color={palette.muted}>
              {` ${diff.path === '' ? 'diff' : diff.path} `}
              <Text color={palette.ok}>{`+${added}`}</Text>
              <Text color={palette.err}>{` −${removed} `}</Text>
            </Text>
          ),
          control,
        },
      )}
    </Box>
  )
}
