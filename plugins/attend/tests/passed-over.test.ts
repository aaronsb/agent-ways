import { describe, expect, test } from 'claude-code/testing'

import { register } from '../hooks/register.tsx'
import { passedOverHits, registrationsOf } from './passed-over.ts'

describe('what the mod hooks reaches it on every host', () => {
  test('no hook of the mod is on an event an organization\'s policy plugin passes over', () => {
    const regs = registrationsOf(register as never)
    // The recording saw the module's hooks: the control for the empty list below.
    expect(regs.map(r => r.pattern)).toContain('session.end')
    expect(regs.map(r => r.pattern)).toContain('tool.call')
    expect(passedOverHits(regs)).toEqual([])
  })

  test('the check names each form a passed-over hook can take', () => {
    const regs = [
      { pattern: 'classic.SessionStart' },
      { pattern: 'classic.*' },
      { pattern: '*' },
      { pattern: 'session.*' },
      { pattern: '!tool.call' },
      { pattern: 'session.compact' },
      { pattern: 'settings.read' },
      { pattern: 'prompt.context' },
      { pattern: 'session.append' },
      { pattern: 'session.append', matcher: { door: 'compaction' } },
      { pattern: 'session.append', matcher: { door: ['prompt', 'hook-context'] } },
    ]
    expect(passedOverHits(regs)).toEqual([
      'classic.SessionStart',
      'classic.*',
      '*',
      'session.*',
      '!tool.call',
      'session.compact',
      'settings.read',
      'prompt.context',
      'session.append{door=attachment|hook-context|compaction}',
      'session.append{door=compaction}',
      'session.append{door=hook-context}',
    ])
  })

  test('the check passes what reaches a mod there', () => {
    const regs = [
      { pattern: 'session.end' },
      { pattern: 'session.measure' },
      { pattern: 'command.run', matcher: { command: 'compact' } },
      { pattern: 'tool.*' },
      { pattern: 'session.append', matcher: { door: 'prompt' } },
    ]
    expect(passedOverHits(regs)).toEqual([])
  })
})
