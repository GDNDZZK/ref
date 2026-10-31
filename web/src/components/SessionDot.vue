<script setup lang="ts">
// A session's state as an 8px dot (the sidebar's and the tiles' StatusDot.qml): it pulses while the
// session works, and a turn that finishes plays the flourish in the success color before the dot
// settles to the idle color.
import { computed, onBeforeUnmount, ref, useTemplateRef, watch } from 'vue'
import { usePulse } from '../motion/usePulse.ts'
import { motion } from '../theme/generated/motion.ts'
import type { RowState } from '../shell/sessionRow.ts'
import Flourish from './Flourish.vue'

const props = defineProps<{ state: RowState }>()

const PULSE_DIMMED = 0.5
const WORKING: readonly RowState[] = ['busy', 'compacting', 'retrying', 'permission', 'question']

const dot = useTemplateRef<HTMLElement>('dot')
const flourish = useTemplateRef<InstanceType<typeof Flourish>>('flourish')
const pulsing = computed(() => WORKING.includes(props.state))
const finished = ref(false) // the success color, while the flourish plays
let settle: ReturnType<typeof setTimeout> | undefined

usePulse(dot, pulsing, { dimmed: PULSE_DIMMED })

watch(
  () => props.state,
  (state, previous) => {
    const turnEnded = previous !== undefined && WORKING.includes(previous) && state === 'idle'
    if (!turnEnded || !dot.value) return
    finished.value = true
    flourish.value?.play(dot.value)
    clearTimeout(settle)
    settle = setTimeout(() => (finished.value = false), motion.slow)
  },
)
onBeforeUnmount(() => clearTimeout(settle))
</script>

<template>
  <span class="dot-box" :class="[state, { finished }]">
    <Flourish ref="flourish" />
    <span ref="dot" class="dot" />
  </span>
</template>

<style scoped>
.dot-box {
  --state-color: var(--text-muted);
  position: relative;
  display: block;
  flex: none;
  width: 8px;
  height: 8px;
}

.busy,
.compacting {
  --state-color: var(--accent);
}

.retrying,
.permission,
.question {
  --state-color: var(--warning);
}

.error {
  --state-color: var(--danger);
}

.finished {
  --state-color: var(--success);
}

.dot {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: var(--state-color);
  transition:
    background-color var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.idle:not(.finished) .dot {
  opacity: 0.5;
}
</style>
