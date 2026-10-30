<script setup lang="ts">
// A button of a dialog footer (DialogButton.qml). Only the face shrinks on press.
withDefaults(
  defineProps<{
    label: string
    kind?: 'primary' | 'secondary' | 'danger'
    disabled?: boolean
    small?: boolean
  }>(),
  { kind: 'secondary', disabled: false, small: false },
)
defineEmits<{ click: [] }>()
</script>

<template>
  <button type="button" class="dialog-button" :disabled="disabled" @click="$emit('click')">
    <span class="face" :class="[kind, { small }]">{{ label }}</span>
  </button>
</template>

<style scoped>
.dialog-button:disabled {
  opacity: 0.4;
  cursor: default;
}

.face {
  display: flex;
  align-items: center;
  height: var(--button-height);
  padding: 0 18px;
  border-radius: calc(var(--button-height) / 2);
  font-size: calc(var(--font-size) - 1px);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    filter var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.face.small {
  height: 28px;
  padding: 0 14px;
}

.dialog-button:not(:disabled):active .face {
  scale: 0.94;
}

.secondary {
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.dialog-button:not(:disabled):hover .secondary {
  background: var(--surface-hover);
}

.primary {
  background: var(--accent);
  color: var(--on-accent);
}

.danger {
  background: var(--danger);
  color: var(--on-accent);
}

.dialog-button:not(:disabled):hover :is(.primary, .danger) {
  filter: brightness(1.08);
}
</style>
