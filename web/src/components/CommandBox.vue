<script setup lang="ts">
// A shell command as `$ cmd` on a code background, wrapped.
import CopyArea from './CopyArea.vue'

defineProps<{ command: string }>()
</script>

<template>
  <CopyArea :text="command">
    <pre class="command"><span class="prompt">$ </span>{{ command }}</pre>
  </CopyArea>
</template>

<style scoped>
.command {
  margin: 0;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--code-background);
  font: bold var(--code-font-size) / 1.45 var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.prompt {
  color: var(--text-muted);
}
</style>
