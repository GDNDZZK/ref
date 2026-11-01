// The prompt history in localStorage, shared by the tabs of this browser.
// (The TUI keeps its own in <state_dir>/prompt-history.jsonl; the browser cannot read that.)

import { PromptHistory } from './history.ts'

export const HISTORY_STORAGE_KEY = 'ref.promptHistory'

export type HistoryStorage = Pick<Storage, 'getItem' | 'setItem'>

// Data that is not a list of strings (an older format, a hand edit) is ignored; text that is not
// JSON at all is flagged `corrupt` for the caller to report.
export function loadHistory(storage: HistoryStorage): { entries: string[]; corrupt: boolean } {
  const stored = storage.getItem(HISTORY_STORAGE_KEY)
  if (stored === null) return { entries: [], corrupt: false }
  let parsed: unknown
  try {
    parsed = JSON.parse(stored)
  } catch {
    return { entries: [], corrupt: true }
  }
  if (!Array.isArray(parsed)) return { entries: [], corrupt: false }
  const entries = parsed.filter((item): item is string => typeof item === 'string')
  return { entries, corrupt: false }
}

export class StoredPromptHistory {
  readonly history: PromptHistory
  readonly corruptAtStart: boolean // the stored text was not JSON when this was created
  private readonly storage: HistoryStorage

  constructor(storage: HistoryStorage) {
    this.storage = storage
    const loaded = loadHistory(storage)
    this.history = new PromptHistory(loaded.entries)
    this.corruptAtStart = loaded.corrupt
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
    this.history.replaceItems(loadHistory(this.storage).entries)
  }
}
