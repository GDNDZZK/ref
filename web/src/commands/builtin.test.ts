import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { info } from '../model/testMessages.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useMcpStore } from '../stores/mcp.ts'
import { useNoticesStore } from '../stores/notices.ts'
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

  it('registers config and reload commands with aliases and refreshes config + MCP', async () => {
    registerBuiltinCommands()
    const commands = useCommandsStore()
    const connection = useConnectionStore()
    const workspace = useWorkspaceStore()
    const dialogs = useDialogsStore()
    const notices = useNoticesStore()
    const mcp = useMcpStore()
    workspace.cwd = '/proj'

    await commands.runSlash('/configuration')
    expect(dialogs.current).toEqual({ kind: 'config' })

    const reloadSpy = vi.spyOn(connection, 'reloadConfig').mockResolvedValue({
      config: {
        agents: [],
        default_agent: 'build',
        models: [],
        commands: [],
        mcp: [],
        tui: null,
        warnings: [],
      },
      warnings: [],
    })
    const mcpSpy = vi.spyOn(mcp, 'refresh').mockResolvedValue(undefined)
    await commands.runSlash('/RELOAD-CONFIG')
    expect(reloadSpy).toHaveBeenCalledWith('/proj')
    expect(mcpSpy).toHaveBeenCalledTimes(1)
    expect(notices.notices.map((n) => [n.level, n.text])).toEqual([['info', 'Configuration reloaded']])
  })

  it('executes direct arguments and singular/case-insensitive aliases for model, agent, variant, sessions, and close-session', async () => {
    registerBuiltinCommands()
    const commands = useCommandsStore()
    const connection = useConnectionStore()
    const workspace = useWorkspaceStore()
    const dialogs = useDialogsStore()

    connection.config = {
      default_agent: 'build',
      agents: [
        { name: 'build', description: 'Builder', model: null, creatable: true, can_spawn: true, color: null },
        {
          name: 'compaction',
          description: 'Hidden',
          model: null,
          creatable: false,
          can_spawn: false,
          color: null,
        },
      ],
      models: [
        {
          provider: 'anthropic',
          provider_name: 'Anthropic',
          id: 'claude-sonnet-4-5',
          name: 'Claude Sonnet 4.5',
          context: 200000,
          output: 64000,
          images: true,
          reasoning: true,
          variants: ['low', 'high'],
        },
      ],
      commands: [],
      mcp: [],
      tui: null,
      warnings: [],
    }
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 'ses_1', name: 'Alpha', model: { provider: 'openai', model: 'gpt-5', variant: 'high' } }),
      ],
    })
    workspace.history = [info({ id: 'ses_hist', name: 'Archived', live: false })]
    workspace.historyLoaded = true
    workspace.currentId = 'ses_1'

    const setModelSpy = vi.spyOn(workspace, 'setModel').mockResolvedValue(undefined)
    const setAgentSpy = vi.spyOn(workspace, 'setAgent').mockResolvedValue(undefined)
    const setVariantSpy = vi.spyOn(workspace, 'setVariant').mockResolvedValue(undefined)
    const clearVariantSpy = vi.spyOn(workspace, 'clearVariant').mockResolvedValue(undefined)
    const openSessionSpy = vi.spyOn(workspace, 'openSession').mockResolvedValue(undefined)
    const closeSessionSpy = vi.spyOn(workspace, 'closeSession').mockResolvedValue(undefined)

    await commands.runSlash('/MODEL claude sonnet 4.5', 'ses_1')
    expect(setModelSpy).toHaveBeenCalledWith('ses_1', {
      provider: 'anthropic',
      model: 'claude-sonnet-4-5',
      variant: 'high',
    })
    await expect(commands.runSlash('/model unknown-model', 'ses_1')).rejects.toThrow('No model matching')

    await commands.runSlash('/AGENT BUILD', 'ses_1')
    expect(setAgentSpy).toHaveBeenCalledWith('ses_1', 'build')
    await expect(commands.runSlash('/agent compaction', 'ses_1')).rejects.toThrow('No agent named')

    await commands.runSlash('/variant DEFAULT', 'ses_1')
    expect(clearVariantSpy).toHaveBeenCalledWith('ses_1')
    await commands.runSlash('/variant', 'ses_1')
    expect(setVariantSpy).toHaveBeenCalledWith('ses_1', '')
    await commands.runSlash('/variant low', 'ses_1')
    expect(setVariantSpy).toHaveBeenCalledWith('ses_1', 'low')

    await commands.runSlash('/sessions archived')
    expect(openSessionSpy).toHaveBeenCalledWith('ses_hist')

    await commands.runSlash('/close-session')
    expect(dialogs.current).toEqual({ kind: 'sessions', mode: 'close' })
    await commands.runSlash('/close-session ALPHA')
    expect(closeSessionSpy).toHaveBeenCalledWith('ses_1')
  })

  it('registers /backend (/server), /stash [name], and /stashed (/groups)', async () => {
    registerBuiltinCommands()
    const commands = useCommandsStore()
    const workspace = useWorkspaceStore()
    const dialogs = useDialogsStore()
    const stashSpy = vi.spyOn(workspace, 'stashAll').mockResolvedValue(null)

    await commands.runSlash('/server')
    expect(dialogs.current).toEqual({ kind: 'backend' })

    await commands.runSlash('/groups')
    expect(dialogs.current).toEqual({ kind: 'stash' })

    await commands.runSlash('/stash sprint-1')
    expect(stashSpy).toHaveBeenCalledWith('sprint-1')
  })
})
