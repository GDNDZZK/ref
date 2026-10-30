<script setup lang="ts">
// The "it happened" flourish (Flourish.qml). It springs `target` to full size with a wobble and
// sends a ring out from its center. Place it before the target in the same positioned parent
// so the ring passes behind the target.
//   play():  on a shown target. It draws in to `gatheredScale` first.
//   enter(): on a target appearing. It fades in from `entryScale` as it springs open.
import { useTemplateRef } from 'vue'
import { cssEasing } from '../motion/cssEasing.ts'
import { motion } from '../theme/generated/motion.ts'
import { components } from '../theme/generated/components.ts'

const props = withDefaults(defineProps<{ ringReach?: number }>(), { ringReach: 3 }) // ring's final diameter, in target diameters

const RING_START_OPACITY = 0.7

const ring = useTemplateRef<HTMLElement>('ring')
let targetAnimation: Animation | undefined
let ringAnimations: Animation[] = []

function stopAll(): void {
  targetAnimation?.cancel()
  for (const animation of ringAnimations) animation.cancel()
}

function sendRing(target: HTMLElement, delayMs: number): void {
  if (!ring.value) return
  const diameter = target.offsetWidth
  const timing = {
    delay: delayMs,
    duration: motion.slow,
    easing: cssEasing('--motion-ease'),
    fill: 'backwards' as const,
  }
  ringAnimations = [
    ring.value.animate([{ width: `${diameter}px` }, { width: `${diameter * props.ringReach}px` }], timing),
    ring.value.animate([{ opacity: RING_START_OPACITY }, { opacity: 0 }], timing),
  ]
}

function play(target: HTMLElement): void {
  stopAll()
  const gather = motion.quick
  const spring = components.flourish.springDuration
  const gathered = components.flourish.gatheredScale
  targetAnimation = target.animate(
    [
      { scale: 1, offset: 0, easing: cssEasing('--easing-in-quad') },
      { scale: gathered, offset: gather / (gather + spring), easing: cssEasing('--easing-flourish-spring') },
      { scale: 1, offset: 1 },
    ],
    { duration: gather + spring },
  )
  sendRing(target, gather)
}

function enter(target: HTMLElement): void {
  stopAll()
  const entryScale = components.flourish.entryScale
  const spring = components.flourish.springDuration
  const quick = motion.quick
  const ease = cssEasing('--motion-ease')
  targetAnimation = target.animate(
    [{ scale: entryScale, easing: cssEasing('--easing-flourish-entry-spring') }, { scale: 1 }],
    { duration: spring },
  )
  ringAnimations = []
  target.animate([{ opacity: 0 }, { opacity: 1 }], { duration: quick, easing: ease })
  sendRing(target, 0)
}

defineExpose({ play, enter })
</script>

<template>
  <span class="flourish" aria-hidden="true"><span ref="ring" class="ring" /></span>
</template>

<style scoped>
.flourish {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.ring {
  position: absolute;
  left: 50%;
  top: 50%;
  width: 100%;
  aspect-ratio: 1;
  translate: -50% -50%;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1.5px var(--state-color, var(--accent));
  opacity: 0;
}
</style>
