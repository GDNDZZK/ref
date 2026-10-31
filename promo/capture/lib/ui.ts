import type { Lang } from '../../src/engine/i18n.ts'

export type UiKind = 'web' | 'gui'

// App commands that have a keyboard shortcut in both interfaces (docs: PLAN §9.1).
// Web uses Alt+<letter>, the GUI Ctrl+Shift+<letter>.
export const COMMAND_LETTERS = {
  new: 'N',
  sessions: 'L',
  models: 'M',
  agents: 'A',
  variant: 'T',
  compact: 'C',
  undo: 'U',
  close: 'K',
  export: 'X',
  copy: 'Y',
  thinking: 'R',
  details: 'O',
  watch: 'W',
  sidebar: 'B',
  help: 'H',
  settings: 'S',
  zoom: 'Z',
} as const

export type CommandId = keyof typeof COMMAND_LETTERS

export interface ShotOptions {
  // Also copy a smaller version to docs/images/ for the READMEs.
  readme?: boolean
  // File name (without .png) of that copy, when it differs from the shot's own name.
  readmeName?: string
}

// What a scenario can do to a running interface. Both the web page and the GUI implement it,
// so one scenario produces the same pictures for both.
export interface Ui {
  readonly kind: UiKind
  readonly lang: Lang
  // Interface text in the shot's language, by key of the web locale files ("sidebar.needsPermission").
  text(key: string): string
  // Types into the prompt box and presses enter.
  send(text: string): Promise<void>
  type(text: string): Promise<void>
  clearPrompt(): Promise<void>
  // "Enter", "Escape", "Tab", "Ctrl+P", "a" ...
  key(chord: string): Promise<void>
  command(id: CommandId): Promise<void>
  clickText(text: string): Promise<void>
  hasText(text: string): Promise<boolean>
  waitText(text: string, timeoutMs?: number): Promise<void>
  waitNoText(text: string, timeoutMs?: number): Promise<void>
  pause(ms: number): Promise<void>
  // Changes the window (GUI) or viewport (web) size, in logical pixels.
  resize(width: number, height: number): Promise<void>
  // Records the window as a clip until endClip, saved as <kind>-<name>[-zh] in assets/clips (GUI only).
  startClip(name: string): Promise<void>
  endClip(): Promise<void>
  // Saves a picture as <kind>-<name>[-zh].png.
  shot(name: string, options?: ShotOptions): Promise<void>
}

export interface Scenario {
  name: string
  // Interfaces this scenario applies to; default both.
  uis?: UiKind[]
  run(ui: Ui): Promise<void>
}

export function shotFileName(kind: UiKind, name: string, lang: Lang): string {
  return `${kind}-${name}${lang === 'zh' ? '-zh' : ''}`
}
