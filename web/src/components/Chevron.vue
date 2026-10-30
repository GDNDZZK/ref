<script setup lang="ts">
// A ">" stroke centered in its box (Chevron.qml); rotate it from outside to point elsewhere.
import { computed } from 'vue'

const props = withDefaults(defineProps<{ size?: number; strokeWidth?: number }>(), {
  size: 12,
  strokeWidth: 1.5,
})

const TIP_TO_END = 0.3 // of the box, vertically and horizontally (a right angle at the tip)

const points = computed(() => {
  const halfSpan = props.size * TIP_TO_END
  const endsX = (props.size - halfSpan) / 2
  const middle = props.size / 2
  return `${endsX},${middle - halfSpan} ${endsX + halfSpan},${middle} ${endsX},${middle + halfSpan}`
})
</script>

<template>
  <svg :width="size" :height="size" :viewBox="`0 0 ${size} ${size}`" aria-hidden="true">
    <polyline
      :points="points"
      fill="none"
      stroke="currentColor"
      :stroke-width="strokeWidth"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
</template>

<style scoped>
svg {
  display: block;
  flex: none;
}
</style>
