<script setup lang="ts">
// The user's message (UserBlock.qml). It opens a turn: every message but the first sits below a
// divider that separates it from the previous reply. Short messages hug their text.
import { onMounted, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { cssEasing } from '../motion/cssEasing.ts'
import type { UserBlock } from '../model/blocks.ts'
import { motion } from '../theme/generated/motion.ts'

const props = defineProps<{ block: UserBlock; fresh: boolean }>()

const { t } = useI18n()
const divider = useTemplateRef<HTMLElement>('divider')

// Draws the divider across only for a freshly sent message.
onMounted(() => {
  if (!props.fresh) return
  divider.value?.animate([{ scale: '0 1' }, { scale: '1 1' }], {
    duration: motion.slow,
    easing: cssEasing('--motion-ease'),
  })
})
</script>

<template>
  <div class="user" :class="[`source-${block.source}`, { later: block.opensTurn }]">
    <div v-if="block.opensTurn" ref="divider" class="divider" />
    <div class="bubble">
      <span v-if="block.senderName" class="sender">{{ t('message.from', { name: block.senderName }) }}</span>
      <div class="text">{{ block.text }}</div>
    </div>
  </div>
</template>

<style scoped>
.user {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--turn-gap);
}

.user.later {
  padding-top: var(--turn-gap);
}

.divider {
  align-self: stretch;
  height: 1px;
  background: var(--border);
  transform-origin: left;
}

.bubble {
  max-width: 100%;
  padding: var(--user-block-vertical-padding) var(--user-block-horizontal-padding);
  border-radius: var(--radius-large);
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
}

.text {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.sender {
  display: block;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.source-shell .text {
  font: var(--code-font-size) / 1.45 var(--font-mono);
}

.source-reminder .bubble,
.source-compaction .bubble {
  color: var(--text-muted);
}
</style>
