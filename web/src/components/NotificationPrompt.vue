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
  <Transition name="banner" appear>
    <div v-if="notifications.promptVisible" class="slot">
      <div class="fold">
        <div class="prompt" role="region" :aria-label="t('notifySettings.title')">
          <span>{{ t('notifySettings.prompt') }}</span>
          <TextButton
            class="allow"
            :label="t('notifySettings.allow')"
            @click="notifications.requestPermission()"
          />
          <TextButton :label="t('notifySettings.later')" @click="notifications.dismissPrompt()" />
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.slot {
  display: grid;
  flex: none;
  grid-template-rows: 1fr;
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.fold {
  min-height: 0;
  overflow: hidden;
}

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
  color: var(--accent-text);
}

.banner-enter-from,
.banner-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
}
</style>
