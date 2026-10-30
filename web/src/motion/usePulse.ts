// Loops an element's opacity between `dimmed` and 1 while `running` (Pulse.qml). On stop it eases
// back to 1 from wherever the pulse was, instead of finishing the cycle.

import { onScopeDispose, watch } from 'vue'
import type { Ref } from 'vue'
import { cssEasing } from './cssEasing.ts'
import { motion } from '../theme/generated/motion.ts'

interface PulseOptions {
  dimmed: number
  period?: number // ms for one dim-and-back cycle
}

export function usePulse(
  target: Readonly<Ref<HTMLElement | null>>,
  running: Readonly<Ref<boolean>>,
  { dimmed, period = motion.pulse }: PulseOptions,
): void {
  let loop: Animation | undefined

  function start(element: HTMLElement): void {
    loop = element.animate([{ opacity: 1 }, { opacity: dimmed }, { opacity: 1 }], {
      duration: period,
      iterations: Infinity,
      easing: cssEasing('--easing-in-out-sine'),
    })
  }

  function settle(element: HTMLElement): void {
    const current = getComputedStyle(element).opacity
    loop?.cancel()
    loop = undefined
    element.animate([{ opacity: current }, { opacity: 1 }], {
      duration: motion.standard,
      easing: cssEasing('--motion-ease'),
    })
  }

  watch(
    [target, running],
    ([element, isRunning]) => {
      if (!element) return
      if (isRunning && !loop) start(element)
      if (!isRunning && loop) settle(element)
    },
    { immediate: true, flush: 'post' },
  )
  onScopeDispose(() => loop?.cancel())
}
