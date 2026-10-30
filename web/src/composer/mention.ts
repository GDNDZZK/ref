// Finding the `@file` mention that ends at the cursor (the TUI's Editor::mention).

export interface Mention {
  start: number // index of the `@`
  query: string // what follows it, up to the cursor
}

// `@` must start the text or follow whitespace, and the query holds no whitespace:
//   "look at @src/ma|"  -> { start: 8, query: "src/ma" }
//   "mail me@host|"     -> undefined
export function mentionAt(text: string, cursor: number): Mention | undefined {
  const before = text.slice(0, cursor)
  const start = before.lastIndexOf('@')
  if (start < 0) return undefined

  const query = before.slice(start + 1)
  if (/\s/.test(query)) return undefined

  const startsWord = start === 0 || /\s/.test(before[start - 1]!)
  if (!startsWord) return undefined
  return { start, query }
}
