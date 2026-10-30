<script setup lang="ts">
// The delay before a tool runs: a ring whose arc shrinks anticlockwise to nothing (ToolBlock.qml).
// It starts at 12 o'clock; `fraction` is the part of the ring still drawn.
import { computed } from 'vue'

const props = defineProps<{ fraction: number }>()

const SIZE = 16
const STROKE = 2
const RADIUS = (SIZE - STROKE) / 2
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

const dash = computed(() => `${Math.max(0, Math.min(1, props.fraction)) * CIRCUMFERENCE} ${CIRCUMFERENCE}`)
</script>

<template>
  <svg class="ring" :width="SIZE" :height="SIZE" :viewBox="`0 0 ${SIZE} ${SIZE}`" aria-hidden="true">
    <circle class="track" :cx="SIZE / 2" :cy="SIZE / 2" :r="RADIUS" :stroke-width="STROKE" />
    <circle
      class="arc"
      :cx="SIZE / 2"
      :cy="SIZE / 2"
      :r="RADIUS"
      :stroke-width="STROKE"
      :stroke-dasharray="dash"
      :transform="`rotate(-90 ${SIZE / 2} ${SIZE / 2})`"
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
