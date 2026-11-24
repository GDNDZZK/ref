<script setup lang="ts">
// Says why the conversation is not live: connecting, lost or closed. (An old backend has its own
// banner.)
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConnectionStore } from '../stores/connection.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import DialogButton from './DialogButton.vue'

const { t } = useI18n()
const connection = useConnectionStore()
const dialogs = useDialogsStore()
const text = computed(() => {
  if (connection.fatal) return t(connection.fatal)
  if (connection.state === 'closed') return t('connection.closed', { reason: connection.detail })
  if (connection.state === 'connecting')
    return connection.hello ? t('connection.reconnecting') : t('connection.connecting')
  return ''
})
const detail = computed(() =>
  connection.state === 'connecting' && connection.hello ? connection.detail : '',
)

// Retain last non-empty text while the banner folds away.
const shownText = ref('')
const shownDetail = ref('')
watch(
  [text, detail],
  ([nextText, nextDetail]) => {
    if (!nextText) return
    shownText.value = nextText
    shownDetail.value = nextDetail
  },
  { immediate: true },
)
</script>

<template>
  <Transition name="banner">
    <div v-if="text" class="slot">
      <div class="fold">
        <div class="banner" role="alert">
          <span class="message">
            {{ shownText }}
            <span v-if="shownDetail" class="detail">{{ shownDetail }}</span>
          </span>
          <DialogButton
            small
            class="switch-backend"
            :label="t('connection.switchBackend')"
            @click="dialogs.open({ kind: 'backend' })"
          />
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

.banner {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin: 0;
  padding: 8px var(--page-padding);
  background: var(--surface);
  box-shadow: 0 1px 0 var(--border);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  text-align: center;
}

.detail {
  margin-left: 8px;
  opacity: 0.7;
}
.banner-enter-from,
.banner-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
}
</style>
