// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/math.ts.

import { widthOf } from './markdown'

// TeX math, parsed into a tree once and drawn three ways: in terminal cells over several
// rows, as one line of Unicode where a drawing will not fit (also the alt text), and as a
// vector card (svg-math.ts). The parser takes the subset replies use and never throws:
// what it does not know it shows as written.

export type Role = 'ord' | 'num' | 'word' | 'fn' | 'op' | 'rel' | 'open' | 'close' | 'punct' | 'big'

export type Accent = 'hat' | 'bar' | 'vec' | 'tilde' | 'dot' | 'ddot' | 'under'

// `pairs` is `aligned`: columns right, left, right, left, so `&=` lines up.
export type GridAlign = 'center' | 'left' | 'pairs'

// `limits` sets scripts above and below (∑, lim) instead of beside.
export type Sym = { kind: 'sym'; text: string; role: Role; italic: boolean; bold?: boolean; limits?: boolean }

export type MathNode =
  | Sym
  | { kind: 'row'; items: MathNode[] }
  | { kind: 'frac'; num: MathNode; den: MathNode; bar: boolean }
  | { kind: 'sqrt'; body: MathNode; index: MathNode | null }
  | { kind: 'scripts'; base: MathNode; sup: MathNode | null; sub: MathNode | null }
  | { kind: 'fence'; open: string; close: string; body: MathNode }
  | { kind: 'grid'; rows: MathNode[][]; align: GridAlign }
  | { kind: 'accent'; mark: Accent; body: MathNode }
  | { kind: 'space'; em: number }

// --- Symbols ------------------------------------------------------------------------

const GREEK: Readonly<Record<string, string>> = {
  alpha: 'α', beta: 'β', gamma: 'γ', delta: 'δ', epsilon: 'ϵ', varepsilon: 'ε', zeta: 'ζ', eta: 'η',
  theta: 'θ', vartheta: 'ϑ', iota: 'ι', kappa: 'κ', lambda: 'λ', mu: 'μ', nu: 'ν', xi: 'ξ', omicron: 'ο',
  pi: 'π', varpi: 'ϖ', rho: 'ρ', varrho: 'ϱ', sigma: 'σ', varsigma: 'ς', tau: 'τ', upsilon: 'υ',
  phi: 'ϕ', varphi: 'φ', chi: 'χ', psi: 'ψ', omega: 'ω',
  Gamma: 'Γ', Delta: 'Δ', Theta: 'Θ', Lambda: 'Λ', Xi: 'Ξ', Pi: 'Π', Sigma: 'Σ', Upsilon: 'Υ',
  Phi: 'Φ', Psi: 'Ψ', Omega: 'Ω',
}

const SYMBOLS: Readonly<Record<string, readonly [string, Role]>> = {
  times: ['×', 'op'], cdot: ['⋅', 'op'], pm: ['±', 'op'], mp: ['∓', 'op'], div: ['÷', 'op'], ast: ['∗', 'op'],
  star: ['⋆', 'op'], circ: ['∘', 'op'], bullet: ['∙', 'op'], oplus: ['⊕', 'op'], ominus: ['⊖', 'op'],
  otimes: ['⊗', 'op'], odot: ['⊙', 'op'], cup: ['∪', 'op'], cap: ['∩', 'op'], setminus: ['∖', 'op'],
  wedge: ['∧', 'op'], land: ['∧', 'op'], vee: ['∨', 'op'], lor: ['∨', 'op'], bmod: ['mod', 'op'],

  leq: ['≤', 'rel'], le: ['≤', 'rel'], geq: ['≥', 'rel'], ge: ['≥', 'rel'], neq: ['≠', 'rel'], ne: ['≠', 'rel'],
  approx: ['≈', 'rel'], equiv: ['≡', 'rel'], sim: ['∼', 'rel'], simeq: ['≃', 'rel'], cong: ['≅', 'rel'],
  propto: ['∝', 'rel'], ll: ['≪', 'rel'], gg: ['≫', 'rel'], prec: ['≺', 'rel'], succ: ['≻', 'rel'],
  in: ['∈', 'rel'], notin: ['∉', 'rel'], ni: ['∋', 'rel'], subset: ['⊂', 'rel'], subseteq: ['⊆', 'rel'],
  supset: ['⊃', 'rel'], supseteq: ['⊇', 'rel'], subsetneq: ['⊊', 'rel'], perp: ['⊥', 'rel'],
  parallel: ['∥', 'rel'], mid: ['∣', 'rel'], nmid: ['∤', 'rel'], models: ['⊨', 'rel'], vdash: ['⊢', 'rel'],
  to: ['→', 'rel'], rightarrow: ['→', 'rel'], leftarrow: ['←', 'rel'], gets: ['←', 'rel'],
  leftrightarrow: ['↔', 'rel'], Rightarrow: ['⇒', 'rel'], Leftarrow: ['⇐', 'rel'], Leftrightarrow: ['⇔', 'rel'],
  implies: ['⟹', 'rel'], impliedby: ['⟸', 'rel'], iff: ['⟺', 'rel'], mapsto: ['↦', 'rel'],
  longrightarrow: ['⟶', 'rel'], longleftarrow: ['⟵', 'rel'], uparrow: ['↑', 'rel'], downarrow: ['↓', 'rel'],
  rightleftharpoons: ['⇌', 'rel'], coloneqq: ['≔', 'rel'], triangleq: ['≜', 'rel'], therefore: ['∴', 'rel'],
  because: ['∵', 'rel'],

  infty: ['∞', 'ord'], partial: ['∂', 'ord'], nabla: ['∇', 'ord'], forall: ['∀', 'ord'], exists: ['∃', 'ord'],
  nexists: ['∄', 'ord'], neg: ['¬', 'ord'], lnot: ['¬', 'ord'], emptyset: ['∅', 'ord'], varnothing: ['∅', 'ord'],
  ldots: ['…', 'ord'], dots: ['…', 'ord'], cdots: ['⋯', 'ord'], vdots: ['⋮', 'ord'], ddots: ['⋱', 'ord'],
  prime: ['′', 'ord'], angle: ['∠', 'ord'], triangle: ['△', 'ord'], hbar: ['ℏ', 'ord'], ell: ['ℓ', 'ord'],
  Re: ['ℜ', 'ord'], Im: ['ℑ', 'ord'], aleph: ['ℵ', 'ord'], top: ['⊤', 'ord'], bot: ['⊥', 'ord'],
  degree: ['°', 'ord'], square: ['□', 'ord'], dagger: ['†', 'ord'],

  langle: ['⟨', 'open'], rangle: ['⟩', 'close'], lfloor: ['⌊', 'open'], rfloor: ['⌋', 'close'],
  lceil: ['⌈', 'open'], rceil: ['⌉', 'close'], lvert: ['|', 'open'], rvert: ['|', 'close'], vert: ['|', 'ord'],
  lVert: ['‖', 'open'], rVert: ['‖', 'close'], Vert: ['‖', 'ord'], lbrace: ['{', 'open'], rbrace: ['}', 'close'],
  lbrack: ['[', 'open'], rbrack: [']', 'close'],
  '{': ['{', 'open'], '}': ['}', 'close'], '|': ['‖', 'ord'], '%': ['%', 'ord'], '$': ['$', 'ord'],
  '&': ['&', 'ord'], '#': ['#', 'ord'], '_': ['_', 'ord'], colon: [':', 'punct'],
}

