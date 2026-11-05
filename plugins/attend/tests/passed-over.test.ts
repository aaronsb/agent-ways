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
      { pattern: 'session.append', matcher: { door: 'tool-message' } },
      { pattern: 'session.append', matcher: { door: 'note' } },
      { pattern: 'session.append', matcher: { door: 'attachment' } },
      { pattern: 'session.append', matcher: { door: /hook/ } },
      { pattern: 'tool.*' },
      { pattern: 'tool.describe' },
      { pattern: 'tool.describe', matcher: { tool: ['Read', 'mcp__docs__search'] } },
      { pattern: 'tool.describe', matcher: { tool: /^Re/ } },
      { pattern: 'tool.describe', matcher: { provider: { tier: 'core' } } },
      { pattern: 'command.describe' },
      { pattern: 'agent.offer' },
      { pattern: 'agent.spawn' },
      { pattern: 'tool.register' },
      { pattern: 'ui.log' },
      { pattern: 'env.get' },
      { pattern: 'env.set' },
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
      'session.append{door=attachment|hook-context|compaction|tool-message|note}',
      'session.append{door=compaction}',
      'session.append{door=hook-context}',
      'session.append{door=tool-message}',
      'session.append{door=note}',
      'session.append{door=attachment}',
      'session.append{door=hook-context}',
      'tool.*',
      'tool.describe',
      'tool.describe',
      'tool.describe',
      'tool.describe',
      'command.describe',
      'agent.offer',
      'agent.spawn',
      'tool.register',
      'ui.log',
      'env.get',
      'env.set',
    ])
  })

  test('the check passes what reaches a mod there', () => {
    const regs = [
      { pattern: 'session.end' },
      { pattern: 'session.measure' },
      { pattern: 'command.run', matcher: { command: 'compact' } },
      { pattern: 'session.append', matcher: { door: 'prompt' } },
      { pattern: 'session.append', matcher: { door: /^prompt$/ } },
      { pattern: 'tool.describe', matcher: { tool: 'Read' } },
      { pattern: 'tool.describe', matcher: { tool: ['Read', 'SendUserFile'] } },
      { pattern: 'tool.list' },
      { pattern: 'tool.check' },
    ]
    expect(passedOverHits(regs)).toEqual([])
  })
})
