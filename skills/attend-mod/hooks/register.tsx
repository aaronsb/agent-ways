import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { AttendModStatus } from '../types/index.d.ts'
import {
  CHAT_PRESENT_ARGV,
  DRAIN_ARGV,
  LIST_ARGV,
  PEERS_ARGV,
  WHOAMI_ARGV,
  WHOAMI_MACHINE_ARGV,
  callsIn,
  chunk,
  formatMessage,
  frameInbound,
  inviteChannel,
  isFullPreamble,
  joinArgv,
  parseDrain,
  parseListing,
  parseMachine,
  peerLines,
  parsePeerCount,
  shortId,
  replyArgv,
  sendArgv,
} from './lib.ts'
import type { Drain, Listed } from './lib.ts'

const status = atom({ plugin: 'attend-mod', key: 'status' } as const, {
  mode: 'dormant',
  self: null,
  project: null,
  enrolled: null,
  peers: 0,
  unread: 0,
  channels: [],
  deliveries: 0,
  afterCompaction: false,
} satisfies AttendModStatus)

// The cheap check: called in, human present, quiet long enough to sleep.
const CHECK_MS = 15_000
// A drain that finds messages waits this long and drains again, so a burst
// arrives as one turn.
const BATCH_MS = 2_000

// agent-ways theme (tools/agent-theme/themes/agent-ways.theme).
const SUBTLE = '#3a3a3c'
const FG = '#d0d0d0'

type $ = EngineInterface

/** What the module keeps between dispatches. A reload starts it afresh,
 *  dormant, which is the default anyway. */
type Mod = {
  pollMs: number
  quietMs: number
  /** Turns running now. The drain poll waits while any runs: the Stop-hook
   *  drain owns delivery at a turn's boundary. */
  running: Set<string>
  isPolling: boolean
  isChecking: boolean
  /** Messages sent before this are not a call-in (load, or the last sleep). */
  sinceMs: number
  /** The last inbound delivery or outbound tool call while awake. */
  lastActivityMs: number
  isChatPresent: boolean
  /** Short `~id` handles of delivered messages, to their full ids. */
  ids: Map<string, string>
}

async function attend($: $, argv: readonly string[]) {
  return $.process.run(argv, { timeoutMs: 15_000 })
}

/** Runs each part's argv in order and reports one line per part. */
async function sendParts($: $, argvs: readonly string[][]) {
  const lines: string[] = []
  let isError = false
  for (const argv of argvs) {
    const r = await attend($, argv)
    const said = (r.stdout.trim() || r.stderr.trim()).split('\n').pop() ?? ''
    lines.push(r.exitCode === 0 ? said || 'sent' : `failed (${r.exitCode}): ${r.stderr.trim()}`)
    if (r.exitCode !== 0) {
      isError = true
      break
    }
  }
  const header = argvs.length > 1 ? `${argvs.length} parts (message over 400 characters)\n` : ''
  return { text: header + lines.join('\n'), isError }
}

/** A body the listing parser cannot read, kept as one line of its own. */
function rawMessage(body: string): Listed {
  return { scope: 'attend', sender: 'unparsed', id: body, when: '', message: body, sentMs: 0 }
}

async function addChannel($: $, channel: string) {
  const c = `#${channel.replace(/^#/, '')}`
  await update($, status, s => ({ ...s, channels: [...new Set([...s.channels, c])] }))
}

async function learnSelf($: $) {
  const display = await attend($, WHOAMI_ARGV)
  const machine = parseMachine((await attend($, WHOAMI_MACHINE_ARGV)).stdout)
  const self = display.exitCode === 0 ? display.stdout.trim() || null : null
  const project = machine.origin_path?.split('/').filter(Boolean).pop() ?? null
  await update($, status, s => ({ ...s, self, project }))
}

async function refreshPeers($: $) {
  const r = await attend($, PEERS_ARGV)
  if (r.exitCode === 0) await update($, status, s => ({ ...s, peers: parsePeerCount(r.stdout) }))
}

