<script setup lang="ts">
// "Copy" under a message: the raw text goes to the clipboard, the icon shows a check for a moment.
import { useI18n } from 'vue-i18n'
import { useChatSession } from '../composables/chatSessionContext.ts'
import { useCopyFeedback } from '../composables/useCopyFeedback.ts'
import { motion } from '../theme/generated/motion.ts'
import Icon from './Icon.vue'

const props = defineProps<{ messageId: string }>()

const { t } = useI18n()
const session = useChatSession()
const { copied, copy } = useCopyFeedback(() => session.copyMessage(props.messageId), motion.slow)
</script>

<template>
  <button type="button" class="copy" @click="copy">
    <Icon :icon="copied ? 'check-16' : 'copy-16'" />{{ t('meta.copy') }}
  </button>
</template>

<style scoped>
.copy {
  display: flex;
  align-items: center;
  gap: 4px;
  color: inherit;
}

.copy:hover {
  color: var(--text);
}
</style>
