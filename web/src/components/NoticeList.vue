<script setup lang="ts">
// Short messages from the backend and from failed requests. Click to dismiss.
import { useNoticesStore } from '../stores/notices.ts'

const store = useNoticesStore()
</script>

<template>
  <TransitionGroup tag="div" name="notice" class="notices" role="status">
    <button
      v-for="notice in store.notices"
      :key="notice.id"
      type="button"
      class="notice"
      :class="notice.level"
      @click="store.dismiss(notice.id)"
    >
      {{ notice.text }}
    </button>
  </TransitionGroup>
</template>

<style scoped>
.notices {
  position: fixed;
  top: 14px;
  left: 50%;
  z-index: 10;
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: min(520px, calc(100% - 120px));
  translate: -50% 0;
  pointer-events: none;
}

.notice {
  padding: 10px 14px;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  font-size: var(--small-font-size);
  text-align: left;
  pointer-events: auto;
}

.notice.warn {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.notice.error {
  background: var(--error-surface);
  box-shadow: inset 0 0 0 1px var(--error-border);
}

.notice-enter-active,
.notice-leave-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease);
}

.notice-enter-from,
.notice-leave-to {
  opacity: 0;
  translate: 0 -10px;
}
</style>
