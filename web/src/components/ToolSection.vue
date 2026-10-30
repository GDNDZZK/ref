<script setup lang="ts">
// A labelled part of a tool card's body; chips (exit code, counts) follow the label.
defineProps<{ label?: string }>()
</script>

<template>
  <section class="section">
    <div v-if="label || $slots.chips" class="head">
      <span class="label">{{ label }}</span>
      <slot name="chips" />
    </div>
    <slot />
  </section>
</template>

<style scoped>
.section + .section {
  margin-top: 12px;
}

.head {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 500;
}
</style>
