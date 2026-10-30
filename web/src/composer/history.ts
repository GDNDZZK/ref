// Prompt history behind ↑/↓ (the TUI's History in ui/editor.rs).
//
//   items:  oldest ... newest      pos = undefined while editing a fresh draft
//   prev()  steps towards older ones and keeps the draft being typed
//   next()  steps towards newer ones; past the newest it gives the draft back

export const HISTORY_MAX = 500

export class PromptHistory {
  private items: string[]
  private pos: number | undefined
  private draft = ''

  constructor(items: readonly string[] = []) {
    this.items = items.slice(-HISTORY_MAX)
  }

  get entries(): readonly string[] {
    return this.items
  }

  get browsing(): boolean {
    return this.pos !== undefined
  }

  // Skips blanks and a repeat of the newest entry.
  push(text: string): void {
    this.pos = undefined
    const trimmed = text.trimEnd()
    if (trimmed === '' || this.items.at(-1) === trimmed) return
    this.items.push(trimmed)
    if (this.items.length > HISTORY_MAX) this.items.splice(0, this.items.length - HISTORY_MAX)
  }

  // `current` is the text in the box; it comes back from next() after the newest entry.
  prev(current: string): string | undefined {
    if (this.items.length === 0) return undefined
    let index: number
    if (this.pos === undefined) {
      this.draft = current
      index = this.items.length - 1
    } else if (this.pos === 0) {
      return undefined
    } else {
      index = this.pos - 1
    }
    this.pos = index
    return this.items[index]
  }

  next(): string | undefined {
    if (this.pos === undefined) return undefined
    const index = this.pos + 1
    if (index >= this.items.length) {
      this.pos = undefined
      const draft = this.draft
      this.draft = ''
      return draft
    }
    this.pos = index
    return this.items[index]
  }

  // The user typed: the next ↑ starts from the newest entry again.
  reset(): void {
    this.pos = undefined
  }

  // Another tab pushed an entry or the stored list changed.
  replaceItems(items: readonly string[]): void {
    this.items = items.slice(-HISTORY_MAX)
    this.pos = undefined
  }
}
