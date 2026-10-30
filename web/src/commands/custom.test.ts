import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import type { ServerConfigInfo } from '../proto/generated'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { installCustomCommands } from './custom.ts'
import type { Command } from './types.ts'

const config = (commands: ServerConfigInfo['commands']) => ({ commands }) as ServerConfigInfo

beforeEach(() => {
  createAppI18n(['en'])
  setActivePinia(createPinia())
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
})

describe('installCustomCommands', () => {
  it('registers the configured commands next to the built-in ones', async () => {
    const commands = useCommandsStore()
    const builtin: Command = {
      id: 'new',
      title: { key: 'command.new.title' },
      group: 'session',
      source: 'builtin',
      slash: true,
      run: vi.fn(),
    }
    commands.register(builtin)
    const connection = useConnectionStore()
    connection.config = config([
      { name: 'review', description: 'Review the diff', source: 'config' },
      { name: 'new', description: 'clashes with a built-in', source: 'config' },
    ])
    installCustomCommands()
    await nextTick()

    expect(commands.commands.map((command) => command.id)).toEqual(['new', 'review'])
    const review = commands.find('review')
    expect(review?.group).toBe('custom')
    expect(review?.title).toEqual({ text: 'Review the diff' })
    expect(commands.slashEntries().map((entry) => entry.name)).toEqual(['new', 'review'])
  })

  it('follows a changed configuration', async () => {
    const commands = useCommandsStore()
    const connection = useConnectionStore()
    installCustomCommands()
    connection.config = config([{ name: 'a', description: '', source: 'skill' }])
    await nextTick()
    expect(commands.find('a')?.source).toBe('skill')
    connection.config = config([])
    await nextTick()
    expect(commands.find('a')).toBeUndefined()
  })
})
