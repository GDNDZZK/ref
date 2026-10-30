// Reads a tool call's `purpose` out of arguments that are still streaming in.

const PURPOSE_KEY = '"purpose"'
const WHITESPACE_ESCAPES = new Set(['n', 't'])

// `{"purpose": "Fix the off-by` -> "Fix the off-by"; undefined until the value starts.
// Same rule as the TUI's partial_purpose.
export function partialPurpose(rawInput: string): string | undefined {
  const keyAt = rawInput.indexOf(PURPOSE_KEY)
  if (keyAt < 0) return undefined

  const afterKey = rawInput.slice(keyAt + PURPOSE_KEY.length).trimStart()
  if (!afterKey.startsWith(':')) return undefined
  const afterColon = afterKey.slice(1).trimStart()
  if (!afterColon.startsWith('"')) return undefined

  let purpose = ''
  let escaped = false
  for (const char of afterColon.slice(1)) {
    if (escaped) {
      purpose += WHITESPACE_ESCAPES.has(char) ? ' ' : char
      escaped = false
    } else if (char === '\\') {
      escaped = true
    } else if (char === '"') {
      return purpose
    } else {
      purpose += char
    }
  }
  return purpose === '' ? undefined : purpose
}
