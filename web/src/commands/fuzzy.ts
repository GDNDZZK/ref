// Fuzzy matching for the command palette: the query's letters must appear in order.

const CONTIGUOUS_BONUS = 3
const WORD_START_BONUS = 2

// Higher is better; undefined when `text` does not contain the query's letters in order.
export function fuzzyScore(query: string, text: string): number | undefined {
  const wanted = query.toLowerCase().replace(/\s+/g, '')
  const haystack = text.toLowerCase()
  let score = 0
  let from = 0
  let previous = -2

  for (const letter of wanted) {
    const at = haystack.indexOf(letter, from)
    if (at < 0) return undefined
    if (at === previous + 1) score += CONTIGUOUS_BONUS
    const startsWord = at === 0 || /[\s/_-]/.test(haystack[at - 1]!)
    if (startsWord) score += WORD_START_BONUS
    score += 1
    previous = at
    from = at + 1
  }
  return score - haystack.length * 0.01 // among equals the shorter text wins
}