const SPACES: Readonly<Record<string, number>> = {
  ',': 1 / 6, ':': 2 / 9, '>': 2 / 9, ';': 5 / 18, '!': -1 / 6, ' ': 1 / 3, quad: 1, qquad: 2, enspace: 0.5,
  thinspace: 1 / 6, medspace: 2 / 9, thickspace: 5 / 18, negthinspace: -1 / 6,
}

const BIG: Readonly<Record<string, string>> = {
  sum: '∑', prod: '∏', coprod: '∐', int: '∫', iint: '∬', iiint: '∭', oint: '∮', bigcup: '⋃', bigcap: '⋂',
  bigoplus: '⨁', bigotimes: '⨂', bigvee: '⋁', bigwedge: '⋀',
}

const INTEGRALS = new Set(['int', 'iint', 'iiint', 'oint'])

const FUNCTIONS = new Set([
  'sin', 'cos', 'tan', 'cot', 'sec', 'csc', 'arcsin', 'arccos', 'arctan', 'sinh', 'cosh', 'tanh', 'coth',
  'log', 'ln', 'lg', 'exp', 'deg', 'dim', 'ker', 'arg', 'hom',
])

// Functions whose scripts sit under them in display: lim_{x→0}.
const LIMIT_FUNCTIONS: Readonly<Record<string, string>> = {
  lim: 'lim', liminf: 'lim inf', limsup: 'lim sup', max: 'max', min: 'min', sup: 'sup', inf: 'inf',
  det: 'det', gcd: 'gcd', Pr: 'Pr', argmax: 'arg max', argmin: 'arg min',
}

const ACCENTS: Readonly<Record<string, Accent>> = {
  hat: 'hat', widehat: 'hat', bar: 'bar', overline: 'bar', vec: 'vec', overrightarrow: 'vec', tilde: 'tilde',
  widetilde: 'tilde', dot: 'dot', ddot: 'ddot', underline: 'under',
}

const NEGATED: Readonly<Record<string, string>> = {
  '=': '≠', '∈': '∉', '<': '≮', '>': '≯', '≤': '≰', '≥': '≱', '⊂': '⊄', '⊆': '⊈', '∃': '∄', '≡': '≢',
  '∼': '≁', '≈': '≉', '∣': '∤', '∥': '∦',
}

// Capital letters in the math alphabets Unicode has; the gaps are older letterlike symbols.
const ALPHABETS: Readonly<Record<string, { start: number; held: Readonly<Record<string, string>> }>> = {
  mathbb: { start: 0x1d538, held: { C: 'ℂ', H: 'ℍ', N: 'ℕ', P: 'ℙ', Q: 'ℚ', R: 'ℝ', Z: 'ℤ' } },
  mathcal: { start: 0x1d49c, held: { B: 'ℬ', E: 'ℰ', F: 'ℱ', H: 'ℋ', I: 'ℐ', L: 'ℒ', M: 'ℳ', R: 'ℛ' } },
  mathscr: { start: 0x1d49c, held: { B: 'ℬ', E: 'ℰ', F: 'ℱ', H: 'ℋ', I: 'ℐ', L: 'ℒ', M: 'ℳ', R: 'ℛ' } },
  mathfrak: { start: 0x1d504, held: { C: 'ℭ', H: 'ℌ', I: 'ℑ', R: 'ℜ', Z: 'ℨ' } },
}

type Environment = { open: string; close: string; align: GridAlign }

const ENVIRONMENTS: Readonly<Record<string, Environment>> = {
  matrix: { open: '', close: '', align: 'center' },
  smallmatrix: { open: '', close: '', align: 'center' },
  pmatrix: { open: '(', close: ')', align: 'center' },
  bmatrix: { open: '[', close: ']', align: 'center' },
  Bmatrix: { open: '{', close: '}', align: 'center' },
  vmatrix: { open: '|', close: '|', align: 'center' },
  Vmatrix: { open: '‖', close: '‖', align: 'center' },
  cases: { open: '{', close: '', align: 'left' },
  dcases: { open: '{', close: '', align: 'left' },
  rcases: { open: '', close: '}', align: 'left' },
  aligned: { open: '', close: '', align: 'pairs' },
  align: { open: '', close: '', align: 'pairs' },
  alignat: { open: '', close: '', align: 'pairs' },
  split: { open: '', close: '', align: 'pairs' },
  eqnarray: { open: '', close: '', align: 'pairs' },
  flalign: { open: '', close: '', align: 'pairs' },
}

