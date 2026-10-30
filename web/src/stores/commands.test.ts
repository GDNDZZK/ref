import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fuzzyScore } from '../commands/fuzzy.ts'
import { formatShortcut } from '../commands/shortcuts.ts'
import type { Command } from '../commands/types.ts'
import { createAppI18n } from '../i18n/index.ts'
import { CommandUnavailableError, useCommandsStore } from './commands.ts'

function command(id: string, overrides: Partial<Command> = {}): Command {
  return {
    id,
    title: { key: `command.${id}` },
    group: 'session',
    source: 'builtin',
    slash: false,
    run: vi.fn(),
    ...overrides,
  }
}

function key(code: string, modifiers: Partial<KeyboardEventInit> = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { code, ...modifiers })
}

beforeEach(() => {
  createAppI18n(['en'])
  setActivePinia(createPinia())
})

describe('commands store', () => {
  it('runs a command with its context and arguments', async () => {
    const store = useCommandsStore()
    const rename = command('rename')
    store.register(rename)
    await store.run('rename', 'Work', 'ses_1')
    expect(rename.run).toHaveBeenCalledWith('ses_1', 'Work')
  })

  it('refuses two commands with one id', () => {
    const store = useCommandsStore()
    store.register(command('a'))
    expect(() => store.register(command('a'))).toThrow(/already registered/)
  })

  it('keeps built-in commands when the custom ones are replaced', () => {
    const store = useCommandsStore()
    store.register(command('new'))
    store.replaceCustom([command('review', { source: 'config' })])
    store.replaceCustom([command('deploy', { source: 'skill' })])
    expect(store.commands.map((entry) => entry.id)).toEqual(['new', 'deploy'])
  })

  it('stops listing a command that was unregistered', () => {
    const store = useCommandsStore()
    const unregister = store.register(command('a'))
    unregister()
    expect(store.find('a')).toBeUndefined()
  })

  it('refuses to run a disabled command', async () => {
    const store = useCommandsStore()
    const close = command('close', { enabled: (sessionId) => sessionId !== '' })
    store.register(close)
    await expect(store.run('close')).rejects.toBeInstanceOf(CommandUnavailableError)
    expect(close.run).not.toHaveBeenCalled()
  })
})

describe('slash commands', () => {
  it('runs by main name or alias, case-insensitively, with the rest as arguments', async () => {
    const store = useCommandsStore()
    const cd = command('cd', {
      slash: true,
      aliases: ['cwd'],
      args: { usage: '[DIR]' },
    })
    store.register(cd)
    expect(await store.runSlash('/CWD  ~/src  x ', 's')).toBe(true)
    expect(cd.run).toHaveBeenCalledWith('s', '~/src x')
  })

  it('leaves unknown names and plain text to the caller', async () => {
    const store = useCommandsStore()
    store.register(command('new', { slash: true }))
    expect(await store.runSlash('/mine run')).toBe(false)
    expect(await store.runSlash('new')).toBe(false)
  })

  it('lists main names with their aliases', () => {
    const store = useCommandsStore()
    store.register(command('sessions', { slash: true, aliases: ['resume'] }))
    store.register(command('palette'))
    expect(store.slashEntries()).toMatchObject([{ name: 'sessions', aliases: ['resume'] }])
  })
})

describe('shortcuts', () => {
  it('finds an enabled command by its key, with exactly the modifiers it names', () => {
    const store = useCommandsStore()
    const sidebar = command('sidebar', { shortcut: { code: 'KeyB', alt: true } })
    store.register(sidebar)
    expect(store.matchKey(key('KeyB', { altKey: true }))).toBe(sidebar)
    expect(store.matchKey(key('KeyB'))).toBeUndefined()
    expect(store.matchKey(key('KeyB', { altKey: true, shiftKey: true }))).toBeUndefined()
    expect(store.matchKey(key('KeyB', { altKey: true, metaKey: true }))).toBeUndefined()
  })

  it('ignores a disabled command, so the browser keeps the key', () => {
    const store = useCommandsStore()
    store.register(command('a', { shortcut: { code: 'KeyA', alt: true }, enabled: () => false }))
    expect(store.matchKey(key('KeyA', { altKey: true }))).toBeUndefined()
  })

  it('shows the key as text', () => {
    expect(formatShortcut({ code: 'KeyN', alt: true })).toBe('Alt+N')
    expect(formatShortcut({ code: 'KeyP', ctrl: true })).toBe('Ctrl+P')
    expect(formatShortcut({ code: 'ArrowLeft', alt: true, shift: true })).toBe('Alt+Shift+←')
    expect(formatShortcut({ code: 'KeyP', meta: true })).toBe('Cmd+P')
  })
})

describe('palette', () => {
  it('lists enabled commands that are not hidden', () => {
    const store = useCommandsStore()
    store.register(command('a'))
    store.register(command('b', { palette: false }))
    store.register(command('c', { enabled: () => false }))
    expect(store.available().map((entry) => entry.id)).toEqual(['a'])
  })
})

describe('fuzzyScore', () => {
  it('needs the letters in order', () => {
    expect(fuzzyScore('nws', 'New session')).toBeDefined()
    expect(fuzzyScore('swn', 'New session')).toBeUndefined()
  })

  it('prefers contiguous matches and word starts', () => {
    expect(fuzzyScore('new', 'New session')!).toBeGreaterThan(fuzzyScore('new', 'Rename the window')!)
    expect(fuzzyScore('', 'anything')).toBeDefined()
  })
})
