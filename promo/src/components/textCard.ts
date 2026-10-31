import { lerp, span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'
import { typedPrefix } from './typedText.ts'

export type CardTone = 'plain' | 'good' | 'bad'

export interface TextCardOptions {
  // Position and width on the stage, in px.
  x: number
  y: number
  width: number
  title?: string | Localized
  // Lines of monospace text. A line with a `type` time is typed from that time on.
  lines: { text: string | Localized; tone?: CardTone; typeAt?: number; charsPerSecond?: number }[]
  from: number
  to: number
}

const ENTER_SECONDS = 0.6
const EXIT_SECONDS = 0.3
const ENTER_SCALE = 0.95
const DEFAULT_CHARS_PER_SECOND = 60

// A panel with a title and monospace lines, for config files and messages that are
// shown as text and not captured from an interface.
export function textCard(options: TextCardOptions): Part {
  const el = element('div', 'part text-card')
  Object.assign(el.style, { left: `${options.x}px`, top: `${options.y}px`, width: `${options.width}px` })
  if (options.title) {
    const title = element('div', 'text-card-title', el)
    title.textContent = tr(options.title)
  }
  const body = element('div', 'text-card-body', el)
  const rows = options.lines.map((line) => {
    const row = element('div', `text-card-line tone-${line.tone ?? 'plain'}`, body)
    return { row, text: tr(line.text), line }
  })

  return {
    el,
    render(time) {
      for (const { row, text, line } of rows) {
        const typeAt = line.typeAt
        row.textContent =
          typeAt === undefined ? text : typedPrefix(text, time - typeAt, line.charsPerSecond ?? DEFAULT_CHARS_PER_SECOND)
        row.style.visibility = typeAt !== undefined && time < typeAt ? 'hidden' : ''
        if (!row.textContent) row.textContent = '\u00a0' // keeps the line's height while empty
      }
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
      el.style.transform = `scale(${lerp(ENTER_SCALE, 1, entering)})`
    },
  }
}
