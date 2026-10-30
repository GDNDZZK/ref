// The commands this app itself provides (area B). The conversation commands (undo, copy,
// thinking, details) are registered by the conversation view.

import { translate } from '../i18n/index.ts'
import { rowState } from '../shell/sessionRow.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { IS_MAC } from './platform.ts'
import type { Command } from './types.ts'

const hasSession = (sessionId: string) => sessionId !== ''

// Defaults every built-in command shares.
type Definition = Omit<Command, 'source' | 'slash' | 'title' | 'group'> &
  Partial<Pick<Command, 'slash' | 'group'>>

function define(definition: Definition): Command {
  return {
    slash: true,
    group: 'session',
    ...definition,
    title: { key: `command.${definition.id}.title` },
    source: 'builtin',
  }
}

function altKey(letter: string, shift = false) {
  return { code: `Key${letter}`, alt: true, shift }
}

function argument(usage: string) {
  return { usage }
}

// Finds live sessions by name or id; `/close-session a b` closes both.
function liveSessionsNamed(names: string[]) {
  const workspace = useWorkspaceStore()
  return names.map((name) => {
    const found = workspace.live.find((session) => session.name === name || session.id === name)
    if (!found) throw new Error(translate('command.noSuchSession', { name }))
    return found
  })
}

export function registerBuiltinCommands(): void {
  const commands = useCommandsStore()
  const workspace = useWorkspaceStore()
  const dialogs = useDialogsStore()
  const notices = useNoticesStore()
  const layout = useLayoutStore()

  const isWorking = (sessionId: string) => {
    const info = workspace.live.find((session) => session.id === sessionId)
    return info !== undefined && rowState(info) !== 'idle' && rowState(info) !== 'error'
  }

  const builtin: Command[] = [
    define({
      id: 'new',
      aliases: ['clear'],
      shortcut: altKey('N'),
      run: async () => void (await workspace.newSession()),
    }),
    define({
      id: 'new-in',
      slash: false,
      shortcut: altKey('N', true),
      run: () => dialogs.open({ kind: 'newSession' }),
    }),
    define({
      id: 'sessions',
      aliases: ['resume', 'continue'],
      shortcut: altKey('L'),
      run: () => dialogs.open({ kind: 'sessions' }),
    }),
    define({
      id: 'models',
      shortcut: altKey('M'),
      enabled: hasSession,
      run: (sessionId) => dialogs.open({ kind: 'models', sessionId }),
    }),
    define({
      id: 'agents',
      shortcut: altKey('A'),
      enabled: hasSession,
      run: (sessionId) => dialogs.open({ kind: 'agents', sessionId }),
    }),
    define({
      id: 'variant',
      aliases: ['variants', 'effort'],
      args: argument('[name]'),
      shortcut: altKey('T'),
      enabled: hasSession,
      run: (sessionId, args) => workspace.setVariant(sessionId, args),
    }),
    define({
      id: 'compact',
      aliases: ['summarize'],
      shortcut: altKey('C'),
      enabled: hasSession,
      run: (sessionId) => workspace.compact(sessionId),
    }),
    define({
      id: 'rename',
      args: argument('NAME'),
      enabled: hasSession,
      run: (sessionId, args) =>
        args ? workspace.renameSession(sessionId, args) : dialogs.open({ kind: 'rename', sessionId }),
    }),
    define({
      id: 'cd',
      aliases: ['cwd'],
      args: argument('[DIR]'),
      enabled: hasSession,
      run: async (sessionId, args) => {
        if (!args) return dialogs.open({ kind: 'cd', sessionId })
        await workspace.setSessionCwd(sessionId, args)
      },
    }),
    define({
      id: 'close',
      shortcut: altKey('K'),
      enabled: hasSession,
      run: (sessionId) =>
        isWorking(sessionId)
          ? dialogs.open({ kind: 'closeSession', sessionId })
          : workspace.closeSession(sessionId),
    }),
    define({
      id: 'close-session',
      aliases: ['kill'],
      args: argument('[name or id]'),
      run: async (_sessionId, args) => {
        if (!args) return dialogs.open({ kind: 'sessions' })
        for (const session of liveSessionsNamed(args.split(/\s+/))) await workspace.closeSession(session.id)
      },
    }),
    define({
      id: 'export',
      args: argument('[PATH]'),
      shortcut: altKey('X'),
      enabled: hasSession,
      run: async (sessionId, args) => {
        if (!args) return dialogs.open({ kind: 'export', sessionId })
        const path = await workspace.exportSession(sessionId, args)
        notices.push('info', translate('dialog.export.done', { path }))
      },
    }),
    define({
      id: 'peers',
      aliases: ['list-agents'],
      enabled: hasSession,
      run: (sessionId) => dialogs.open({ kind: 'peers', sessionId, tab: 'peers' }),
    }),
    define({
      id: 'peers-name',
      args: argument('[name]'),
      enabled: hasSession,
      run: (sessionId, args) =>
        dialogs.open({ kind: 'peers', sessionId, tab: 'name', ...(args ? { name: args } : {}) }),
    }),
    define({
      id: 'peers-inbox',
      enabled: hasSession,
      run: (sessionId) => dialogs.open({ kind: 'peers', sessionId, tab: 'inbox' }),
    }),
    define({
      id: 'peers-outbox',
      enabled: hasSession,
      run: (sessionId) => dialogs.open({ kind: 'peers', sessionId, tab: 'outbox' }),
    }),
    define({
      id: 'watch',
      aliases: ['split'],
      group: 'view',
      shortcut: altKey('W'),
      enabled: () => !layout.narrow,
      run: () => workspace.toggleSplit(),
    }),
    define({
      id: 'sidebar',
      group: 'view',
      shortcut: altKey('B'),
      run: () => layout.toggleSidebar(),
    }),
    define({
      id: 'zoom',
      slash: false,
      group: 'view',
      shortcut: altKey('Z'),
      enabled: () => !layout.narrow && workspace.mode === 'split' && workspace.focusedId !== undefined,
      run: (sessionId) => (workspace.zoomedId ? workspace.unzoom() : workspace.zoomTile(sessionId)),
    }),
    define({
      id: 'mcp',
      aliases: ['mcps'],
      group: 'help',
      run: () => dialogs.open({ kind: 'mcp' }),
    }),
    define({
      id: 'help',
      group: 'help',
      shortcut: altKey('H'),
      run: () => dialogs.open({ kind: 'help' }),
    }),
    define({
      id: 'settings',
      slash: false,
      group: 'help',
      shortcut: altKey('S'),
      run: () => dialogs.open({ kind: 'settings' }),
    }),
    define({
      id: 'palette',
      slash: false,
      group: 'help',
      palette: false,
      shortcut: IS_MAC ? { code: 'KeyP', meta: true } : { code: 'KeyP', ctrl: true },
      run: () => dialogs.open({ kind: 'palette' }),
    }),
  ]
  for (const command of builtin) commands.register(command)
}
