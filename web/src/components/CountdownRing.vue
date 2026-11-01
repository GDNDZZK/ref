<script setup lang="ts">
// The delay before a tool runs: a ring whose arc shrinks anticlockwise to nothing (ToolBlock.qml).
// It starts at 12 o'clock; `fraction` is the part of the ring still drawn.
import { computed } from 'vue'
import { clamp } from '../clamp.ts'

const props = defineProps<{ fraction: number }>()

const SIZE = 16
const STROKE = 2
const CENTER = SIZE / 2
const RADIUS = (SIZE - STROKE) / 2
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

const dash = computed(() => `${clamp(props.fraction, 0, 1) * CIRCUMFERENCE} ${CIRCUMFERENCE}`)
</script>

<template>
  <svg class="ring" :width="SIZE" :height="SIZE" :viewBox="`0 0 ${SIZE} ${SIZE}`" aria-hidden="true">
    <circle class="track" :cx="CENTER" :cy="CENTER" :r="RADIUS" :stroke-width="STROKE" />
    <circle
      class="arc"
      :cx="CENTER"
      :cy="CENTER"
      :r="RADIUS"
      :stroke-width="STROKE"
      :stroke-dasharray="dash"
      :transform="`rotate(-90 ${CENTER} ${CENTER})`"
    />
  </svg>
</template>

<style scoped>
.ring {
  display: block;
  fill: none;
}

.track {
  stroke: var(--border);
}

.arc {
  stroke: var(--warning);
  stroke-linecap: round;
}
</style>
