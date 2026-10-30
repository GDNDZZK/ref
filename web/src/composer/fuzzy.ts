// Subsequence match score for the `/` command list (the TUI's ui/text.rs fuzzy_score).

const CHAR_MATCH = 10
const CONSECUTIVE_BONUS = 15
const WORD_START_BONUS = 12
const PREFIX_BONUS = 40
const WORD_SEPARATORS = new Set(['/', '-', '_', ' ', '.'])

// Higher is better; undefined when `needle` is not a subsequence of `hay`.
// Spaces in `needle` are skipped.
export function fuzzyScore(hay: string, needle: string): number | undefined {
  if (needle === '') return 0

  const text = Array.from(hay.toLowerCase())
  const wanted = Array.from(needle.toLowerCase())
  let score = 0
  let from = 0
  let previous: number | undefined

  for (const char of wanted) {
    if (char === ' ') continue

    const index = text.indexOf(char, from)
    if (index < 0) return undefined
    from = index + 1

    score += CHAR_MATCH
    if (previous === undefined) score -= index
    else if (index === previous + 1) score += CONSECUTIVE_BONUS
    else score -= Math.trunc((index - previous) / 2)

    const startsWord = index === 0 || WORD_SEPARATORS.has(text[index - 1]!)
    if (startsWord) score += WORD_START_BONUS
    previous = index
  }

  if (text.join('').startsWith(wanted.join(''))) score += PREFIX_BONUS
  return score
}
