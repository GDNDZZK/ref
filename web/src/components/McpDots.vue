<script setup lang="ts">
// One dot per MCP server, at most six and then "+n" (the sidebar footer, McpDots.qml).
import { computed } from 'vue'
import type { McpServerStatus } from '../proto/generated'

const props = defineProps<{ servers: readonly McpServerStatus[] }>()

const MAX_DOTS = 6

const shown = computed(() => props.servers.slice(0, MAX_DOTS))
const hidden = computed(() => props.servers.length - shown.value.length)
</script>

<template>
  <span class="dots">
    <span
      v-for="server in shown"
      :key="server.name"
      class="dot"
      :class="server.status"
      :title="server.name"
    />
    <span v-if="hidden > 0" class="more">+{{ hidden }}</span>
  </span>
</template>

<style scoped>
.dots {
  display: flex;
  align-items: center;
  gap: 4px;
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--text-muted);
  opacity: 0.5;
}

.connected {
  background: var(--success);
  opacity: 1;
}

.connecting {
  background: var(--accent);
  opacity: 1;
  animation: dim var(--motion-pulse) var(--easing-in-out-sine) infinite;
}

.failed {
  background: var(--danger);
  opacity: 1;
}

.more {
  font-size: var(--small-font-size);
}

@keyframes dim {
  50% {
    opacity: 0.5;
  }
}
</style>
