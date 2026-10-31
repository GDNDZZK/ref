import { span } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'

export interface KeycapOptions {
  // One cap per key, e.g. ["esc", "esc"] or ["Enter"].
  keys: readonly string[]
  // Stage position of the caps' centre.
  x: number
  y: number
  // Each key is pressed at its time; the caps show from `from` to `to`.
  presses: readonly number[]
  from: number
  to: number
}

const ENTER_SECONDS = 0.3
const EXIT_SECONDS = 0.3
const PRESS_SECONDS = 0.18
const PRESS_DROP_PX = 6

// N8: keyboard keys drawn over a region, pressed one after another.
export function keycap(options: KeycapOptions): Part {
  const el = element('div', 'part keycaps')
  Object.assign(el.style, { left: `${options.x}px`, top: `${options.y}px` })
  const caps = options.keys.map((key) => {
    const cap = element('span', 'keycap', el)
    cap.textContent = key
    return cap
  })

  return {
    el,
    render(time) {
      caps.forEach((cap, index) => {
        const press = options.presses[index]
        const down = press !== undefined && time >= press && time < press + PRESS_SECONDS
        cap.dataset.down = down ? 'on' : 'off'
        cap.style.transform = `translateY(${down ? PRESS_DROP_PX : 0}px)`
      })
      const entering = span(time, options.from, ENTER_SECONDS, 'outExpo')
      const leaving = span(time, options.to - EXIT_SECONDS, EXIT_SECONDS, 'inCubic')
      el.style.opacity = String(entering * (1 - leaving))
    },
  }
}
