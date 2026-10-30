// Text measured in terminal columns, as the terminal client cuts it: East Asian wide characters
// and emoji take 2, combining marks 0, and a cluster of characters never more than 2.

const MAX_CLUSTER_COLUMNS = 2
const ELLIPSIS = '…'

// Code point ranges that take two columns (a rough table; the terminal client uses unicode-width).
const WIDE_RANGES: readonly (readonly [number, number])[] = [
  [0x1100, 0x115f],
  [0x2e80, 0xa4cf],
  [0xac00, 0xd7a3],
  [0xf900, 0xfaff],
  [0xfe30, 0xfe6f],
  [0xff00, 0xff60],
  [0xffe0, 0xffe6],
  [0x1f300, 0x1f64f],
  [0x1f680, 0x1f6ff],
  [0x1f900, 0x1f9ff],
  [0x20000, 0x3fffd],
]

const ZERO_WIDTH = /^[\p{Mn}\p{Me}\p{Cf}]$/u
const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

function codePointColumns(character: string): number {
  if (ZERO_WIDTH.test(character)) return 0
  const code = character.codePointAt(0)!
  return WIDE_RANGES.some(([first, last]) => code >= first && code <= last) ? 2 : 1
}

function clusterColumns(cluster: string): number {
  let columns = 0
  for (const character of cluster) columns += codePointColumns(character)
  return Math.min(columns, MAX_CLUSTER_COLUMNS)
}

export function columnsOf(text: string): number {
  let columns = 0
  for (const { segment } of segmenter.segment(text)) columns += clusterColumns(segment)
  return columns
}

// `text` cut to at most `max` columns, ending in "…" when something was cut.
export function truncateColumns(text: string, max: number): string {
  if (columnsOf(text) <= max) return text
  if (max === 0) return ''
  let result = ''
  let used = 0
  for (const { segment } of segmenter.segment(text)) {
    const columns = clusterColumns(segment)
    if (used + columns > max - 1) break
    used += columns
    result += segment
  }
  return result + ELLIPSIS
}
