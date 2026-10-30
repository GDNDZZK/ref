// Matching and showing keyboard shortcuts.

import type { Shortcut } from './types.ts'

const KEY_PREFIX = 'Key'
const DIGIT_PREFIX = 'Digit'
const ARROWS: Record<string, string> = { ArrowLeft: '←', ArrowRight: '→', ArrowUp: '↑', ArrowDown: '↓' }

export function matchesShortcut(shortcut: Shortcut, event: KeyboardEvent): boolean {
  return (
    event.code === shortcut.code &&
    event.altKey === (shortcut.alt ?? false) &&
    event.ctrlKey === (shortcut.ctrl ?? false) &&
    event.shiftKey === (shortcut.shift ?? false) &&
    event.metaKey === (shortcut.meta ?? false)
  )
}

function keyName(code: string): string {
  if (code.startsWith(KEY_PREFIX)) return code.slice(KEY_PREFIX.length)
  if (code.startsWith(DIGIT_PREFIX)) return code.slice(DIGIT_PREFIX.length)
  return ARROWS[code] ?? code
}

// "Alt+N", "Ctrl+P".
export function formatShortcut(shortcut: Shortcut): string {
  const modifiers = [
    shortcut.ctrl && 'Ctrl',
    shortcut.meta && 'Cmd',
    shortcut.alt && 'Alt',
    shortcut.shift && 'Shift',
  ]
  return [...modifiers.filter((name): name is string => Boolean(name)), keyName(shortcut.code)].join('+')
}
