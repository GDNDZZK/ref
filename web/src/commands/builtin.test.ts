import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { registerBuiltinCommands } from './builtin.ts'
import { formatShortcut } from './shortcuts.ts'

beforeEach(() => {
  createAppI18n(['en'])
  setActivePinia(createPinia())
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
})

describe('built-in commands', () => {
  it('has the TUI commands under their slash names', () => {
    registerBuiltinCommands()
    const slashes = useCommandsStore()
      .slashEntries()
      .map((entry) => entry.name)
    for (const name of ['new', 'sessions', 'models', 'agents', 'variant', 'compact', 'undo', 'rename', 'cd'])
      expect(slashes).toContain(name)
    for (const name of ['close', 'close-session', 'export', 'copy', 'thinking', 'details', 'peers'])
      expect(slashes).toContain(name)
    for (const name of ['peers-name', 'peers-inbox', 'peers-outbox', 'watch', 'sidebar', 'mcp', 'help'])
      expect(slashes).toContain(name)
    expect(slashes).toContain('detach')
    expect(slashes).not.toContain('exit')
  })

  it('gives no two commands the same shortcut', () => {
    registerBuiltinCommands()
    const keys = useCommandsStore().commands.flatMap((command) =>
      command.shortcut ? [formatShortcut(command.shortcut)] : [],
    )
    expect(new Set(keys).size).toBe(keys.length)
  })

  it('needs a session for the commands that act on one', () => {
    registerBuiltinCommands()
    const commands = useCommandsStore()
    const undo = commands.find('undo')
    expect(undo?.enabled?.('')).toBe(false)
    expect(undo?.enabled?.('ses_1')).toBe(true)
    expect(commands.find('new')?.enabled).toBeUndefined()
  })

  it('has no shortcut for starting a session in another folder', () => {
    registerBuiltinCommands()
    expect(useCommandsStore().find('new-in')?.shortcut).toBeUndefined()
  })

  it('registers detach with background/leave aliases and enables it only when currentId or zoomedId is set', async () => {
    registerBuiltinCommands()
    const commands = useCommandsStore()
    const workspace = useWorkspaceStore()
    const detachSpy = vi.spyOn(workspace, 'detachCurrent').mockResolvedValue(undefined)
    const detach = commands.find('detach')!
    expect(detach.aliases).toEqual(['background', 'leave'])
    expect(detach.enabled?.('')).toBe(false)
    workspace.currentId = 'ses_1'
    expect(detach.enabled?.('')).toBe(true)
    await commands.runSlash('/background')
    expect(detachSpy).toHaveBeenCalledTimes(1)
    workspace.currentId = undefined
    workspace.zoomedId = 'ses_2'
    expect(detach.enabled?.('')).toBe(true)
  })
})
