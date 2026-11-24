// The commands of the app: sessions, view, help, and the conversation commands (undo, copy,
// thinking, details), which act on the session's ChatSession.

import { translate } from '../i18n/index.ts'
import { isWorkingSession } from '../shell/sessionRow.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useMcpStore } from '../stores/mcp.ts'
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

// Finds live sessions by name or id (case-insensitively); `/close-session a b` closes both.
function liveSessionsNamed(names: string[]) {
  const workspace = useWorkspaceStore()
  return names.map((name) => {
    const wanted = name.toLowerCase()
    const found = workspace.live.find(
      (session) => session.name.toLowerCase() === wanted || session.id.toLowerCase() === wanted,
    )
    if (!found) throw new Error(translate('command.noSuchSession', { name }))
    return found
  })
}

async function findAnySession(workspace: ReturnType<typeof useWorkspaceStore>, raw: string) {
  const lower = raw.toLowerCase()
  const match = (list: readonly { id: string; name: string; title: string }[]) =>
    list.find((s) => s.id.toLowerCase() === lower || (s.name !== '' && s.name.toLowerCase() === lower)) ??
    list.find((s) => s.title !== '' && s.title.toLowerCase() === lower)

  const inLive = match(workspace.live)
  if (inLive) return inLive
  const inHistory = match(workspace.history)
  if (inHistory) return inHistory
  if (!workspace.historyLoaded) {
    try {
      await workspace.refreshHistory()
    } catch {
      // Disconnected or unmocked in isolated tests
    }
    const afterRefresh = match(workspace.history)
    if (afterRefresh) return afterRefresh
  }
  throw new Error(translate('command.noSuchAnySession', { name: raw }))
}

