import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface TimeSkipChipOptions {
  text: string | Localized
  // Stage position of the chip's top-left corner.
  x: number
  y: number
  from: number
  to: number
}

const ENTER_SECONDS = 0.35
const EXIT_SECONDS = 0.3
const ENTER_SCALE = 0.8

// N6: a small pill such as "+2 min" that says time was skipped at a cut.
export function timeSkipChip(options: TimeSkipChipOptions): Part {
  const el = element('div', 'part time-skip-chip')
  Object.assign(el.style, { left: `${options.x}px`, top: `${options.y}px` })
  el.textContent = tr(options.text)

  return {
    el,
    render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outBack')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(Math.min(1, entering) * (1 - leaving))
      el.style.transform = `scale(${ENTER_SCALE + (1 - ENTER_SCALE) * entering})`
    },
  }
}
