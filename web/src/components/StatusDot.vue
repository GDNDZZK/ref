<script setup lang="ts">
// Tool status: pulses while running, plays a Flourish in its final color on finishing (StatusDot.qml).
import { ref, useTemplateRef, watch } from 'vue'
import { usePulse } from '../motion/usePulse.ts'
import Flourish from './Flourish.vue'

const props = defineProps<{ running: boolean; failed: boolean; muted?: boolean; color?: string }>()

const PULSE_DIMMED = 0.5

const dot = useTemplateRef<HTMLElement>('dot')
const flourish = useTemplateRef<InstanceType<typeof Flourish>>('flourish')
const running = ref(props.running)

usePulse(dot, running, { dimmed: PULSE_DIMMED })

watch(
  () => props.running,
  (isRunning) => {
    running.value = isRunning
    if (!isRunning && dot.value) flourish.value?.play(dot.value)
  },
)
</script>

<template>
  <span
    class="status"
    :class="{ running, failed, muted }"
    :style="color ? { '--state-color': color } : undefined"
  >
    <Flourish ref="flourish" />
    <span ref="dot" class="dot" />
  </span>
</template>

<style scoped>
.status {
  --state-color: var(--success);
  position: relative;
  display: block;
  flex: none;
  width: 8px;
  height: 8px;
}

.status.failed {
  --state-color: var(--danger);
}

.status.muted {
  --state-color: var(--text-muted);
}

.status.running {
  --state-color: var(--accent);
}

.dot {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: var(--state-color);
  transition: background-color var(--motion-standard) var(--motion-ease);
}
</style>
