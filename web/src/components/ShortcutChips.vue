<script setup lang="ts">
// A shortcut as one chip per key: "Alt" "N".
import { computed } from 'vue'
import { formatShortcut } from '../commands/shortcuts.ts'
import type { Shortcut } from '../commands/types.ts'

// `keys` names the chips directly, for keys that are not commands (Esc Esc).
const props = defineProps<{ shortcut?: Shortcut; keys?: readonly string[] }>()
const chips = computed(() => props.keys ?? (props.shortcut ? formatShortcut(props.shortcut).split('+') : []))
</script>

<template>
  <span class="chips">
    <kbd v-for="(key, index) in chips" :key="index" class="chip">{{ key }}</kbd>
  </span>
</template>

<style scoped>
.chips {
  display: inline-flex;
  flex: none;
  gap: 4px;
}

.chip {
  padding: 2px 6px;
  border-radius: var(--radius-small);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text-muted);
  font: inherit;
  font-size: var(--small-font-size);
}
</style>
