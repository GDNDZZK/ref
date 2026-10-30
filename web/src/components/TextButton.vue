<script setup lang="ts">
// Small borderless button with a text label (TextButton.qml).
defineProps<{ label: string }>()
defineEmits<{ click: [] }>()
</script>

<template>
  <button type="button" class="text-button" @click="$emit('click')">
    <span class="face">{{ label }}</span>
  </button>
</template>

<style scoped>
.text-button {
  height: 24px;
}

.face {
  display: flex;
  align-items: center;
  height: 100%;
  padding: 0 8px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.text-button:hover .face {
  background: var(--surface-hover);
  color: var(--text);
}

.text-button:active .face {
  scale: 0.94;
}
</style>
