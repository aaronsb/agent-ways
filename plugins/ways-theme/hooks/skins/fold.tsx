// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/fold.tsx.

import type { Look } from './rows'

// Long blocks fold to their first lines behind a `▾ N more` control; `▴ less` folds them
// again. Which blocks the person opened lives in the drawing's `folds`.

// Lines of code and rows of a table shown while folded.
export const FOLD_CODE = 24
export const FOLD_ROWS = 14

// How many of `count` lines to show: all where nothing folds or the person opened it.
export function shownCount(look: Look, key: string, count: number, limit: number): number {
  return look.folds === undefined || look.folds.open.has(key) || count <= limit ? count : limit
}

// The control under a long block; nothing for a block short enough to show whole.
export function foldButton(look: Look, key: string, count: number, limit: number) {
  const { Box, Button } = look.ui
  const folds = look.folds

  if (folds === undefined || count <= limit) {
    return ''
  }

  const isOpen = folds.open.has(key)
  const ascii = look.prefs.icons === 'ascii'
  const label = isOpen ? `${ascii ? '^' : '▴'} less` : `${ascii ? 'v' : '▾'} ${count - limit} more`

  return (
    <Box flexDirection="row">
      <Button key={key} label={label} plain dimColor onPress={() => folds.toggle(key)} />
    </Box>
  )
}
