// The input box of every session opened in this window, made on first use and kept until the
// session is forgotten, so a draft survives switching to another session and back.

import { defineStore } from 'pinia'
import { SessionComposer } from '../composer/sessionComposer.ts'
import type { ComposerHost } from '../composer/sessionComposer.ts'
import type { SlashCommand } from '../composer/completion.ts'
import { HISTORY_STORAGE_KEY, StoredPromptHistory } from '../composer/historyStorage.ts'
import { translate } from '../i18n/index.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { useCommandsStore } from './commands.ts'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'
import { useWorkspaceStore } from './workspace.ts'

// The last tool call of the newest message waits out its delay: one Esc cancels it.
export function hasDelayedTool(session: ChatSession): boolean {
  const blocks = session.blocks
  const lastMessageId = blocks.at(-1)?.messageId
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i]!
    if (block.messageId !== lastMessageId) return false
    if (block.kind === 'tool' && block.state === 'delayed') return true
  }
  return false
}

export const useComposerStore = defineStore('composer', () => {
  const connection = useConnectionStore()
  const workspace = useWorkspaceStore()
  const commands = useCommandsStore()
  const notices = useNoticesStore()

  const history = new StoredPromptHistory(localStorage)
  window.addEventListener('storage', (event) => {
    if (event.key === HISTORY_STORAGE_KEY || event.key === null) history.reload()
  })

  const composers = new Map<string, SessionComposer>()

  // The built-in commands of the registry, then the user's own from the configuration.
  function slashCommands(): SlashCommand[] {
    const builtin = commands.slashEntries().map((entry): SlashCommand => ({
      name: entry.name,
      aliases: entry.aliases,
      title: entry.descriptionKey ? translate(entry.descriptionKey) : '',
      takesArgs: entry.argsHint !== undefined,
      custom: false,
    }))
    const taken = new Set(builtin.flatMap((command) => [command.name, ...command.aliases]))
    const custom = (connection.config?.commands ?? [])
      .filter((command) => !taken.has(command.name))
      .map((command): SlashCommand => ({
        name: command.name,
        aliases: [],
        title: command.description,
        takesArgs: true,
        custom: true,
      }))
    return [...builtin, ...custom]
  }

  function hostFor(sessionId: string): ComposerHost {
    const session = () => workspace.chat(sessionId)
    return {
      sessionId,
      api: () => connection.getApi(),
      history,
      cwd: () => session().info?.cwd ?? workspace.cwd,
      isBusy: () => session().busy,
      hasDelayedTool: () => hasDelayedTool(session()),
      slashCommands,
      runSlash: (line) => commands.runSlash(line, { sessionId }).then(() => undefined),
      abort: () => session().stop(),
      toast: (level, messageKey, params) => notices.push(level, translate(messageKey, params)),
      reportError: (error) => notices.pushError(error),
    }
  }

  function of(sessionId: string): SessionComposer {
    const existing = composers.get(sessionId)
    if (existing) return existing
    const created = new SessionComposer(hostFor(sessionId))
    composers.set(sessionId, created)
    return created
  }

  // The session is closed: its draft goes with it.
  function forget(sessionId: string): void {
    composers.get(sessionId)?.dispose()
    composers.delete(sessionId)
  }

  // Tab / Shift+Tab. The tags change when the backend confirms with an Info event.
  async function cycleAgent(sessionId: string, direction: 1 | -1): Promise<void> {
    const names = (connection.config?.agents ?? []).map((agent) => agent.name)
    if (names.length === 0) return
    const current = names.indexOf(workspace.chat(sessionId).info?.agent ?? '')
    const next = names[(current + direction + names.length) % names.length]!
    try {
      await connection.getApi().setAgent(sessionId, next)
    } catch (error) {
      notices.pushError(error)
    }
  }

  // Ctrl+T: the next variant of the model.
  async function cycleVariant(sessionId: string): Promise<void> {
    try {
      await connection.getApi().setVariant(sessionId)
    } catch (error) {
      notices.pushError(error)
    }
  }

  return { of, forget, slashCommands, cycleAgent, cycleVariant }
})
