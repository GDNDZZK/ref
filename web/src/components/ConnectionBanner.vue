<script setup lang="ts">
// Says why the conversation is not live: connecting, lost or closed. (An old backend has its own
// banner.)
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConnectionStore } from '../stores/connection.ts'

const { t } = useI18n()
const connection = useConnectionStore()

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
</script>

<template>
  <p v-if="text" class="banner" role="alert">
    {{ text }}
    <span v-if="detail" class="detail">{{ detail }}</span>
  </p>
</template>

<style scoped>
.banner {
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
</style>
