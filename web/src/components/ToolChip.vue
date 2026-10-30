<script setup lang="ts">
// A small fact about a result: `exit 1`, `truncated`, `3 results`.
defineProps<{ danger?: boolean }>()
</script>

<template>
  <span class="chip" :class="{ danger }"><slot /></span>
</template>

<style scoped>
.chip {
  padding: 0 6px;
  border-radius: 4px;
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text-muted);
  font: 11px / 16px var(--font-mono);
}

.danger {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--danger) 50%, transparent);
  color: var(--danger);
}
</style>
