<script setup lang="ts">
// "!" in a disc (AlertBadge.qml). The stroke and offsets are rounded to whole pixels.
import { computed } from 'vue'

const props = withDefaults(defineProps<{ size?: number }>(), { size: 18 })

const STROKE_DIVISOR = 9
const MIN_STROKE = 2
const BAR_TOP = 0.22
const BAR_HEIGHT = 0.39
const DOT_TOP = 0.67

const stroke = computed(() => Math.max(MIN_STROKE, Math.round(props.size / STROKE_DIVISOR)))
const bar = computed(() => ({
  top: `${Math.round(props.size * BAR_TOP)}px`,
  height: `${Math.round(props.size * BAR_HEIGHT)}px`,
  width: `${stroke.value}px`,
}))
const dot = computed(() => ({
  top: `${Math.round(props.size * DOT_TOP)}px`,
  width: `${stroke.value}px`,
  height: `${stroke.value}px`,
}))
</script>

<template>
  <span class="badge" :style="{ width: `${size}px`, height: `${size}px` }" aria-hidden="true">
    <span class="mark" :style="bar" />
    <span class="mark" :style="dot" />
  </span>
</template>

<style scoped>
.badge {
  position: relative;
  display: block;
  flex: none;
  border-radius: 50%;
  background: var(--danger);
}

.mark {
  position: absolute;
  left: 50%;
  translate: -50% 0;
  border-radius: 999px;
  background: var(--on-accent);
}
</style>
