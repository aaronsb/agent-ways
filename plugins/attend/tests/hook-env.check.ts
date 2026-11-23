// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

// The mod's --parent wiring: Claude Code's pid, read as CLAUDE_PID from
// $.process.hookEnv(), is the sensor host's --parent. A test module that hooks
// process.hookEnv does not load on a Claude Code without the event, such as
// 2.1.312, so this file is not named *.test.ts: scripts/check-plugins.sh runs
// it as one from a copy of the plugin, and reports it skipped there.

import { describe, expect, mock, test } from 'claude-code/testing'
import type { On, ProcessRunResult } from 'claude-code'

const ok = (stdout = ''): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })

/** `ways settings list mods.attend --json --effective`: a project that
 *  starts its sessions listening, keepwarm off. */
const CONFIG = JSON.stringify({ view: 'effective', keys: { 'mods.attend.listening': { value: 'active' }, 'mods.attend.keepwarm': { value: 'off' } } })
const DRAIN = JSON.stringify({ schema: 1, status: 'drained', messages: [], held_back: 0, note: null, you_are: null, cache_lapsed: false })

/** The host beneath the plugin, with `env` as the command-hook environment;
 *  each `sensor host` exits 2 at once, and its argv is kept. */
function host(on: On, env: Record<string, string>): string[][] {
  const spawned: string[][] = []
  const state: { value: unknown; version: number } = { value: undefined, version: 0 }
  on('state.get', { plugin: 'attend', key: 'status' }, () => ({ value: { value: state.value, version: state.version } }) as never)
  on('state.set', { plugin: 'attend', key: 'status' }, ($, e) => {
    state.value = e.value
    state.version += 1
    return { value: { isSet: true, version: state.version } } as never
  })
  on('store.get', () => ({ value: undefined }) as never)
  on('store.set', () => ({ value: undefined }) as never)
  on('ui.log', () => ({ value: undefined }) as never)
  on('ui.toast', () => ({ value: undefined }) as never)
  on('session.id', () => ({ value: 's1' }) as never)
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('tool.register', ($, e) => ({ value: { tool: `mcp__attend__${e.name}` } }))
  on('process.run', ($, e) => {
    const cmd = e.argv.slice(1).join(' ')
    if (e.argv[0] === 'ways') return { value: ok(CONFIG) }
    if (cmd.startsWith('message inbox --drain')) return { value: ok(DRAIN) }
    if (cmd === 'message unread --json') return { value: ok('{"unread":0,"held_back":0,"listening":true,"rooms":[]}') }
    return { value: ok() }
  })
  on('process.hookEnv', () => ({ value: env }) as never)
  on('process.spawn', async function* ($, e) {
    spawned.push([...e.argv])
    return { value: { code: 2, signal: null } } as never
  })
  return spawned
}

const START = { cwd: '/work/agent-ways', surface: 'terminal', isInteractive: true } as const
const ARGV = ['attend', 'sensor', 'host', '--session', 's1', '--cwd', '/work/agent-ways']

describe('the sensor host watches Claude Code\'s pid', () => {
  test('CLAUDE_PID from the hook environment is the host\'s --parent', async ($, on) => {
    const clock = mock.clock(on, { now: 1_800_000_000_000 })
    const spawned = host(on, { CLAUDECODE: '1', CLAUDE_PID: '4242' })
    await $.session.start(START as never)
    await clock.advance(1_000)
    expect(spawned).toEqual([[...ARGV, '--parent', '4242']])
  })

  test('a CLAUDE_PID that is no pid gives no --parent', async ($, on) => {
    const clock = mock.clock(on, { now: 1_800_000_000_000 })
    const spawned = host(on, { CLAUDE_PID: 'claude' })
    await $.session.start(START as never)
    await clock.advance(1_000)
    expect(spawned).toEqual([ARGV])
  })
})
