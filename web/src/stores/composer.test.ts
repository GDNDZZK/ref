import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { registerBuiltinCommands } from '../commands/builtin.ts'
import { installCustomCommands } from '../commands/custom.ts'
import { HISTORY_STORAGE_KEY } from '../composer/historyStorage.ts'
import { createAppI18n } from '../i18n/index.ts'
import { useComposerStore } from './composer.ts'
import { useConnectionStore } from './connection.ts'
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

  it('marks only commands with required usage as requiresArgs', () => {
    registerBuiltinCommands()
    const connection = useConnectionStore()
    connection.config = {
      agents: [],
      models: [],
      commands: [{ name: 'review', description: 'Review diff', source: 'config' }],
    } as never
    installCustomCommands()
    const slash = useComposerStore().slashCommands()
    const byName = new Map(slash.map((entry) => [entry.name, entry]))

    expect(byName.get('rename')?.requiresArgs).toBe(true)
    for (const standalone of [
      'sessions',
      'models',
      'agents',
      'variant',
      'cd',
      'export',
      'close-session',
      'peers-name',
      'undo',
      'review',
    ]) {
      expect(byName.get(standalone)?.requiresArgs, standalone).toBe(false)
    }
  })
})
