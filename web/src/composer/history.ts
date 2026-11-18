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
  private activeFilter: ((entry: string) => boolean) | undefined

  constructor(history: PromptHistory) {
    this.history = history
  }

  get browsing(): boolean {
    return this.pos !== undefined
  }

  private findBackward(fromExclusive: number): number | undefined {
    const items = this.history.entries
    const filter = this.activeFilter
    for (let i = Math.min(fromExclusive, items.length) - 1; i >= 0; i--) {
      if (!filter || filter(items[i]!)) return i
    }
    return undefined
  }

  private findForward(fromExclusive: number): number | undefined {
    const items = this.history.entries
    const filter = this.activeFilter
    for (let i = fromExclusive + 1; i < items.length; i++) {
      if (!filter || filter(items[i]!)) return i
    }
    return undefined
  }

  // `current` is the text in the box; it comes back from next() after the newest entry.
  prev(current: string, filter?: (entry: string) => boolean): string | undefined {
    const firstStep = this.pos === undefined
    if (firstStep) this.activeFilter = filter
    const target = this.findBackward(firstStep ? this.history.entries.length : this.pos!)
    if (target === undefined) {
      if (firstStep) this.activeFilter = undefined
      return undefined
    }
    if (firstStep) this.draft = current
    this.pos = target
    return this.history.entries[target]
  }

  next(): string | undefined {
    if (this.pos === undefined) return undefined
    const target = this.findForward(this.pos)
    if (target === undefined) {
      this.pos = undefined
      this.activeFilter = undefined
      const draft = this.draft
      this.draft = ''
      return draft
    }
    this.pos = target
    return this.history.entries[target]
  }

  // The user typed or sent: the next ↑ starts from the newest entry again.
  reset(): void {
    this.pos = undefined
    this.activeFilter = undefined
  }
}
