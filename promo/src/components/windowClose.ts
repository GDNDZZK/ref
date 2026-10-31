import { span } from '../engine/anim.ts'
import type { Part } from '../engine/part.ts'

const CLOSE_SECONDS = 0.4
const END_SCALE = 0.2

// N5: any window part shrinks to its centre while it fades, from `at` on. The part's own
// transform (its entrance) is kept and the shrink is applied on top.
export function windowClose<P extends Part>(part: P, at: number): P {
  return {
    ...part,
    el: part.el,
    async render(time) {
      await part.render(time)
      const closing = span(time, at, CLOSE_SECONDS, 'inCubic')
      if (closing <= 0) return
      part.el.style.transformOrigin = 'center'
      part.el.style.transform = `${part.el.style.transform} scale(${1 - (1 - END_SCALE) * closing})`
      part.el.style.opacity = String(Number(part.el.style.opacity || 1) * (1 - closing))
    },
  }
}
