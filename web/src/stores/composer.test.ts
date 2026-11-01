import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { HISTORY_STORAGE_KEY } from '../composer/historyStorage.ts'
import { createAppI18n } from '../i18n/index.ts'
import { useComposerStore } from './composer.ts'
import { useNoticesStore } from './notices.ts'

beforeEach(() => {
  createAppI18n(['en'])
  localStorage.clear()
  setActivePinia(createPinia())
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
})

describe('composer store', () => {
  it('tells the user once when the stored prompt history is not JSON', () => {
    localStorage.setItem(HISTORY_STORAGE_KEY, 'not json')
    useComposerStore()
    expect(useNoticesStore().notices.map((notice) => notice.level)).toEqual(['warn'])
  })

  it('says nothing about a history that reads fine', () => {
    localStorage.setItem(HISTORY_STORAGE_KEY, '["a"]')
    useComposerStore()
    expect(useNoticesStore().notices).toEqual([])
  })
})