/** Submits one framed batch; resolves as its turn starts, waking an idle
 *  session. */
async function deliver($: $, mod: Mod, msgs: readonly Listed[], count: number, lead?: string) {
  mod.lastActivityMs = await $.clock.now()
  for (const m of msgs) mod.ids.set(shortId(m.id), m.id)
  const seen = msgs.map(m => m.scope).filter(c => c.startsWith('#'))
  const s = await read($, status)
  const full = isFullPreamble(s.deliveries, s.afterCompaction)
  await update($, status, t => ({
    ...t,
    deliveries: t.deliveries + 1,
    afterCompaction: false,
    channels: [...new Set([...t.channels, ...seen])],
  }))
  const framed = frameInbound(msgs.map(formatMessage), count, full)
  await $.prompt.submit({ text: lead ? `${lead}\n\n${framed}` : framed })
}

/** One drain; on messages, a second after BATCH_MS, then one prompt for the
 *  batch. Awake only. Returns what the first drain said. */
async function poll($: $, mod: Mod): Promise<Drain | undefined> {
  if (mod.isPolling || mod.running.size > 0) return undefined
  if ((await read($, status)).mode !== 'awake') return undefined
  mod.isPolling = true
  try {
    const first = parseDrain((await attend($, DRAIN_ARGV)).stdout)
    await update($, status, s => ({ ...s, enrolled: first.kind !== 'silent' }))
    if (first.kind !== 'messages') return first
    const msgs = parseListing(first.body)
    if (msgs.length === 0) msgs.push(rawMessage(first.body))
    let count = first.count
    await update($, status, s => ({ ...s, unread: count }))
    await new Promise<void>(resolve => $.clock.after(BATCH_MS, resolve))
    const second = parseDrain((await attend($, DRAIN_ARGV)).stdout)
    if (second.kind === 'messages') {
      const more = parseListing(second.body)
      msgs.push(...(more.length > 0 ? more : [rawMessage(second.body)]))
      count += second.count
      await update($, status, s => ({ ...s, unread: count }))
    }
    await deliver($, mod, msgs, count)
    return first
  } finally {
    mod.isPolling = false
    await update($, status, s => ({ ...s, unread: 0 }))
  }
}

/** Wakes the mod: accepts any invitation among `hits`, drains, and where
 *  the drain is silent (not enrolled) delivers the call-in messages
 *  themselves from the read-only listing. */
async function wake($: $, mod: Mod, reason: string, hits: readonly Listed[]) {
  const now = await $.clock.now()
  mod.lastActivityMs = now
  const was = (await read($, status)).mode
  if (was === 'awake' && hits.length === 0) return
  await update($, status, s => ({ ...s, mode: 'awake' as const }))
  if (was === 'dormant') await refreshPeers($)
  for (const m of hits) {
    const channel = inviteChannel(m)
    if (channel) {
      await attend($, joinArgv(channel))
      await addChannel($, channel)
    }
  }
  const drained = await poll($, mod)
  if (hits.length > 0 && (drained === undefined || drained.kind === 'silent')) {
    await deliver($, mod, hits, hits.length, `attend: this session was called in (${reason}).`)
  }
  // Called-in messages are handled; the next check looks past them.
  mod.sinceMs = Math.max(mod.sinceMs, ...hits.map(m => m.sentMs))
}

async function sleep($: $, mod: Mod) {
  mod.sinceMs = await $.clock.now()
  await update($, status, s => ({ ...s, mode: 'dormant' as const, unread: 0 }))
}

/** The periodic cheap check. Dormant: is the human here, or is this session
 *  called in? Awake: refresh peers, and sleep after the quiet period with
 *  no human present. */
