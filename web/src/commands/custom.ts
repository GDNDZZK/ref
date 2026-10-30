// The user's own commands (the configuration's `command` entries) in the registry, so the palette
// lists them and `/name` runs them. The server expands the template; the composer sends the result.

import { watch } from 'vue'
import { useComposerStore } from '../stores/composer.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import type { Command, CommandSource } from './types.ts'

const SOURCES: readonly CommandSource[] = ['config', 'skill', 'mcp']

export function installCustomCommands(): void {
  const connection = useConnectionStore()
  const commands = useCommandsStore()
  const composer = useComposerStore()

  watch(
    () => connection.config?.commands,
    (infos) => {
      // A built-in command with the same name wins.
      const taken = new Set(commands.commands.filter((c) => c.source === 'builtin').flatMap(slashNamesOf))
      const custom = (infos ?? [])
        .filter((info) => !taken.has(info.name))
        .map((info): Command => ({
          id: info.name,
          title: { text: info.description || info.name },
          group: 'custom',
          source: SOURCES.find((source) => source === info.source) ?? 'config',
          slash: true,
          args: { usage: '[args]' },
          enabled: (sessionId) => sessionId !== '',
          run: (sessionId, args) => composer.of(sessionId).runCustomCommand(info.name, args),
        }))
      commands.replaceCustom(custom)
    },
    { immediate: true },
  )
}

function slashNamesOf(command: Command): string[] {
  return command.slash ? [command.id, ...(command.aliases ?? [])] : []
}
