// A command is one thing the user can do. The command palette, the `/` list in the composer
// and the keyboard shortcuts all run the same commands.

// Matched on `KeyboardEvent.code`, not `key`: with Alt held, macOS reports a different character.
export interface Shortcut {
  code: string // e.g. "KeyN", "ArrowLeft"
  alt?: boolean
  ctrl?: boolean
  shift?: boolean
}

// The session a command acts on: the focused tile in split view, else the current session.
// Absent when no session is open.
export interface CommandContext {
  sessionId?: string
}

export interface SlashName {
  names: string[] // the first is the main name, the rest are aliases
  argsHint?: string // e.g. "[DIR]"
}

export interface Command {
  id: string // e.g. "session.new"
  titleKey: string // i18n key; the English text is the one the GUI uses
  descriptionKey?: string
  slash?: SlashName
  shortcut?: Shortcut
  palette?: boolean // listed in the command palette; default true
  enabled?(context: CommandContext): boolean // default: always
  run(context: CommandContext, args: string): void | Promise<void>
}

export interface SlashEntry {
  name: string
  aliases: string[]
  argsHint: string | undefined
  descriptionKey: string | undefined
  commandId: string
}
