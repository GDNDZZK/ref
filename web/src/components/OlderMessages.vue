<script setup lang="ts">
// The first row when older messages exist: a pill that loads the next page (they also load by
// themselves when the view is scrolled near the top). A failed load shows its error here.
import { useI18n } from 'vue-i18n'
import Icon from './Icon.vue'
import StatusDot from './StatusDot.vue'

defineProps<{ loading: boolean; error: string }>()
defineEmits<{ load: [] }>()

const { t } = useI18n()
</script>

<template>
  <div class="older">
    <button
      type="button"
      class="pill"
      :class="{ failed: error !== '' }"
      :disabled="loading"
      @click="$emit('load')"
    >
      <StatusDot v-if="loading" running :failed="false" color="var(--accent)" />
      <Icon v-else icon="history-16" />
      <span>{{ error || t('chat.loadOlder') }}</span>
    </button>
  </div>
</template>

<style scoped>
.older {
  display: flex;
  justify-content: center;
}

.pill {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 28px;
  padding: 0 12px;
  border-radius: 14px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.pill:hover:not(:disabled) {
  background: var(--surface-hover);
}

.pill.failed {
  color: var(--danger);
}
</style>
