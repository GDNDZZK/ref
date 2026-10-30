// Prompt history behind ↑/↓ (the TUI's History in ui/editor.rs), split in two:
//
//   PromptHistory  the sent prompts, oldest ... newest; one per page, shared by all composers
//   HistoryBrowser one composer's walk through them: prev() steps towards older entries and
//                  keeps the draft being typed, next() steps back and, past the newest,
//                  gives the draft back

export const HISTORY_MAX = 500

export class PromptHistory {
  private items: string[]

  constructor(items: readonly string[] = []) {
    this.items = items.slice(-HISTORY_MAX)
  }

  get entries(): readonly string[] {
    return this.items
  }

  // Skips blanks and a repeat of the newest entry. Returns whether the entry was added.
  push(text: string): boolean {
    const trimmed = text.trimEnd()
    if (trimmed === '' || this.items.at(-1) === trimmed) return false
    this.items.push(trimmed)
    if (this.items.length > HISTORY_MAX) this.items.splice(0, this.items.length - HISTORY_MAX)
    return true
  }

  replaceItems(items: readonly string[]): void {
    this.items = items.slice(-HISTORY_MAX)
  }
}

export class HistoryBrowser {
  private readonly history: PromptHistory
  private pos: number | undefined // undefined while editing a fresh draft
  private draft = ''

  constructor(history: PromptHistory) {
    this.history = history
  }

  get browsing(): boolean {
    return this.pos !== undefined
  }

  // `current` is the text in the box; it comes back from next() after the newest entry.
  prev(current: string): string | undefined {
    const items = this.history.entries
    if (items.length === 0) return undefined

    let index: number
    if (this.pos === undefined) {
      this.draft = current
      index = items.length - 1
    } else if (this.pos === 0) {
      return undefined
    } else {
      index = Math.min(this.pos, items.length) - 1
    }
    this.pos = index
    return items[index]
  }

  next(): string | undefined {
    if (this.pos === undefined) return undefined

    const items = this.history.entries
    const index = this.pos + 1
    if (index >= items.length) {
      this.pos = undefined
      const draft = this.draft
      this.draft = ''
      return draft
    }
    this.pos = index
    return items[index]
  }

  // The user typed or sent: the next ↑ starts from the newest entry again.
  reset(): void {
    this.pos = undefined
  }
}
