<script setup lang="ts">
// A bar that asks to allow desktop notifications. The click on "Allow" is the user gesture the
// browser needs before it shows its own permission prompt.
import { useI18n } from 'vue-i18n'
import { useNotificationsStore } from '../stores/notifications.ts'
import TextButton from './TextButton.vue'

const { t } = useI18n()
const notifications = useNotificationsStore()
</script>

<template>
  <div
    v-if="notifications.promptVisible"
    class="prompt"
    role="region"
    :aria-label="t('notifySettings.title')"
  >
    <span>{{ t('notifySettings.prompt') }}</span>
    <TextButton class="allow" :label="t('notifySettings.allow')" @click="notifications.requestPermission()" />
    <TextButton :label="t('notifySettings.later')" @click="notifications.dismissPrompt()" />
  </div>
</template>

<style scoped>
.prompt {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: 4px 8px;
  padding: 6px var(--page-padding);
  background: var(--surface);
  box-shadow: 0 1px 0 var(--border);
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.allow :deep(.face) {
  color: var(--accent);
}
</style>
