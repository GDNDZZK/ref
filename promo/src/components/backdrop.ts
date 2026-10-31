import { lerp, span } from '../engine/anim.ts'
import { element, type Part } from '../engine/part.ts'

const DRIFT_PERIOD_SECONDS = 30

// Full-frame background: two soft accent glows that drift slowly.
export function backdrop(): Part {
  const el = element('div', 'part backdrop')
  return {
    el,
    render(time) {
      const drift = span(time % DRIFT_PERIOD_SECONDS, 0, DRIFT_PERIOD_SECONDS, 'linear')
      const glowA = `${lerp(15, 30, drift)}% ${lerp(0, 10, drift)}%`
      const glowB = `${lerp(90, 75, drift)}% ${lerp(100, 85, drift)}%`
      const tint = 'color-mix(in srgb, var(--accent) 16%, transparent)'
      el.style.background =
        `radial-gradient(900px 600px at ${glowA}, ${tint}, transparent), ` +
        `radial-gradient(800px 560px at ${glowB}, ${tint}, transparent)`
    },
  }
}
