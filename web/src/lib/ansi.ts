import type { CSSProperties } from 'react'

export type AnsiSegment = { text: string; style?: CSSProperties }

// CSI sequences: SGR ("m") are interpreted, every other final byte is dropped.
// A bare ESC-less "[0m" is also accepted because some game servers log
// through pipes that swallow the ESC byte.
// oxlint-disable-next-line no-control-regex
const CSI = /(?:\u001b\[|\[(?=[\d;]*m))([\d;:]*)([\x20-\x2f]*[\x40-\x7e])/g
// Remaining non-CSI escapes (OSC titles, charset selection, ...).
// oxlint-disable-next-line no-control-regex
const OTHER_ESCAPES = /\u001b(?:\][^\u0007\u001b]*(?:\u0007|\u001b\\)|[()][\x20-\x7e]|[@-Z\\-_])/g

type State = { fg?: string; bg?: string; bold: boolean; dim: boolean; italic: boolean; underline: boolean; inverse: boolean }

const blank = (): State => ({ fg: undefined, bg: undefined, bold: false, dim: false, italic: false, underline: false, inverse: false })

// 0-15 come from theme-aware CSS variables (see index.css); the rest are fixed.
function paletteColor(index: number): string | undefined {
  if (index < 0 || index > 255) return undefined
  if (index < 16) return `var(--ansi-${index})`
  if (index >= 232) {
    const level = 8 + (index - 232) * 10
    return `rgb(${level} ${level} ${level})`
  }
  const cube = index - 16
  const channel = (n: number) => (n === 0 ? 0 : 55 + n * 40)
  return `rgb(${channel(Math.floor(cube / 36))} ${channel(Math.floor(cube / 6) % 6)} ${channel(cube % 6)})`
}

function extendedColor(params: number[], i: number): [string | undefined, number] {
  if (params[i + 1] === 5) return [paletteColor(params[i + 2]), 2]
  if (params[i + 1] === 2) {
    const [r, g, b] = params.slice(i + 2, i + 5)
    return [[r, g, b].every((n) => n >= 0 && n <= 255) ? `rgb(${r} ${g} ${b})` : undefined, 4]
  }
  return [undefined, 0]
}

function applySgr(state: State, rawParams: string) {
  const params = rawParams === '' ? [0] : rawParams.split(/[;:]/).map((p) => (p === '' ? 0 : Number(p)))
  for (let i = 0; i < params.length; i++) {
    const code = params[i]
    if (code === 0) Object.assign(state, blank())
    else if (code === 1) state.bold = true
    else if (code === 2) state.dim = true
    else if (code === 3) state.italic = true
    else if (code === 4) state.underline = true
    else if (code === 7) state.inverse = true
    else if (code === 22) state.bold = state.dim = false
    else if (code === 23) state.italic = false
    else if (code === 24) state.underline = false
    else if (code === 27) state.inverse = false
    else if (code >= 30 && code <= 37) state.fg = paletteColor(code - 30)
    else if (code >= 90 && code <= 97) state.fg = paletteColor(code - 90 + 8)
    else if (code === 39) state.fg = undefined
    else if (code >= 40 && code <= 47) state.bg = paletteColor(code - 40)
    else if (code >= 100 && code <= 107) state.bg = paletteColor(code - 100 + 8)
    else if (code === 49) state.bg = undefined
    else if (code === 38 || code === 48) {
      const [color, consumed] = extendedColor(params, i)
      if (code === 38) state.fg = color
      else state.bg = color
      i += consumed
    }
  }
}

function toStyle(state: State): CSSProperties | undefined {
  const style: CSSProperties = {}
  const fg = state.inverse ? (state.bg ?? 'var(--background)') : state.fg
  const bg = state.inverse ? (state.fg ?? 'var(--foreground)') : state.bg
  if (fg) style.color = fg
  if (bg) style.backgroundColor = bg
  if (state.bold) style.fontWeight = 700
  if (state.dim) style.opacity = 0.7
  if (state.italic) style.fontStyle = 'italic'
  if (state.underline) style.textDecoration = 'underline'
  return Object.keys(style).length > 0 ? style : undefined
}

/** Splits a log line into runs of text sharing the same ANSI styling. */
export function parseAnsi(line: string): AnsiSegment[] {
  const cleaned = line.replace(OTHER_ESCAPES, '')
  const segments: AnsiSegment[] = []
  const state = blank()
  let last = 0
  const push = (end: number) => {
    if (end > last) segments.push({ text: cleaned.slice(last, end), style: toStyle(state) })
  }
  for (const match of cleaned.matchAll(CSI)) {
    push(match.index)
    last = match.index + match[0].length
    if (match[2] === 'm') applySgr(state, match[1])
  }
  push(cleaned.length)
  return segments
}

/** The plain text of a log line, for filtering and error detection. */
export function stripAnsi(line: string): string {
  return line.replace(OTHER_ESCAPES, '').replace(CSI, '')
}
