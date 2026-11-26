import { nextTick } from 'vue'
import { prefersReducedMotion } from '../motion/reducedMotion.ts'

export interface RadialReveal {
  x: number
  y: number
  expand: boolean
}

let fadeToken = 0

function fadeMode(speed: 'slow' | 'standard', radial?: RadialReveal): string {
  if (!radial) return speed
  return radial.expand ? 'radial-expand' : 'radial-shrink'
}

// Runs `change` under a snapshot of the old look and either fades the snapshot out (base.css) or
// reveals the new look via a circular clip-path wave around `radial`. Without view transitions or
// under reduced motion the change applies at once.
export async function crossfade(
  change: () => void,
  speed: 'slow' | 'standard' = 'slow',
  radial?: RadialReveal,
): Promise<void> {
  if (!document.startViewTransition || prefersReducedMotion()) return change()
  const token = ++fadeToken
  document.documentElement.dataset['fade'] = fadeMode(speed, radial)
  const transition = document.startViewTransition(async () => {
    change()
    await nextTick() // the new DOM must be in place when the snapshot is taken
  })
  // An aborted transition (timeout, viewport resize) rejects `ready` and `finished` alike.
  if (radial) {
    void transition.ready
      .then(() => {
        const maxR = Math.hypot(
          Math.max(radial.x, window.innerWidth - radial.x),
          Math.max(radial.y, window.innerHeight - radial.y),
        )
        const start = radial.expand
          ? `circle(0px at ${radial.x}px ${radial.y}px)`
          : `circle(${maxR}px at ${radial.x}px ${radial.y}px)`
        const end = radial.expand
          ? `circle(${maxR}px at ${radial.x}px ${radial.y}px)`
          : `circle(0px at ${radial.x}px ${radial.y}px)`
        document.documentElement.animate(
          { clipPath: [start, end] },
          {
            duration: 460,
            easing: 'cubic-bezier(0.65, 0, 0.35, 1)',
            fill: 'both',
            pseudoElement: radial.expand ? '::view-transition-new(root)' : '::view-transition-old(root)',
          },
        )
      })
      .catch(() => undefined)
  } else {
    void transition.ready.catch(() => undefined)
  }
  void transition.finished
    .catch(() => undefined)
    .finally(() => {
      if (token === fadeToken) delete document.documentElement.dataset['fade']
    })
  await transition.updateCallbackDone
}
