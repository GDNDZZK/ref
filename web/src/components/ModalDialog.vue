<script setup lang="ts">
// The frame of a dialog (ModalDialog.qml): header with title and close button, scrolling content and
// an optional footer for the buttons. The scrim and the entrance belong to DialogHost.
import { useI18n } from 'vue-i18n'
import Icon from './Icon.vue'

withDefaults(defineProps<{ title: string; width?: 'normal' | 'wide' | 'settings' }>(), { width: 'normal' })
const emit = defineEmits<{ close: [] }>()

const { t } = useI18n()
</script>

<template>
  <section class="dialog" :class="width" role="dialog" aria-modal="true" :aria-label="title">
    <header class="header">
      <h2 class="title">{{ title }}</h2>
      <button type="button" class="close" :aria-label="t('dialog.close')" @click="emit('close')">
        <Icon icon="close-20" />
      </button>
    </header>
    <div class="content"><slot /></div>
    <footer v-if="$slots['footer']" class="footer"><slot name="footer" /></footer>
  </section>
</template>

<style scoped>
.dialog {
  display: flex;
  flex-direction: column;
  width: min(var(--dialog-width), calc(100vw - 24px));
  max-height: calc(var(--dialog-max-height-ratio) * 100vh);
  overflow: hidden;
  border-radius: var(--radius-large);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 12px 40px #00000040;
}

.dialog.wide {
  width: min(var(--dialog-width-wide), calc(100vw - 24px));
}

.dialog.settings {
  width: min(var(--settings-width), calc(100vw - 24px));
}

.header {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  height: var(--dialog-header-height);
  padding: 0 14px 0 22px;
}

.title {
  margin: 0;
  font-size: calc(var(--font-size) + 1px);
  font-weight: 700;
}

.close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 50%;
  color: var(--text-muted);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.close:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.close:active {
  scale: 0.94;
}

.content {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  padding: 0 22px 20px;
  overflow-y: auto;
}

.footer {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  height: var(--dialog-footer-height);
  padding: 0 22px;
}
</style>