const OP_CHARS = new Set([...'+−×÷±∓⋅·∗∘∪∩⊕⊗∧∨'])
const REL_CHARS = new Set([...'=<>≤≥≠≈≡∼≃≅∝∈∉⊂⊆⊃⊇→←↔⇒⇐⇔⟹⟺↦≪≫∥'])

const ASCII: Readonly<Record<string, readonly [string, Role]>> = {
  '-': ['−', 'op'], '*': ['∗', 'op'], ':': [':', 'rel'], ',': [',', 'punct'], ';': [';', 'punct'],
  '(': ['(', 'open'], '[': ['[', 'open'], ')': [')', 'close'], ']': [']', 'close'], '!': ['!', 'close'],
  '?': ['?', 'close'],
}

// --- Parsing ------------------------------------------------------------------------

type Token = { kind: 'cmd'; name: string } | { kind: 'char'; text: string }

function tokenize(tex: string): Token[] {
  const chars = [...tex]
  const tokens: Token[] = []

  for (let i = 0; i < chars.length; i++) {
    const char = chars[i] ?? ''

    if (char !== '\\') {
      tokens.push({ kind: 'char', text: char })
      continue
    }

    const next = chars[i + 1]

    if (next === undefined) {
      break
    }

    if (!/[A-Za-z]/.test(next)) {
      tokens.push({ kind: 'cmd', name: next })
      i++
      continue
    }

    let name = ''

    while (/[A-Za-z]/.test(chars[i + 1] ?? '')) {
      name += chars[i + 1]
      i++
    }

    if (name === 'operatorname' && chars[i + 1] === '*') {
      name += '*'
      i++
    }

    tokens.push({ kind: 'cmd', name })
  }

  return tokens
}

const sym = (text: string, role: Role, italic = false): Sym => ({ kind: 'sym', text, role, italic })

const EMPTY: MathNode = { kind: 'row', items: [] }

const rowOf = (items: MathNode[]): MathNode => (items.length === 1 && items[0] !== undefined ? items[0] : { kind: 'row', items })

const isChar = (text: string) => (token: Token): boolean => token.kind === 'char' && token.text === text

const isCmd = (name: string) => (token: Token): boolean => token.kind === 'cmd' && token.name === name

const ENDS_CELL = (token: Token): boolean =>
  (token.kind === 'char' && token.text === '&') || (token.kind === 'cmd' && (token.name === '\\' || token.name === 'end'))

const ENDS_LINE = (token: Token): boolean =>
  (token.kind === 'char' && token.text === '&') || (token.kind === 'cmd' && token.name === '\\')

// Every symbol in a subtree changed, for \mathrm, \mathbf and the math alphabets.
function restyle(node: MathNode, change: (node: Sym) => Sym): MathNode {
  switch (node.kind) {
    case 'sym':
      return change(node)
    case 'row':
      return { ...node, items: node.items.map(item => restyle(item, change)) }
    case 'scripts':
      return { ...node, base: restyle(node.base, change) }
    case 'accent':
      return { ...node, body: restyle(node.body, change) }
    default:
      return node
  }
}

const inAlphabet = (name: string) => (node: Sym): Sym => {
  const alphabet = ALPHABETS[name]

  if (alphabet === undefined) {
    return node
  }

  const text = [...node.text]
    .map(char => alphabet.held[char] ?? (/[A-Z]/.test(char) ? String.fromCodePoint(alphabet.start + char.charCodeAt(0) - 65) : char))
    .join('')

  return { ...node, text, italic: false }
}

class Parser {
  private at = 0

  constructor(private readonly tokens: readonly Token[]) {}

  // The whole formula: one node, or a grid when it has `\\` or `&` at the top.
  top(): MathNode {
    const rows = this.grid(ENDS_LINE)
    const only = rows.length === 1 ? rows[0] : undefined

    if (only !== undefined && only.length === 1 && only[0] !== undefined) {
      return only[0]
    }

    return { kind: 'grid', rows, align: rows.some(cells => cells.length > 1) ? 'pairs' : 'center' }
  }

  private peek(): Token | undefined {
    return this.tokens[this.at]
  }

  private take(): Token | undefined {
    const token = this.tokens[this.at]
    this.at++

    return token
  }

  private skipSpace(): void {
    for (let token = this.peek(); token?.kind === 'char' && /\s/.test(token.text); token = this.peek()) {
      this.at++
    }
  }

  private next(test: (token: Token) => boolean): boolean {
    const token = this.peek()

    return token !== undefined && test(token)
  }

  private list(stop: (token: Token) => boolean): MathNode[] {
    const items: MathNode[] = []

    for (;;) {
      this.skipSpace()
      const token = this.peek()

      if (token === undefined || stop(token)) {
        return items
      }

      const atom = this.atom()

      if (atom !== null) {
        items.push(this.scripts(atom))
      }
    }
  }

  // Cells split on `&`, rows on `\\`, up to `\end` inside an environment or the end.
  private grid(stop: (token: Token) => boolean): MathNode[][] {
    const rows: MathNode[][] = []
    let cells: MathNode[] = []

    for (;;) {
      cells.push(rowOf(this.list(stop)))
      const token = this.take()

      if (token?.kind === 'char') {
        continue
      }

      if (token?.kind === 'cmd' && token.name === '\\') {
        rows.push(cells)
        cells = []
        this.skipSpace()

        // A row's extra spacing, `\\[4pt]`.
        if (this.next(isChar('['))) {
          this.list(isChar(']'))
          this.take()
        }

        continue
      }

      if (token?.kind === 'cmd' && token.name === 'end') {
        this.raw()
      }

      rows.push(cells)
      break
    }

    const last = rows.at(-1)

    if (rows.length > 1 && last?.length === 1 && last[0]?.kind === 'row' && last[0].items.length === 0) {
      rows.pop()
    }

    return rows
  }

  private group(close: string): MathNode {
    const items = this.list(isChar(close))
    this.take()

    return rowOf(items)
  }

