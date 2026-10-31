import { element, type Part } from '../engine/part.ts'

export interface TypedTextOptions {
  text: string
  start: number
  charsPerSecond: number
  className?: string
  // Caret blink period; the caret is always on while typing.
  blinkSeconds?: number
}

const DEFAULT_BLINK_SECONDS = 1

// How much of `text` has been typed `elapsed` seconds after typing began.
export function typedPrefix(text: string, elapsed: number, charsPerSecond: number): string {
  const count = Math.max(0, Math.floor(elapsed * charsPerSecond))
  return Array.from(text).slice(0, count).join('')
}

// Text that types itself. Position and size come from `className` (CSS).
export function typedText(options: TypedTextOptions): Part {
  const el = element('div', `part typed-text ${options.className ?? ''}`)
  const blink = options.blinkSeconds ?? DEFAULT_BLINK_SECONDS
  const typingSeconds = Array.from(options.text).length / options.charsPerSecond

  return {
    el,
    render(time) {
      const elapsed = time - options.start
      const typing = elapsed >= 0 && elapsed < typingSeconds
      const caretOn = typing || Math.floor((time / blink) * 2) % 2 === 0
      el.textContent = typedPrefix(options.text, elapsed, options.charsPerSecond)
      el.dataset.caret = caretOn ? 'on' : 'off'
    },
  }
}
