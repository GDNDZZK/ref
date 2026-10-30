// The commands of the app, registered by whoever owns the feature.

import { defineStore } from 'pinia'
import { shallowRef } from 'vue'
import { matchesShortcut } from '../commands/shortcuts.ts'
import type { Command, SlashEntry } from '../commands/types.ts'
import { translate } from '../i18n/index.ts'
import { useWorkspaceStore } from './workspace.ts'

const SLASH_PREFIX = '/'

export class CommandUnavailableError extends Error {}

function isEnabled(command: Command, sessionId: string): boolean {
  return command.enabled?.(sessionId) ?? true
}

function slashNames(command: Command): string[] {
  return command.slash ? [command.id, ...(command.aliases ?? [])] : []
}

export const useCommandsStore = defineStore('commands', () => {
  const workspace = useWorkspaceStore()
  const commands = shallowRef<readonly Command[]>([])

  // Commands act on `sessionId`, or on the focused tile / open session when it is empty.
  function target(sessionId: string): string {
    return sessionId || workspace.focusedId || ''
  }

  // Returns a function that removes the command again.
  function register(command: Command): () => void {
    if (commands.value.some((existing) => existing.id === command.id))
      throw new Error(`command "${command.id}" is already registered`)
    commands.value = [...commands.value, command]
    return () => (commands.value = commands.value.filter((existing) => existing !== command))
  }

  // Replaces every command that did not come from the app itself (custom commands from the config).
  function replaceCustom(custom: readonly Command[]): void {
    commands.value = [...commands.value.filter((command) => command.source === 'builtin'), ...custom]
  }

  function find(id: string): Command | undefined {
    return commands.value.find((command) => command.id === id)
  }

  async function execute(command: Command, sessionId: string, args: string): Promise<void> {
    const sessionTarget = target(sessionId)
    if (!isEnabled(command, sessionTarget))
      throw new CommandUnavailableError(translate('command.unavailable'))
    await command.run(sessionTarget, args)
  }

  async function run(id: string, args = '', sessionId = ''): Promise<void> {
    const command = find(id)
    if (!command) throw new Error(`unknown command "${id}"`)
    await execute(command, sessionId, args)
  }

  // "/rename Work" runs `rename` with "Work". Returns false when no command has that name, so the
  // caller can send the text as a prompt.
  async function runSlash(line: string, sessionId = ''): Promise<boolean> {
    if (!line.startsWith(SLASH_PREFIX)) return false
    const [head = '', ...rest] = line.slice(SLASH_PREFIX.length).split(/\s+/)
    const name = head.toLowerCase()
    const command = commands.value.find((candidate) => slashNames(candidate).includes(name))
    if (!command) return false
    await execute(command, sessionId, rest.join(' ').trim())
    return true
  }

  function slashEntries(): SlashEntry[] {
    return commands.value
      .filter((command) => command.slash)
      .map((command) => ({
        name: command.id,
        aliases: command.aliases ?? [],
        usage: command.args?.usage,
        title: command.title,
        source: command.source,
      }))
  }

  // The commands the palette lists now.
  function available(sessionId = ''): Command[] {
    const sessionTarget = target(sessionId)
    return commands.value.filter((command) => command.palette !== false && isEnabled(command, sessionTarget))
  }

  // Every command the palette lists, with whether it can run now (disabled ones are shown dimmed).
  function listed(sessionId = ''): { command: Command; enabled: boolean }[] {
    const sessionTarget = target(sessionId)
    return commands.value
      .filter((command) => command.palette !== false)
      .map((command) => ({ command, enabled: isEnabled(command, sessionTarget) }))
  }

  // The enabled command bound to the key press, if any.
  function matchKey(event: KeyboardEvent): Command | undefined {
    const sessionTarget = target('')
    return commands.value.find(
      (command) =>
        command.shortcut && matchesShortcut(command.shortcut, event) && isEnabled(command, sessionTarget),
    )
  }

  return { commands, register, replaceCustom, find, run, runSlash, slashEntries, available, listed, matchKey }
})