  // An argument: a braced group, or a single token as TeX takes it (\frac12 is ½).
  private arg(): MathNode {
    this.skipSpace()
    const token = this.peek()

    if (token === undefined) {
      return EMPTY
    }

    if (token.kind === 'char' && token.text === '{') {
      this.at++

      return this.group('}')
    }

    if (token.kind === 'char' && /[0-9]/.test(token.text)) {
      this.at++

      return sym(token.text, 'num')
    }

    return this.atom() ?? EMPTY
  }

  private optional(): MathNode | null {
    this.skipSpace()

    if (!this.next(isChar('['))) {
      return null
    }

    this.at++

    return this.group(']')
  }

  // A braced argument as plain text, for \text, \operatorname and environment names.
  private raw(): string {
    this.skipSpace()
    const first = this.take()

    if (first === undefined) {
      return ''
    }

    if (!(first.kind === 'char' && first.text === '{')) {
      return first.kind === 'char' ? first.text : (GREEK[first.name] ?? SYMBOLS[first.name]?.[0] ?? first.name)
    }

    let text = ''
    let depth = 1

    for (let token = this.take(); token !== undefined; token = this.take()) {
      if (token.kind === 'char') {
        depth += token.text === '{' ? 1 : token.text === '}' ? -1 : 0

        if (depth === 0) {
          break
        }

        text += token.text === '{' || token.text === '}' || token.text === '$' ? '' : token.text
      } else {
        text += token.name.length === 1 ? token.name : (GREEK[token.name] ?? SYMBOLS[token.name]?.[0] ?? '')
      }
    }

    return text.replace(/\s+/g, ' ')
  }

  private delimiter(): string {
    this.skipSpace()
    const token = this.take()

    if (token === undefined) {
      return ''
    }

    if (token.kind === 'char') {
      return token.text === '.' ? '' : token.text
    }

    return SYMBOLS[token.name]?.[0] ?? ''
  }

  private scripts(base: MathNode): MathNode {
    let node = base
    let sup: MathNode | null = null
    let sub: MathNode | null = null
    let primes = ''

    for (;;) {
      this.skipSpace()

      if (this.next(isCmd('limits')) || this.next(isCmd('nolimits'))) {
        const limits = this.next(isCmd('limits'))
        this.at++
        node = node.kind === 'sym' ? { ...node, limits } : node
      } else if (this.next(isChar("'"))) {
        this.at++
        primes += '′'
      } else if (sup === null && this.next(isChar('^'))) {
        this.at++
        sup = this.arg()
      } else if (sub === null && this.next(isChar('_'))) {
        this.at++
        sub = this.arg()
      } else {
        break
      }
    }

    const marks = primes === '' ? sup : rowOf([sym(primes, 'ord'), ...(sup === null ? [] : [sup])])

    return marks === null && sub === null ? node : { kind: 'scripts', base: node, sup: marks, sub }
  }

  private atom(): MathNode | null {
    const token = this.take()

    if (token === undefined) {
      return null
    }

    return token.kind === 'char' ? this.character(token.text) : this.command(token.name)
  }

  private character(text: string): MathNode | null {
    if (text === '{') {
      return this.group('}')
    }

    if (text === '}' || text === '&') {
      return null
    }

    // Scripts with nothing before them, as in {}^{14}C: an empty base the loop picks up.
    if (text === '^' || text === '_' || text === "'") {
      this.at--

      return { kind: 'row', items: [] }
    }

    if (text === '~') {
      return { kind: 'space', em: 1 / 3 }
    }

    if (/[0-9]/.test(text)) {
      let number = text

      for (let token = this.peek(); token?.kind === 'char'; token = this.peek()) {
        const after = this.tokens[this.at + 1]
        const isDigit = /[0-9]/.test(token.text)
        const isPoint = token.text === '.' && after?.kind === 'char' && /[0-9]/.test(after.text)

        if (!isDigit && !isPoint) {
          break
        }

        number += token.text
        this.at++
      }

      return sym(number, 'num')
    }

    if (/\p{L}/u.test(text)) {
      return sym(text, 'ord', true)
    }

    const ascii = ASCII[text]

    if (ascii !== undefined) {
      return sym(ascii[0], ascii[1])
    }

    return sym(text, OP_CHARS.has(text) ? 'op' : REL_CHARS.has(text) ? 'rel' : 'ord')
  }

