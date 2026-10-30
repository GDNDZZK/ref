import { describe, expect, it } from 'vitest'
import { HISTORY_STORAGE_KEY, StoredPromptHistory, loadHistory } from './historyStorage.ts'

function memoryStorage(initial?: string) {
  const values = new Map<string, string>()
  if (initial !== undefined) values.set(HISTORY_STORAGE_KEY, initial)
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  }
}

describe('loadHistory', () => {
  it('reads a list of strings', () => {
    expect(loadHistory(memoryStorage('["a","b"]'))).toEqual(['a', 'b'])
  })

  it('ignores what is not a list of strings', () => {
    expect(loadHistory(memoryStorage())).toEqual([])
    expect(loadHistory(memoryStorage('not json'))).toEqual([])
    expect(loadHistory(memoryStorage('{"a":1}'))).toEqual([])
    expect(loadHistory(memoryStorage('["a",3,null,"b"]'))).toEqual(['a', 'b'])
  })
})

describe('StoredPromptHistory', () => {
  it('saves pushed prompts', () => {
    const storage = memoryStorage('["old"]')
    const stored = new StoredPromptHistory(storage)
    stored.push('new')
    expect(JSON.parse(storage.getItem(HISTORY_STORAGE_KEY)!)).toEqual(['old', 'new'])
  })

  it('merges what another tab saved in between', () => {
    const storage = memoryStorage()
    const first = new StoredPromptHistory(storage)
    const second = new StoredPromptHistory(storage)
    first.push('from first')
    second.push('from second')
    expect(JSON.parse(storage.getItem(HISTORY_STORAGE_KEY)!)).toEqual(['from first', 'from second'])
    first.reload()
    expect(first.history.entries).toEqual(['from first', 'from second'])
  })
})
