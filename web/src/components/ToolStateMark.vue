<script setup lang="ts">
// The 16 x 16 slot at the left of a tool card: one mark for each of the eight states.
import type { ToolState } from '../proto/generated'
import CountdownRing from './CountdownRing.vue'
import Icon from './Icon.vue'
import StatusDot from './StatusDot.vue'

defineProps<{ state: ToolState; delayFraction: number }>()
</script>

<template>
  <span class="mark">
    <Icon v-if="state === 'awaiting_permission'" icon="lock-16" class="lock" />
    <CountdownRing v-else-if="state === 'delayed'" :fraction="delayFraction" />
    <span v-else-if="state === 'denied' || state === 'cancelled'" class="hollow" />
    <StatusDot v-else :running="state === 'streaming' || state === 'running'" :failed="state === 'error'" />
  </span>
</template>

<style scoped>
.mark {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
}

.lock {
  color: var(--warning);
}

.hollow {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1.5px var(--text-muted);
}
</style>