  private command(name: string): MathNode | null {
    const greek = GREEK[name]

    if (greek !== undefined) {
      return sym(greek, 'ord', /^[a-z]/.test(name))
    }

    const symbol = SYMBOLS[name]

    if (symbol !== undefined) {
      return sym(symbol[0], symbol[1])
    }

    const space = SPACES[name]

    if (space !== undefined) {
      return { kind: 'space', em: space }
    }

    const big = BIG[name]

    if (big !== undefined) {
      return { ...sym(big, 'big'), limits: !INTEGRALS.has(name) }
    }

    if (FUNCTIONS.has(name)) {
      return sym(name, 'fn')
    }

    const limited = LIMIT_FUNCTIONS[name]

    if (limited !== undefined) {
      return { ...sym(limited, 'fn'), limits: true }
    }

    const accent = ACCENTS[name]

    if (accent !== undefined) {
      return { kind: 'accent', mark: accent, body: this.arg() }
    }

    switch (name) {
      case 'frac':
      case 'dfrac':
      case 'tfrac':
      case 'cfrac': {
        const num = this.arg()

        return { kind: 'frac', num, den: this.arg(), bar: true }
      }
      case 'binom':
      case 'dbinom':
      case 'tbinom': {
        const num = this.arg()

        return { kind: 'fence', open: '(', close: ')', body: { kind: 'frac', num, den: this.arg(), bar: false } }
      }
      case 'sqrt': {
        const index = this.optional()

        return { kind: 'sqrt', index, body: this.arg() }
      }
      case 'left': {
        const open = this.delimiter()
        const items = this.list(isCmd('right'))
        this.take()

        return { kind: 'fence', open, close: this.delimiter(), body: rowOf(items) }
      }
      case 'middle':
        return sym(this.delimiter(), 'rel')
      case 'big':
      case 'Big':
      case 'bigg':
      case 'Bigg':
      case 'bigm':
      case 'Bigm':
        return sym(this.delimiter(), 'ord')
      case 'bigl':
      case 'Bigl':
      case 'biggl':
      case 'Biggl':
        return sym(this.delimiter(), 'open')
      case 'bigr':
      case 'Bigr':
      case 'biggr':
      case 'Biggr':
        return sym(this.delimiter(), 'close')
      case 'begin':
        return this.environment()
      case 'end':
        this.raw()

        return null
      case 'text':
      case 'textrm':
      case 'textit':
      case 'textbf':
      case 'textsf':
      case 'texttt':
      case 'textnormal':
      case 'mbox':
      case 'hbox':
        return sym(this.raw(), 'word')
      case 'operatorname':
      case 'mathop':
        return sym(this.raw(), 'fn')
      case 'operatorname*':
        return { ...sym(this.raw(), 'fn'), limits: true }
      case 'mathrm':
      case 'mathsf':
      case 'mathtt':
      case 'mathup':
      case 'rm':
        return restyle(this.arg(), node => ({ ...node, italic: false }))
      case 'mathit':
        return restyle(this.arg(), node => ({ ...node, italic: /\p{L}/u.test(node.text) }))
      case 'mathbf':
      case 'boldsymbol':
      case 'bm':
        return restyle(this.arg(), node => ({ ...node, bold: true }))
      case 'mathbb':
      case 'mathcal':
      case 'mathscr':
      case 'mathfrak':
        return restyle(this.arg(), inAlphabet(name))
      case 'not': {
        const next = this.atom()
        const text = next?.kind === 'sym' ? next.text : ''

        return sym(NEGATED[text] ?? `${text}̸`, 'rel')
      }
      case 'pmod':
        return rowOf([{ kind: 'space', em: 1 }, sym('(', 'open'), sym('mod', 'fn'), this.arg(), sym(')', 'close')])
      case 'tag':
        return rowOf([{ kind: 'space', em: 2 }, sym(`(${this.raw()})`, 'word')])
      case 'overset':
      case 'stackrel': {
        const over = this.arg()
        const base = this.arg()

        return { kind: 'scripts', base: base.kind === 'sym' ? { ...base, limits: true } : base, sup: over, sub: null }
      }
      case 'underset': {
        const under = this.arg()
        const base = this.arg()

        return { kind: 'scripts', base: base.kind === 'sym' ? { ...base, limits: true } : base, sup: null, sub: under }
      }
      case 'textcolor':
        this.raw()

        return this.arg()
      case 'hspace':
        this.raw()

        return { kind: 'space', em: 0.5 }
      case 'label':
      case 'color':
      case 'vspace':
        this.raw()

        return null
      case 'phantom':
      case 'hphantom':
      case 'vphantom':
        this.arg()

        return null
      case 'overbrace':
      case 'underbrace':
      case 'boxed':
      case 'displaystyle':
      case 'textstyle':
      case 'scriptstyle':
      case 'scriptscriptstyle':
      case 'nonumber':
      case 'notag':
      case 'right':
      case 'limits':
      case 'nolimits':
      case 'mathstrut':
      case 'strut':
      case '\\':
        return name === 'overbrace' || name === 'underbrace' || name === 'boxed' ? this.arg() : null
      default:
        return sym(`\\${name}`, 'word')
    }
  }

  private environment(): MathNode {
    const name = this.raw().trim().replace(/\*$/, '')

    // The column spec of an array, the column count of alignat.
    if (name === 'array' || name === 'alignat') {
      this.raw()
    }

    const rows = this.grid(ENDS_CELL)
    const shape = ENVIRONMENTS[name] ?? { open: '', close: '', align: 'center' }
    const grid: MathNode = { kind: 'grid', rows, align: shape.align }

    return shape.open === '' && shape.close === '' ? grid : { kind: 'fence', open: shape.open, close: shape.close, body: grid }
  }
}

export const parseTex = (tex: string): MathNode => new Parser(tokenize(tex)).top()

// --- Spacing ------------------------------------------------------------------------

const THIN = 1 / 6
const MEDIUM = 2 / 9
const THICK = 5 / 18

// Roles after which an operator is a sign, not a binary operator: −x, a = −b.
const SIGN_AFTER = new Set<Role>(['op', 'rel', 'open', 'punct', 'big', 'fn'])

const roleOf = (node: MathNode): Role =>
  node.kind === 'sym' ? node.role : node.kind === 'scripts' ? roleOf(node.base) : 'ord'

// The space before each item, in em, by TeX's rules cut down: a medium space around binary
// operators, a thick one around relations, a thin one after punctuation and after a
// function name. Scripts (`compact`) keep only the one after a function name.
export function gapsOf(items: readonly MathNode[], compact: boolean): number[] {
  const roles: Role[] = []

  for (const item of items) {
    const role = roleOf(item)
    const before = roles.at(-1)
    roles.push(role === 'op' && (before === undefined || SIGN_AFTER.has(before)) ? 'ord' : role)
  }

  return roles.map((role, i) => {
    const before = roles[i - 1]

    if (before === undefined) {
      // A cell of `aligned` starts at its relation: `&= x` keeps the space before `=`.
      return !compact && role === 'rel' ? THICK : 0
    }

    const afterName = (before === 'fn' || before === 'big') && !['open', 'close', 'punct', 'rel', 'op'].includes(role)

    if (compact) {
      return before === 'fn' && afterName ? THIN : 0
    }

    if (role === 'op' || before === 'op') {
      return MEDIUM
    }

    if (role === 'rel' || before === 'rel') {
      return role === before ? 0 : THICK
    }

    return before === 'punct' || afterName ? THIN : 0
  })
}

// --- One line of Unicode ------------------------------------------------------------

