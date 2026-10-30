<script setup lang="ts">
// The status mark of a todo row: ring, ring with a pulsing dot, tick on a disc, or a cross.
import { computed, useTemplateRef } from 'vue'
import type { TodoStatus } from '../proto/generated'
import { usePulse } from '../motion/usePulse.ts'
import Icon from './Icon.vue'

const props = defineProps<{ status: TodoStatus }>()

const PULSE_DIMMED = 0.5

const dot = useTemplateRef<HTMLElement>('dot')
usePulse(
  dot,
  computed(() => props.status === 'in_progress'),
  { dimmed: PULSE_DIMMED },
)
</script>

<template>
  <span class="marker" :class="status">
    <Icon v-if="status === 'completed'" icon="check-16" class="tick" />
    <Icon v-else-if="status === 'cancelled'" icon="close-16" />
    <span v-else-if="status === 'in_progress'" ref="dot" class="dot" />
  </span>
</template>

<style scoped>
.marker {
  display: grid;
  flex: none;
  place-items: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  color: var(--text-muted);
  transition:
    background-color var(--motion-standard) var(--motion-ease),
    box-shadow var(--motion-standard) var(--motion-ease);
}

.pending,
.in_progress {
  box-shadow: inset 0 0 0 1.5px var(--text-muted);
}

.in_progress {
  box-shadow: inset 0 0 0 1.5px var(--accent);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent);
}

.completed {
  background: var(--success);
}

.tick {
  color: var(--on-accent);
}

.cancelled {
  border-radius: 0;
}
</style>