async function check($: $, mod: Mod) {
  if (mod.isChecking) return
  mod.isChecking = true
  try {
    mod.isChatPresent = (await $.process.run(CHAT_PRESENT_ARGV, { timeoutMs: 5_000 })).exitCode === 0
    const s = await read($, status)
    if (s.mode === 'dormant') {
      if (mod.isChatPresent) return await wake($, mod, 'attend-chat is open', [])
      const listed = parseListing((await attend($, LIST_ARGV)).stdout)
      const hits = listed.filter(m => callsIn(m, s.self, mod.sinceMs) !== undefined)
      const first = hits[0]
      if (first) await wake($, mod, callsIn(first, s.self, mod.sinceMs) ?? 'called in', hits)
      return
    }
    await refreshPeers($)
    const idleFor = (await $.clock.now()) - mod.lastActivityMs
    if (!mod.isChatPresent && mod.running.size === 0 && idleFor >= mod.quietMs) await sleep($, mod)
  } finally {
    mod.isChecking = false
  }
}

function toolText(sent: { text: string; isError: boolean }) {
  return sent.isError ? { result: sent.text, isError: true as const } : { result: sent.text }
}

export const register: Register = (on, options) => {
  const mod: Mod = {
    pollMs: Math.max(1, Number(options.pollSeconds ?? 5)) * 1000,
    quietMs: Math.max(1, Number(options.quietMinutes ?? 20)) * 60_000,
    running: new Set(),
    isPolling: false,
    isChecking: false,
    sinceMs: 0,
    lastActivityMs: 0,
    isChatPresent: false,
    ids: new Map(),
  }

  on('session.start', async ($, e, next) => {
    await $.tool.register({
      name: 'attend_send',
      description:
        'Send a message to peer Claude sessions over attend. Defaults to the #open base channel; `channel` scopes it to a named channel, `to` to one project path. Messages over 400 characters are sent in numbered parts.',
      inputSchema: {
        type: 'object',
        properties: {
          message: { type: 'string', description: 'The message body.' },
          channel: { type: 'string', description: 'A named channel, with or without #.' },
          to: { type: 'string', description: 'A project path to scope the send to.' },
        },
        required: ['message'],
      },
      isDeferred: false,
    })
    await $.tool.register({
      name: 'attend_reply',
      description:
        'Reply to a peer message over attend. Threads to the most recent inbound message, or to the message given as `re` (the `~id` at the end of a line, or a full id). Messages over 400 characters are sent in numbered parts.',
      inputSchema: {
        type: 'object',
        properties: {
          message: { type: 'string', description: 'The reply body.' },
          re: { type: 'string', description: 'The id of the message replied to (the `~id` at the end of a delivery line).' },
        },
        required: ['message'],
      },
      isDeferred: false,
    })
    await $.tool.register({
      name: 'attend_join',
      description: 'Join an attend channel (created if absent), so its messages reach this session.',
      inputSchema: {
        type: 'object',
        properties: { channel: { type: 'string', description: 'The channel name, with or without #.' } },
        required: ['channel'],
      },
      isDeferred: false,
    })
    await $.tool.register({
      name: 'attend_peers',
      description: 'List the active Claude Code sessions on this machine and their channels.',
      inputSchema: { type: 'object', properties: {} },
      isDeferred: false,
    })

    mod.sinceMs = await $.clock.now()
    // Dormant from here: nothing drawn, nothing submitted. The first check
    // runs after this hook returns, so a wake it causes can start a turn.
    $.clock.after(0, () => void learnSelf($).then(() => check($, mod)))
    $.clock.every(CHECK_MS, () => void check($, mod))
    $.clock.every(mod.pollMs, () => void poll($, mod))
    return next(e)
  })

  // A compaction drops the earlier preamble from the model's view, so the
  // next delivery shows it in full. A precompute installs nothing.
  on('session.compact', async ($, e, next) => {
    const done = await next(e)
    if (e.trigger !== 'precompute' && e.agentId === undefined && done.skip === undefined) {
      await update($, status, s => ({ ...s, afterCompaction: true }))
    }
    return done
  })

  on('turn.start', ($, e, next) => {
    mod.running.add(e.turnId)
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    mod.running.delete(e.turnId)
    // Whatever the Stop hook left (or all of it, where no Stop hook runs).
    $.clock.after(250, () => void poll($, mod))
    return done
  })

  on('tool.call', { tool: 'mcp__attend-mod__attend_send' }, async ($, e) => {
    const input = e as unknown as { message?: unknown; channel?: unknown; to?: unknown }
    const message = String(input.message ?? '')
    if (message.trim() === '') return { result: 'message is empty; nothing sent', isError: true }
    const channel = typeof input.channel === 'string' ? input.channel : undefined
    const to = typeof input.to === 'string' ? input.to : undefined
    const sent = await sendParts($, chunk(message).map(p => sendArgv(p, channel, to)))
    $.clock.after(0, () => void wake($, mod, 'this session sent', []))
    return toolText(sent)
  })

  on('tool.call', { tool: 'mcp__attend-mod__attend_reply' }, async ($, e) => {
    const input = e as unknown as { message?: unknown; re?: unknown }
    const message = String(input.message ?? '')
    if (message.trim() === '') return { result: 'message is empty; nothing sent', isError: true }
    const raw = typeof input.re === 'string' ? input.re.replace(/^~/, '') : ''
    const re = raw === '' ? undefined : mod.ids.get(raw) ?? raw
    const sent = await sendParts($, chunk(message).map(p => replyArgv(p, re)))
    $.clock.after(0, () => void wake($, mod, 'this session replied', []))
    return toolText(sent)
  })

  on('tool.call', { tool: 'mcp__attend-mod__attend_join' }, async ($, e) => {
    const input = e as unknown as { channel?: unknown }
    const channel = String(input.channel ?? '').trim()
    if (channel === '') return { result: 'channel is empty; nothing joined', isError: true }
    const joined = await sendParts($, [joinArgv(channel)])
    if (!joined.isError) await addChannel($, channel)
    $.clock.after(0, () => void wake($, mod, 'this session joined a channel', []))
    return toolText(joined)
  })

  on('tool.call', { tool: 'mcp__attend-mod__attend_peers' }, async $ => {
    const r = await attend($, PEERS_ARGV)
    if (r.exitCode !== 0) return { result: `attend peers failed (${r.exitCode}): ${r.stderr.trim()}`, isError: true }
    await update($, status, s => ({ ...s, peers: parsePeerCount(r.stdout) }))
    return { result: r.stdout }
  })

  // One lozenge while awake: the attend state only (peers, unread, channels).
  // The operator's statusline already shows name and project. Dormant draws
  // nothing.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) return next(e)
    const s = await read($, status)
    if (s.mode !== 'awake') return next(e)
    const { Box, Text } = $.ui.resolve(e)
    const state =
      s.enrolled === false ? '○ not enrolled' : `● ${s.peers} peers ✉ ${s.unread} unread`
    const channels = s.channels.length > 0 ? ` ${s.channels.join(' ')}` : ''
    return (
      <Box flexDirection="row">
        <Text color={SUBTLE}>{'\ue0b6'}</Text>
        <Text color={FG} backgroundColor={SUBTLE}>
          {' '}
          {state}
          {channels}{' '}
        </Text>
        <Text color={SUBTLE}>{'\ue0b4'}</Text>
      </Box>
    )
  })

  // The human's row for a delivery this plugin submitted: one dim line per
  // peer message. ctrl+o (isExpanded) and rows of any other origin keep the
  // engine's drawing. The model still reads the full framed text.
  on('ui.render', { component: 'UserMessage' }, ($, e, next) => {
    const { origin, isExpanded, text } = e.props
    if (isExpanded || origin.kind !== 'plugin' || origin.name !== 'attend-mod') return next(e)
    const lines = peerLines(text)
    if (!lines) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    return (
      <Box flexDirection="column">
        {lines.map((l, i) => (
          <Text key={i} dimColor wrap="truncate-end">
            {l}
          </Text>
        ))}
      </Box>
    )
  })
}