const SUP: Readonly<Record<string, string>> = {
  '0': '⁰', '1': '¹', '2': '²', '3': '³', '4': '⁴', '5': '⁵', '6': '⁶', '7': '⁷', '8': '⁸', '9': '⁹',
  '+': '⁺', '−': '⁻', '-': '⁻', '=': '⁼', '(': '⁽', ')': '⁾', '′': '′',
  a: 'ᵃ', b: 'ᵇ', c: 'ᶜ', d: 'ᵈ', e: 'ᵉ', f: 'ᶠ', g: 'ᵍ', h: 'ʰ', i: 'ⁱ', j: 'ʲ', k: 'ᵏ', l: 'ˡ', m: 'ᵐ',
  n: 'ⁿ', o: 'ᵒ', p: 'ᵖ', r: 'ʳ', s: 'ˢ', t: 'ᵗ', u: 'ᵘ', v: 'ᵛ', w: 'ʷ', x: 'ˣ', y: 'ʸ', z: 'ᶻ',
  A: 'ᴬ', B: 'ᴮ', D: 'ᴰ', E: 'ᴱ', G: 'ᴳ', H: 'ᴴ', I: 'ᴵ', J: 'ᴶ', K: 'ᴷ', L: 'ᴸ', M: 'ᴹ', N: 'ᴺ',
  O: 'ᴼ', P: 'ᴾ', R: 'ᴿ', T: 'ᵀ', U: 'ᵁ', V: 'ⱽ', W: 'ᵂ',
  α: 'ᵅ', β: 'ᵝ', γ: 'ᵞ', δ: 'ᵟ', ε: 'ᵋ', θ: 'ᶿ', φ: 'ᵠ', ϕ: 'ᵠ', χ: 'ᵡ',
}

const SUB: Readonly<Record<string, string>> = {
  '0': '₀', '1': '₁', '2': '₂', '3': '₃', '4': '₄', '5': '₅', '6': '₆', '7': '₇', '8': '₈', '9': '₉',
  '+': '₊', '−': '₋', '-': '₋', '=': '₌', '(': '₍', ')': '₎',
  a: 'ₐ', e: 'ₑ', h: 'ₕ', i: 'ᵢ', j: 'ⱼ', k: 'ₖ', l: 'ₗ', m: 'ₘ', n: 'ₙ', o: 'ₒ', p: 'ₚ', r: 'ᵣ', s: 'ₛ',
  t: 'ₜ', u: 'ᵤ', v: 'ᵥ', x: 'ₓ', β: 'ᵦ', γ: 'ᵧ', ρ: 'ᵨ', φ: 'ᵩ', ϕ: 'ᵩ', χ: 'ᵪ',
}

const VULGAR: Readonly<Record<string, string>> = {
  '1/2': '½', '1/3': '⅓', '2/3': '⅔', '1/4': '¼', '3/4': '¾', '1/5': '⅕', '1/6': '⅙', '1/8': '⅛',
}

const COMBINING: Readonly<Record<Accent, string>> = {
  hat: '̂', bar: '̄', vec: '⃗', tilde: '̃', dot: '̇', ddot: '̈', under: '̲',
}

// `text` in super- or subscript letters, or null when one of them has none.
function mapped(text: string, table: Readonly<Record<string, string>>): string | null {
  let out = ''

  for (const char of text) {
    const to = table[char]

    if (to === undefined) {
      return null
    }

    out += to
  }

  return out
}

// One thing on its own, needing no brackets to keep it together after a `/` or `^`.
const isAtomic = (node: MathNode): boolean =>
  node.kind === 'sym' || node.kind === 'sqrt' || node.kind === 'fence' || node.kind === 'accent' ||
  (node.kind === 'scripts' && isAtomic(node.base)) || (node.kind === 'row' && node.items.length === 1 && node.items[0] !== undefined && isAtomic(node.items[0]))

const kept = (node: MathNode, text: string): string => (isAtomic(node) || text === '' ? text : `(${text})`)

function scriptLine(node: MathNode | null, table: Readonly<Record<string, string>>, mark: string): string {
  if (node === null) {
    return ''
  }

  const text = linear(node, true)

  return mapped(text, table) ?? (isAtomic(node) && [...text].length === 1 ? `${mark}${text}` : `${mark}(${text})`)
}

function gridLine(rows: readonly MathNode[][], align: GridAlign, rowJoin: string): string {
  const cellJoin = align === 'pairs' ? '' : align === 'left' ? '  ' : ' '

  return rows.map(cells => cells.map(cell => linear(cell)).join(cellJoin).trim()).join(rowJoin)
}

// The formula as one line of Unicode (a line a row, for a grid at the top). `compact`
// drops the spaces around operators, as scripts do.
export function linear(node: MathNode, compact = false): string {
  switch (node.kind) {
    case 'sym':
      return node.text
    case 'space':
      return compact || node.em <= 0.1 ? '' : node.em >= 1.5 ? '    ' : node.em >= 0.9 ? '  ' : ' '
    case 'row': {
      const gaps = gapsOf(node.items, compact)

      return node.items.map((item, i) => ((gaps[i] ?? 0) > 0 ? ' ' : '') + linear(item, compact)).join('')
    }
    case 'frac': {
      const num = linear(node.num, compact)
      const den = linear(node.den, compact)

      if (!node.bar) {
        return `C(${num}, ${den})`
      }

      const digits = /^\d{1,3}$/.test(num) && /^\d{1,3}$/.test(den)

      return VULGAR[`${num}/${den}`] ?? (digits ? `${mapped(num, SUP) ?? num}⁄${mapped(den, SUB) ?? den}` : `${kept(node.num, num)}/${kept(node.den, den)}`)
    }
    case 'sqrt': {
      const index = node.index === null ? '' : linear(node.index, true)
      const root = index === '3' ? '∛' : index === '4' ? '∜' : `${mapped(index, SUP) ?? index}√`

      return root + kept(node.body, linear(node.body, compact))
    }
    case 'scripts':
      return kept(node.base, linear(node.base, compact)) + scriptLine(node.sub, SUB, '_') + scriptLine(node.sup, SUP, '^')
    case 'fence':
      return node.open + (node.body.kind === 'grid' ? gridLine(node.body.rows, node.body.align, '; ') : linear(node.body, compact)) + node.close
    case 'grid':
      return gridLine(node.rows, node.align, '\n')
    case 'accent': {
      const body = linear(node.body, compact)
      const mark = COMBINING[node.mark]

      return node.mark === 'bar' || node.mark === 'under' ? [...body].map(char => char + mark).join('') : body + mark
    }
  }
}

