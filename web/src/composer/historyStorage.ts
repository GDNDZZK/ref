// The prompt history in localStorage, shared by the tabs of this browser.
// (The TUI keeps its own in <state_dir>/prompt-history.jsonl; the browser cannot read that.)

import { PromptHistory } from './history.ts'

export const HISTORY_STORAGE_KEY = 'ref.promptHistory'

export type HistoryStorage = Pick<Storage, 'getItem' | 'setItem'>

// Data that is not a list of strings (an older format, a hand edit) is ignored.
export function loadHistory(storage: HistoryStorage): string[] {
  const stored = storage.getItem(HISTORY_STORAGE_KEY)
  if (stored === null) return []
  let parsed: unknown
  try {
    parsed = JSON.parse(stored)
  } catch {
    return []
  }
  if (!Array.isArray(parsed)) return []
  return parsed.filter((item): item is string => typeof item === 'string')
}

export class StoredPromptHistory {
  readonly history: PromptHistory
  private readonly storage: HistoryStorage

  constructor(storage: HistoryStorage) {
    this.storage = storage
    this.history = new PromptHistory(loadHistory(storage))
  }

  // Merges what other tabs added first, so two tabs do not overwrite each other.
  push(text: string): void {
    this.reload()
    if (this.history.push(text)) {
      this.storage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(this.history.entries))
    }
  }

  // Another tab changed the stored list.
  reload(): void {
    this.history.replaceItems(loadHistory(this.storage))
  }
}
