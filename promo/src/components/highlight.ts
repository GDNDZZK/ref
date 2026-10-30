import { span } from '../engine/anim.ts'
import { tr, type Localized } from '../engine/i18n.ts'
import { element, type Part } from '../engine/part.ts'

export interface HighlightOptions {
  // Rectangle in picture coordinates, 0..1 each.
  x: number
  y: number
  width: number
  height: number
  label?: string | Localized
  from: number
  to: number
}

const ENTER_SECONDS = 0.45
const EXIT_SECONDS = 0.25
const ENTER_SCALE = 1.06

// An outlined area with an optional label. Attach it to an appFrame so it follows the camera.
export function highlight(options: HighlightOptions): Part {
  const el = element('div', 'part highlight')
  Object.assign(el.style, {
    left: `${options.x * 100}%`,
    top: `${options.y * 100}%`,
    width: `${options.width * 100}%`,
    height: `${options.height * 100}%`,
  })
  if (options.label) {
    const label = element('span', 'highlight-label', el)
    label.textContent = tr(options.label)
  }

  return {
    el,
    render(time) {
      const entering = span(time, options.from, ENTER_SECONDS, 'outBack')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(Math.min(1, entering) * (1 - leaving))
      el.style.transform = `scale(${ENTER_SCALE - (ENTER_SCALE - 1) * entering})`
    },
  }
}