// --- Terminal drawing ---------------------------------------------------------------

// Rows of equal width in cells, `base` the row the formula's baseline runs through.
type Art = { lines: string[]; base: number; width: number }

const blank = (width: number): string => ' '.repeat(Math.max(0, width))

function fill(line: string, width: number, align: 'left' | 'right' | 'center' = 'left'): string {
  const room = Math.max(0, width - widthOf(line))
  const left = align === 'right' ? room : align === 'center' ? Math.floor(room / 2) : 0

  return blank(left) + line + blank(room - left)
}

const flat = (text: string): Art => ({ lines: [text], base: 0, width: widthOf(text) })

// Side by side on a shared baseline.
function beside(parts: readonly Art[]): Art {
  const up = Math.max(0, ...parts.map(part => part.base))
  const down = Math.max(0, ...parts.map(part => part.lines.length - 1 - part.base))
  const lines = Array.from({ length: up + down + 1 }, (_, r) =>
    parts.map(part => part.lines[r - up + part.base] ?? blank(part.width)).join(''),
  )

  return { lines, base: up, width: parts.reduce((sum, part) => sum + part.width, 0) }
}

function stacked(parts: readonly Art[], base: number, align: 'left' | 'center' = 'center'): Art {
  const width = Math.max(0, ...parts.map(part => part.width))

  return { lines: parts.flatMap(part => part.lines.map(line => fill(line, width, align))), base, width }
}

// Tall delimiters from the bracket pieces: top, extension, bottom, and the middle piece.
const TALL: Readonly<Record<string, readonly [string, string, string, string]>> = {
  '(': ['⎛', '⎜', '⎝', '⎜'],
  ')': ['⎞', '⎟', '⎠', '⎟'],
  '[': ['⎡', '⎢', '⎣', '⎢'],
  ']': ['⎤', '⎥', '⎦', '⎥'],
  '{': ['⎧', '⎪', '⎩', '⎨'],
  '}': ['⎫', '⎪', '⎭', '⎬'],
  '⌊': ['⎢', '⎢', '⎣', '⎢'],
  '⌋': ['⎥', '⎥', '⎦', '⎥'],
  '⌈': ['⎡', '⎢', '⎢', '⎢'],
  '⌉': ['⎤', '⎥', '⎥', '⎥'],
}

function tall(delimiter: string, height: number, base: number): Art {
  if (delimiter === '') {
    return { lines: Array.from({ length: height }, () => ''), base, width: 0 }
  }

  const pieces = TALL[delimiter]
  const middle = Math.floor((height - 1) / 2)
  const lines = Array.from({ length: height }, (_, r) => {
    if (pieces !== undefined) {
      return r === 0 ? pieces[0] : r === height - 1 ? pieces[2] : r === middle && height > 2 ? pieces[3] : pieces[1]
    }

    if (delimiter === '⟨' || delimiter === '⟩') {
      return (r < height / 2) === (delimiter === '⟨') ? '╱' : '╲'
    }

    return delimiter === '|' ? '│' : delimiter === '‖' ? '║' : r === middle ? delimiter : ' '
  })

  return { lines, base, width: widthOf(lines[0] ?? '') }
}

// Marks drawn on the row above a body too wide to take a combining mark.
const OVER: Readonly<Record<Accent, string>> = { hat: '^', bar: '_', vec: '→', tilde: '~', dot: '·', ddot: '¨', under: '‾' }

function art(node: MathNode, compact: boolean): Art {
  switch (node.kind) {
    case 'sym':
    case 'space':
      return flat(linear(node, compact))
    case 'row': {
      const gaps = gapsOf(node.items, compact)

      return beside(node.items.flatMap((item, i) => ((gaps[i] ?? 0) > 0 ? [flat(' '), art(item, compact)] : [art(item, compact)])))
    }
    case 'frac': {
      if (compact) {
        return flat(linear(node, true))
      }

      const num = art(node.num, false)
      const den = art(node.den, false)

      if (!node.bar) {
        return stacked([num, den], num.lines.length - 1)
      }

      return stacked([num, flat('─'.repeat(Math.max(num.width, den.width) + 2)), den], num.lines.length)
    }
    case 'sqrt': {
      if (compact) {
        return flat(linear(node, true))
      }

      const body = art(node.body, false)
      const index = node.index === null ? '' : linear(node.index, true)
      const mark = mapped(index, SUP) ?? index
      const height = body.lines.length

      if (height === 1) {
        const lead = `${mark}√`

        return { lines: [blank(widthOf(lead)) + '_'.repeat(body.width), lead + (body.lines[0] ?? '')], base: 1, width: widthOf(lead) + body.width }
      }

      // The radical climbs one cell a row, so it takes as many cells as the body has rows.
      const lead = height + 1
      const bar = (widthOf(mark) < lead ? fill(mark, lead) : blank(lead)) + '_'.repeat(body.width)
      const rows = body.lines.map((line, r) => (r < height - 1 ? `${blank(height - r)}╱${blank(r)}` : `╲╱${blank(height - 1)}`) + line)

      return { lines: [bar, ...rows], base: body.base + 1, width: lead + body.width }
    }
    case 'scripts':
      return scriptsArt(node.base, node.sup, node.sub, compact)
    case 'fence': {
      const body = art(node.body, compact)

      if (body.lines.length === 1) {
        return beside([flat(node.open), body, flat(node.close)])
      }

      const pad = node.body.kind === 'grid' ? [flat(' ')] : []
      const left = tall(node.open, body.lines.length, body.base)
      const right = tall(node.close, body.lines.length, body.base)

      return beside([left, ...(left.width > 0 ? pad : []), body, ...(right.width > 0 ? pad : []), right])
    }
    case 'grid':
      return gridArt(node.rows, node.align, compact)
    case 'accent': {
      const body = art(node.body, compact)

      if (body.lines.length === 1 && [...(body.lines[0] ?? '')].length === 1) {
        return flat((body.lines[0] ?? '') + COMBINING[node.mark])
      }

      if (node.mark === 'under') {
        return { lines: [...body.lines, '‾'.repeat(body.width)], base: body.base, width: body.width }
      }

      const over = node.mark === 'bar' ? '_'.repeat(body.width) : node.mark === 'vec' ? `${'─'.repeat(Math.max(0, body.width - 1))}→` : fill(OVER[node.mark], body.width, 'center')

      return { lines: [over, ...body.lines], base: body.base + 1, width: body.width }
    }
  }
}

