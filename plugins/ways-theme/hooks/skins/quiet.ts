// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/quiet.ts.

// Quiet output: a call that only looks (a read, a search, `git status`) gets its row and
// nothing under it. Anything that might change a file, or that this cannot be sure of,
// keeps its output.

export type Shell = 'bash' | 'powershell'

// Redirects that only throw output away or merge streams change nothing.
const HARMLESS_REDIRECT = /\s(?:[12]?>\s*\/dev\/null|[12]?>\s*\$null|2>&1|>&2|[12]>&[12])(?=\s|$)/g

// What runs code or writes, wherever it sits: substitutions, heredocs, process substitution.
const EXECUTES = /\$\(|`|<<|<\(|>\(/

const SEPARATORS = ['&&', '||', ';', '|', '&', '\n'] as const

// The command split on its separators, quotes respected; null when a quote never closes.
export function segmentsOf(command: string): string[] | null {
  const segments: string[] = []
  let current = ''
  let quote: '"' | "'" | null = null

  for (let i = 0; i < command.length; i++) {
    const char = command[i] ?? ''

    if (quote !== null) {
      current += char
      quote = char === quote ? null : quote
      continue
    }

    if (char === '"' || char === "'") {
      quote = char
      current += char
      continue
    }

    const separator = SEPARATORS.find(mark => command.startsWith(mark, i))

    if (separator !== undefined) {
      segments.push(current)
      current = ''
      i += separator.length - 1
      continue
    }

    current += char
  }

  if (quote !== null) {
    return null
  }

  segments.push(current)

  return segments.map(segment => segment.trim()).filter(segment => segment !== '')
}

// Words with their quotes taken off; a quoted `>` is text, not a redirect.
function wordsOf(segment: string): { words: string[]; redirects: boolean } {
  const words: string[] = []
  let word = ''
  let quote: '"' | "'" | null = null
  let redirects = false
  let started = false

  for (const char of segment) {
    if (quote !== null) {
      if (char === quote) {
        quote = null
      } else {
        word += char
      }

      continue
    }

    if (char === '"' || char === "'") {
      quote = char
      started = true
    } else if (/\s/.test(char)) {
      if (started) {
        words.push(word)
      }

      word = ''
      started = false
    } else {
      redirects ||= char === '>'
      word += char
      started = true
    }
  }

  if (started) {
    words.push(word)
  }

  return { words, redirects }
}

const READERS = new Set([
  'cat', 'head', 'tail', 'ls', 'dir', 'pwd', 'cd', 'echo', 'wc', 'grep', 'egrep', 'fgrep', 'rg', 'tree', 'file',
  'stat', 'which', 'where', 'type', 'du', 'df', 'uniq', 'cut', 'diff', 'jq', 'date', 'whoami', 'uname', 'basename',
  'dirname', 'realpath', 'readlink', 'nl', 'column', 'tr', 'true', 'test', '[',
])

const POWERSHELL_READERS = new Set([
  'get-content', 'gc', 'get-childitem', 'gci', 'select-string', 'sls', 'get-item', 'gi', 'test-path',
  'get-location', 'gl', 'resolve-path', 'measure-object', 'measure', 'select-object', 'select', 'format-table', 'ft',
  'format-list', 'fl', 'sort-object', 'sort', 'get-command', 'gcm', 'get-date', 'write-output', 'write-host',
  'out-string', 'get-filehash', 'get-itemproperty', 'ls', 'dir', 'cat', 'pwd', 'cd', 'set-location', 'echo',
])

const GIT_READS = new Set(['log', 'status', 'diff', 'show', 'blame', 'rev-parse', 'ls-files', 'shortlog', 'describe', 'grep', 'reflog'])

// `gh <noun> <verb>` pairs that only fetch.
const GH_READS = /^(pr|issue|repo|run|release|workflow) (view|list|diff|checks|status)$/

// Flags that turn an otherwise read-only command into a writer or a launcher.
const FIND_WRITES = /^-(delete|exec|execdir|ok|okdir|fprint|fprint0|fprintf|fls)$/

function gitReads(words: readonly string[]): boolean {
  let i = 1

  // `git -C <dir> --no-pager log` reads the same as `git log`.
  while (words[i]?.startsWith('-') === true) {
    i += words[i] === '-C' || words[i] === '-c' ? 2 : 1
  }

  const sub = words[i] ?? ''
  const rest = words.slice(i + 1)

  if (sub === 'branch' || sub === 'tag' || sub === 'stash') {
    return sub === 'stash' ? rest[0] === 'list' || rest[0] === 'show' : rest.every(word => /^(-a|-r|-v|-vv|--list|-l|--show-current|--all|--remotes)$/.test(word))
  }

  if (sub === 'remote') {
    return rest.length === 0 || (rest.length === 1 && rest[0] === '-v')
  }

  return GIT_READS.has(sub) && !rest.some(word => word === '--output' || word.startsWith('--output='))
}

function bashReads(words: readonly string[]): boolean {
  const name = (words[0] ?? '').replace(/^.*[\\/]/, '').replace(/\.exe$/i, '')
  const args = words.slice(1)

  switch (name) {
    case 'git':
      return gitReads(words)
    case 'gh':
      return GH_READS.test(args.slice(0, 2).join(' '))
    case 'find':
      return !args.some(arg => FIND_WRITES.test(arg))
    case 'sort':
      return !args.some(arg => arg === '-o' || arg.startsWith('--output'))
    case 'sed': {
      // Only the printing form, `sed -n 10,40p file…`: one script, no other flags, so no
      // `-e 'w out'` or `-i` can ride along.
      const [script, ...files] = args.filter(arg => arg !== '-n')
      return args.includes('-n') && /^(\d+|\$)?(,(\d+|\$))?p$/.test(script ?? '') && !files.some(file => file.startsWith('-'))
    }
    default:
      return READERS.has(name)
  }
}

function powershellReads(words: readonly string[]): boolean {
  const name = (words[0] ?? '').toLowerCase()

  if (name === 'git' || name === 'gh') {
    return bashReads(words)
  }

  return POWERSHELL_READERS.has(name)
}

// Whether every command in `command` only reads. Unsure is no.
export function isReadOnlyShell(command: string, shell: Shell = 'bash'): boolean {
  const cleaned = ` ${command} `.replace(HARMLESS_REDIRECT, ' ')

  // A PowerShell script block can run anything; so can a substitution.
  if (EXECUTES.test(cleaned) || (shell === 'powershell' && /[{}]/.test(cleaned))) {
    return false
  }

  const segments = segmentsOf(cleaned)

  if (segments === null || segments.length === 0) {
    return false
  }

  return segments.every(segment => {
    const { words, redirects } = wordsOf(segment)

    // An assignment in front (`FOO=1 cmd`) changes what runs; `>` writes.
    if (redirects || words.length === 0 || /^[A-Za-z_]\w*=/.test(words[0] ?? '')) {
      return false
    }

    return shell === 'powershell' ? powershellReads(words) : bashReads(words)
  })
}

const QUIET_TOOLS = new Set(['Read', 'Glob', 'Grep'])

export const commandOf = (input: unknown): string => {
  const command = typeof input === 'object' && input !== null ? (input as { command?: unknown }).command : undefined

  return typeof command === 'string' ? command : ''
}

// Whether a call only looks, so its output can stay folded away.
export function isQuiet(tool: string, input: unknown): boolean {
  if (QUIET_TOOLS.has(tool)) {
    return true
  }

  if (tool === 'Bash' || tool === 'PowerShell') {
    const command = commandOf(input)

    return command !== '' && isReadOnlyShell(command, tool === 'Bash' ? 'bash' : 'powershell')
  }

  return false
}

// The row's verb for a quiet call: `Read`, `Searched`, `Listed`, `Looked`.
export function quietLabel(tool: string): string {
  switch (tool) {
    case 'Grep':
      return 'Searched'
    case 'Glob':
      return 'Listed'
    case 'Bash':
    case 'PowerShell':
      return 'Looked'
    default:
      return tool
  }
}

const ERROR_WORDS = /error|fatal|denied|not found|no such|cannot|can't|failed|invalid|unknown|refused/i

// The one line of a failure worth reading: the first that names an error, else the last.
export function errorLine(output: unknown): string {
  const fields = typeof output === 'object' && output !== null ? (output as { stderr?: unknown; stdout?: unknown }) : {}
  const text =
    typeof output === 'string'
      ? output
      : [fields.stderr, fields.stdout].find((value): value is string => typeof value === 'string' && value.trim() !== '') ?? ''

  const lines = text
    .replace(/<\/?tool_use_error>/g, '')
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(line => line !== '')

  return lines.find(line => ERROR_WORDS.test(line)) ?? lines.at(-1) ?? 'Failed'
}
