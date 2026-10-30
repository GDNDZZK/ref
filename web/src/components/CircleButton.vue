<script setup lang="ts">
// Round surface button; the glyph is the slot (CircleButton.qml). Only the face shrinks on press.
// `--face` is a registered color (base.css), so glyphs painted with it follow the hover fade.
defineProps<{ label: string }>()
defineEmits<{ click: [] }>()
</script>

<template>
  <button type="button" class="circle" :aria-label="label" :title="label" @click="$emit('click')">
    <span class="face"><slot /></span>
  </button>
</template>

<style scoped>
.circle {
  --face: var(--surface);
  width: 34px;
  height: 34px;
  transition: --face var(--motion-quick) var(--motion-ease);
}

.circle:hover {
  --face: var(--surface-hover);
}

.face {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  background: var(--face);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: scale var(--motion-quick) var(--motion-ease);
}

.circle:active .face {
  scale: 0.92;
}
</style>
