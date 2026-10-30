// The commands of the app, registered by whoever owns the feature.

import { defineStore } from 'pinia'
import { shallowRef } from 'vue'
import { matchesShortcut } from '../commands/shortcuts.ts'
import type { Command, CommandContext, SlashEntry } from '../commands/types.ts'
import { translate } from '../i18n/index.ts'

const SLASH_PREFIX = '/'

export class CommandUnavailableError extends Error {}

function isEnabled(command: Command, context: CommandContext): boolean {
  return command.enabled?.(context) ?? true
}

export const useCommandsStore = defineStore('commands', () => {
  const commands = shallowRef<readonly Command[]>([])

  // Returns a function that removes the command again.
  function register(command: Command): () => void {
    if (commands.value.some((existing) => existing.id === command.id))
      throw new Error(`command "${command.id}" is already registered`)
    commands.value = [...commands.value, command]
    return () => (commands.value = commands.value.filter((existing) => existing !== command))
  }

  function find(id: string): Command | undefined {
    return commands.value.find((command) => command.id === id)
  }

  async function execute(command: Command, context: CommandContext, args: string): Promise<void> {
    if (!isEnabled(command, context)) throw new CommandUnavailableError(translate('command.unavailable'))
    await command.run(context, args)
  }

  async function run(id: string, context: CommandContext = {}, args = ''): Promise<void> {
    const command = find(id)
    if (!command) throw new Error(`unknown command "${id}"`)
    await execute(command, context, args)
  }

  // "/rename Work" runs session.rename with "Work". Returns false when no command has that name,
  // so the caller can treat the text as a custom command or a plain prompt.
  async function runSlash(text: string, context: CommandContext = {}): Promise<boolean> {
    if (!text.startsWith(SLASH_PREFIX)) return false
    const [head = '', ...rest] = text.slice(SLASH_PREFIX.length).split(/\s+/)
    const name = head.toLowerCase()
    const command = commands.value.find((candidate) => candidate.slash?.names.includes(name))
    if (!command) return false
    await execute(command, context, rest.join(' ').trim())
    return true
  }

  function slashEntries(): SlashEntry[] {
    return commands.value.flatMap((command): SlashEntry[] => {
      if (!command.slash) return []
      const [name = '', ...aliases] = command.slash.names
      return [
        {
          name,
          aliases,
          argsHint: command.slash.argsHint,
          descriptionKey: command.descriptionKey,
          commandId: command.id,
        },
      ]
    })
  }

  // The commands the palette lists now.
  function available(context: CommandContext): Command[] {
    return commands.value.filter((command) => command.palette !== false && isEnabled(command, context))
  }

  // The enabled command bound to the key press, if any.
  function matchKey(event: KeyboardEvent, context: CommandContext): Command | undefined {
    return commands.value.find(
      (command) =>
        command.shortcut && matchesShortcut(command.shortcut, event) && isEnabled(command, context),
    )
  }

  return { commands, register, find, run, runSlash, slashEntries, available, matchKey }
})