export function registerBuiltinCommands(): void {
  const commands = useCommandsStore()
  const connection = useConnectionStore()
  const workspace = useWorkspaceStore()
  const dialogs = useDialogsStore()
  const notices = useNoticesStore()
  const layout = useLayoutStore()
  const mcp = useMcpStore()

  const canZoomTile = () => !layout.narrow && workspace.mode === 'split' && workspace.focusedId !== undefined

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
      run: () => dialogs.open({ kind: 'newSession' }),
    }),
    define({
      id: 'sessions',
      aliases: ['resume', 'continue'],
      args: argument('[session]'),
      shortcut: altKey('L'),
      run: async (_sessionId, args) => {
        const wanted = args.trim()
        if (!wanted) return dialogs.open({ kind: 'sessions' })
        const found = await findAnySession(workspace, wanted)
        await workspace.openSession(found.id)
      },
    }),
    define({
      id: 'models',
      aliases: ['model'],
      args: argument('[model]'),
      shortcut: altKey('M'),
      enabled: hasSession,
      run: async (sessionId, args) => {
        const wanted = args.trim()
        if (!wanted) return dialogs.open({ kind: 'models', sessionId })
        const lower = wanted.toLowerCase()
        const models = connection.config?.models ?? []
        const found =
          models.find((m) => `${m.provider}/${m.id}`.toLowerCase() === lower) ??
          models.find((m) => m.id.toLowerCase() === lower) ??
          models.find((m) => m.name.toLowerCase() === lower)
        if (!found) throw new Error(translate('command.noSuchModel', { name: wanted }))
        const variant = workspace.infoOf(sessionId)?.model.variant
        const kept = variant && found.variants.includes(variant) ? variant : null
        await workspace.setModel(sessionId, { provider: found.provider, model: found.id, variant: kept })
      },
    }),
    define({
      id: 'agents',
      aliases: ['agent'],
      args: argument('[agent]'),
      shortcut: altKey('A'),
      enabled: hasSession,
      run: async (sessionId, args) => {
        const wanted = args.trim()
        if (!wanted) return dialogs.open({ kind: 'agents', sessionId })
        const lower = wanted.toLowerCase()
        const found = (connection.config?.agents ?? []).find(
          (agent) => agent.creatable && agent.name.toLowerCase() === lower,
        )
        if (!found) throw new Error(translate('command.noSuchAgent', { name: wanted }))
        await workspace.setAgent(sessionId, found.name)
      },
    }),
    define({
      id: 'variant',
      aliases: ['variants', 'effort'],
      args: argument('[name]'),
      shortcut: altKey('T'),
      enabled: hasSession,
      runPalette: (sessionId) => dialogs.open({ kind: 'variants', sessionId }),
      run: (sessionId, args) => {
        const wanted = args.trim()
        if (!wanted) return workspace.setVariant(sessionId, '')
        if (wanted.toLowerCase() === 'default') return workspace.clearVariant(sessionId)
        const currentModel = workspace.infoOf(sessionId)?.model
        const modelInfo = connection.config?.models.find(
          (m) => m.provider === currentModel?.provider && m.id === currentModel?.model,
        )
        const matched = modelInfo?.variants.find((v) => v.toLowerCase() === wanted.toLowerCase()) ?? wanted
        return workspace.setVariant(sessionId, matched)
      },
    }),
    define({
      id: 'compact',
      aliases: ['summarize'],
      shortcut: altKey('C'),
      enabled: hasSession,
      run: (sessionId) => workspace.compact(sessionId),
    }),
    define({
      id: 'undo',
      shortcut: altKey('U'),
      enabled: hasSession,
      run: (sessionId) => workspace.chat(sessionId).undoLast(),
    }),
    define({
      id: 'copy',
      shortcut: altKey('Y'),
      enabled: hasSession,
      run: async (sessionId) => {
        const copied = await workspace.chat(sessionId).copyLastReply()
        if (copied) notices.push('info', translate('code.copied'))
        else notices.push('warn', translate('command.copy.nothing'))
      },
    }),
    define({
      id: 'thinking',
      group: 'view',
      shortcut: altKey('R'),
      enabled: hasSession,
      run: (sessionId) => workspace.chat(sessionId).toggleReasoning(),
    }),
    define({
      id: 'details',
      group: 'view',
      shortcut: altKey('O'),
      enabled: hasSession,
      run: (sessionId) => workspace.chat(sessionId).toggleDetails(),
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
      id: 'detach',
      aliases: ['background', 'leave'],
      enabled: () => workspace.currentId !== undefined || workspace.zoomedId !== undefined,
      run: () => workspace.detachCurrent(),
    }),
    define({
      id: 'close',
      shortcut: altKey('K'),
      enabled: hasSession,
      run: (sessionId) =>
        isWorkingSession(workspace.infoOf(sessionId))
          ? dialogs.open({ kind: 'closeSession', sessionId })
          : workspace.closeSession(sessionId),
    }),
    define({
      id: 'close-session',
      aliases: ['kill'],
      args: argument('[name or id]'),
      run: async (_sessionId, args) => {
        const trimmed = args.trim()
        if (!trimmed) return dialogs.open({ kind: 'sessions', mode: 'close' })
        const seen = new Set<string>()
        for (const session of liveSessionsNamed(trimmed.split(/\s+/))) {
          if (seen.has(session.id)) continue
          seen.add(session.id)
          await workspace.closeSession(session.id)
        }
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
      enabled: canZoomTile,
      run: (sessionId) => (workspace.zoomedId ? workspace.unzoom() : workspace.zoomTile(sessionId)),
    }),
    define({
      id: 'mcp',
      aliases: ['mcps'],
      group: 'help',
      run: () => dialogs.open({ kind: 'mcp' }),
    }),
    define({
      id: 'config',
      aliases: ['configuration'],
      group: 'help',
      run: () => dialogs.open({ kind: 'config' }),
    }),
    define({
      id: 'reload',
      aliases: ['reload-config'],
      group: 'help',
      run: async () => {
        await connection.reloadConfig(workspace.effectiveCwd)
        await mcp.refresh()
        notices.push('info', translate('command.reload.done'))
      },
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
      id: 'backend',
      aliases: ['server'],
      group: 'help',
      run: () => dialogs.open({ kind: 'backend' }),
    }),
    define({
      id: 'stash',
      args: argument('[name]'),
      run: async (_sessionId, args) => {
        const name = args.trim()
        if (name) {
          await workspace.stashAll(name)
          return
        }
        if (workspace.live.length > 0) {
          await workspace.stashAll()
          return
        }
        dialogs.open({ kind: 'stash' })
      },
    }),
    define({
      id: 'stashed',
      aliases: ['groups'],
      run: () => dialogs.open({ kind: 'stash' }),
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
