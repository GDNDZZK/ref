// A command is one thing the user can do. The command palette, the `/` list in the composer,
// the help dialog and the keyboard shortcuts all run the same commands.
//
// The id is the command's slash name, as in the TUI ("new", "close-session"). `palette`,
// `settings` and `zoom` have no slash form.

// Matched on `KeyboardEvent.code`, not `key`: with Alt held, macOS reports a different character.
export interface Shortcut {
  code: string // e.g. "KeyN", "ArrowLeft"
  alt?: boolean
  ctrl?: boolean
  meta?: boolean
  shift?: boolean
}

// Text that is an i18n key (built-in commands) or already the final text (custom commands).
export type Label = { key: string } | { text: string }

export type CommandSource = 'builtin' | 'config' | 'skill' | 'mcp'

export type CommandGroup = 'session' | 'model' | 'view' | 'peers' | 'app' | 'custom'

export interface CommandArgs {
  usage: string // e.g. "NAME", "[DIR]"
  placeholder: Label // the palette's argument prompt
}

export interface Command {
  id: string
  title: Label
  group: CommandGroup
  source: CommandSource
  slash: boolean // can be typed as "/<id>"
  aliases?: string[] // more slash names
  args?: CommandArgs
  shortcut?: Shortcut
  palette?: boolean // listed in the command palette; default true
  enabled?(sessionId: string): boolean // `sessionId` is "" when no session is open; default: always
  run(sessionId: string, args: string): void | Promise<void>
}

export interface SlashEntry {
  name: string
  aliases: string[]
  usage: string | undefined
  title: Label
  source: CommandSource
}
