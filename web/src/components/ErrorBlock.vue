<script setup lang="ts">
// Why a reply failed or was cut short (ErrorBlock.qml). Errors are rare and must not be missed.
//
// The card drops in neutral. Once it lands, its badge fades and springs in with a Flourish, and
// the error tint spreads out of it with it, as a hard-edged circle at a constant speed until it
// covers the card. A card that was there when the session opened starts covered.
import { onBeforeUnmount, onMounted, useTemplateRef } from 'vue'
import type { ErrorBlock } from '../model/blocks.ts'
import { tween } from '../motion/tween.ts'
import { components } from '../theme/generated/components.ts'
import { motion } from '../theme/generated/motion.ts'
import AlertBadge from './AlertBadge.vue'
import Flourish from './Flourish.vue'

const props = defineProps<{ block: ErrorBlock; fresh: boolean }>()

const RING_REACH = 2.2 // stays inside a one-line card
const SPREAD_SPEED = motion.slow // px per ms the circle's edge travels, as in ErrorBlock.qml
const EDGE_SOFTNESS = 1 // px over which the circle's edge fades

const { verticalPadding, horizontalPadding } = components.errorBlock

const card = useTemplateRef<HTMLElement>('card')
const badge = useTemplateRef<HTMLElement>('badge')
const flourish = useTemplateRef<InstanceType<typeof Flourish>>('flourish')
let landing: ReturnType<typeof setTimeout> | undefined
let stopSpread: (() => void) | undefined

// 0 = neutral card, 1 = covered by the tint.
function setSpread(
  element: HTMLElement,
  spread: number,
  radius: number,
  centerX: number,
  centerY: number,
): void {
  element.style.setProperty('--spread', String(spread))
  const covered = spread >= 1
  element.style.background = covered
    ? 'var(--error-surface)'
    : `radial-gradient(circle at ${centerX}px ${centerY}px, var(--error-surface) ${radius}px, transparent ${radius + EDGE_SOFTNESS}px), var(--surface)`
}

function arrive(element: HTMLElement, dot: HTMLElement): void {
  const box = element.getBoundingClientRect()
  const dotBox = dot.getBoundingClientRect()
  const centerX = dotBox.left - box.left + dotBox.width / 2
  const centerY = dotBox.top - box.top + dotBox.height / 2
  const startRadius = dotBox.width / 2
  const endRadius = Math.hypot(
    Math.max(centerX, box.width - centerX),
    Math.max(centerY, box.height - centerY),
  )
  setSpread(element, 0, startRadius, centerX, centerY)
  dot.style.opacity = '0' // the Flourish fades it in
  landing = setTimeout(() => {
    dot.style.opacity = ''
    flourish.value?.enter(dot)
    stopSpread = tween({
      from: 0,
      to: 1,
      durationMs: (endRadius - startRadius) / SPREAD_SPEED,
      easing: 'linear',
      update: (spread) =>
        setSpread(element, spread, startRadius + spread * (endRadius - startRadius), centerX, centerY),
    })
  }, motion.quick) // lets the drop land first
}

onMounted(() => {
  const element = card.value
  const dot = badge.value
  if (props.fresh && element && dot) arrive(element, dot)
})
onBeforeUnmount(() => {
  clearTimeout(landing)
  stopSpread?.()
})
</script>

<template>
  <div ref="card" class="error" :style="{ padding: `${verticalPadding}px ${horizontalPadding}px` }">
    <span class="badge">
      <Flourish ref="flourish" :ring-reach="RING_REACH" />
      <span ref="badge" class="badge-dot"><AlertBadge /></span>
    </span>
    <div class="message">{{ block.text }}</div>
  </div>
</template>

<style scoped>
.error {
  --state-color: var(--danger);
  --spread: 1;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  border-radius: var(--radius);
  background: var(--error-surface);
  box-shadow: inset 0 0 0 1px
    color-mix(in srgb, var(--error-border) calc(var(--spread) * 100%), var(--border));
}

.badge {
  position: relative;
  flex: none;
  width: 18px;
  height: 18px;
  margin-top: 2px; /* level with the first line of the message */
}

.badge-dot {
  position: absolute;
  inset: 0;
}

.message {
  flex: 1;
  min-width: 0;
  line-height: 22px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