function scriptsArt(baseNode: MathNode, supNode: MathNode | null, subNode: MathNode | null, compact: boolean): Art {
  const base = art(baseNode, compact)

  // ∑ and lim in display: the scripts stacked over and under.
  if (!compact && baseNode.kind === 'sym' && baseNode.limits === true) {
    const sup = supNode === null ? [] : [art(supNode, true)]
    const sub = subNode === null ? [] : [art(subNode, true)]

    return stacked([...sup, base, ...sub], (sup[0]?.lines.length ?? 0) + base.base)
  }

  const supText = supNode === null ? '' : mapped(linear(supNode, true), SUP)
  const subText = subNode === null ? '' : mapped(linear(subNode, true), SUB)

  if (base.lines.length === 1 && supText !== null && subText !== null) {
    return flat((base.lines[0] ?? '') + subText + supText)
  }

  // Otherwise the scripts take their own rows: the superscript ends on the row above the
  // baseline, the subscript starts on the row below it.
  const sup = supNode === null ? null : art(supNode, true)
  const sub = subNode === null ? null : art(subNode, true)
  const supTop = sup === null ? 0 : base.base - sup.lines.length
  const subTop = base.base + 1
  const top = Math.min(0, supTop)
  const bottom = Math.max(base.lines.length, sub === null ? 0 : subTop + sub.lines.length)
  const side = Math.max(sup?.width ?? 0, sub?.width ?? 0)
  const lines = Array.from({ length: bottom - top }, (_, k) => {
    const r = k + top
    const script = sup?.lines[r - supTop] ?? sub?.lines[r - subTop] ?? ''

    return (base.lines[r] ?? blank(base.width)) + fill(script, side)
  })

  return { lines, base: base.base - top, width: base.width + side }
}

function gridArt(rows: readonly MathNode[][], align: GridAlign, compact: boolean): Art {
  const cells = rows.map(cells => cells.map(cell => art(cell, compact)))
  const columns = Math.max(0, ...cells.map(row => row.length))
  const widths = Array.from({ length: columns }, (_, c) => Math.max(0, ...cells.map(row => row[c]?.width ?? 0)))
  const gap = (c: number): number => (c === 0 ? 0 : align === 'pairs' ? (c % 2 === 1 ? 0 : 3) : 2)
  const side = (c: number): 'left' | 'right' | 'center' => (align === 'pairs' ? (c % 2 === 0 ? 'right' : 'left') : align === 'left' ? 'left' : 'center')
  const lines = cells.map(row =>
    beside(
      widths.map((width, c) => {
        const cell = row[c] ?? flat('')

        return { lines: cell.lines.map(line => blank(gap(c)) + fill(line, width, side(c))), base: cell.base, width: gap(c) + width }
      }),
    ),
  )
  const height = lines.reduce((sum, line) => sum + line.lines.length, 0)

  return stacked(lines, Math.floor((height - 1) / 2), 'left')
}

// The formula in terminal rows: drawn over several rows where `maxWidth` has room for
// it, otherwise as Unicode lines.
export function mathArt(node: MathNode, maxWidth: number): string[] {
  const drawn = art(node, false)
  const lines = drawn.width <= maxWidth ? drawn.lines : linear(node).split('\n')

  return lines.map(line => line.replace(/\s+$/, ''))
}

// --- Inline math --------------------------------------------------------------------

// `$x$` as Pandoc reads it (no space inside either dollar, no digit after the closing
// one, so "$5 and $10" stays money), `\(x\)`, and `$$x$$` inside a line. Code spans
// come first so the formula inside one is left alone.
const INLINE = /(`+)[^`]*?\1|\\\((.+?)\\\)|\$\$([^$\n]+?)\$\$|(?<![\\$\w])\$([^$\s](?:[^$\n]*[^$\s\\])?)\$(?![\w$])/g

// A dollar pair only counts when what is inside reads as math.
const looksLikeMath = (tex: string): boolean =>
  /[\\^_=<>]/.test(tex) || /^[A-Za-z](\([^()]*\))?$/.test(tex) || (/[A-Za-z]/.test(tex) && /^[A-Za-z0-9]+(\s*[+\-*/]\s*[A-Za-z0-9]+)+$/.test(tex))

// Markdown would read a `_` or `*` in the Unicode as emphasis.
const markdownSafe = (text: string): string => text.replace(/[\\*_`]/g, '\\$&')

const FENCE_LINE = /^\s*(```|~~~)/

// Inline formulas in markdown turned into Unicode, outside code spans and fences.
export function inlineMath(markdown: string): string {
  let inFence = false

  return markdown
    .split('\n')
    .map(line => {
      if (FENCE_LINE.test(line)) {
        inFence = !inFence

        return line
      }

      if (inFence || !/[$\\]/.test(line)) {
        return line
      }

      return line.replace(INLINE, (whole: string, ticks: string | undefined, paren: string | undefined, display: string | undefined, dollar: string | undefined) => {
        const tex = paren ?? display ?? dollar

        if (ticks !== undefined || tex === undefined || (dollar !== undefined && !looksLikeMath(dollar))) {
          return whole
        }

        return markdownSafe(linear(parseTex(tex)))
      })
    })
    .join('\n')
}
