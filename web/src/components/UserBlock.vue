<script setup lang="ts">
// The user's message (UserBlock.qml). It opens a turn: every message but the first sits below a
// divider that separates it from the previous reply. Short messages hug their text.
import { computed, onMounted, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { renderMarkdown } from '../markdown/markdown.ts'
import { cssEasing } from '../motion/cssEasing.ts'
import type { UserBlock } from '../model/blocks.ts'
import { motion } from '../theme/generated/motion.ts'
import Icon from './Icon.vue'
import ImageThumbs from './ImageThumbs.vue'
import { USER_THUMBNAIL_HEIGHT } from './toolLimits.ts'

const props = defineProps<{ block: UserBlock; fresh: boolean }>()

const { t } = useI18n()
const divider = useTemplateRef<HTMLElement>('divider')

// Messages that are not what the user typed say where they come from.
const caption = computed(() => {
  const { source, senderName } = props.block
  if (source === 'peer')
    return { icon: 'chat-send-16', text: t('message.fromPeer', { name: senderName }) } as const
  if (source === 'spawn')
    return { icon: 'chat-plus-16', text: t('message.createdBy', { name: senderName }) } as const
  if (source === 'reminder') return { icon: 'retry-16', text: t('message.reminder') } as const
  if (source === 'shell') return { icon: 'terminal-16', text: t('message.shell') } as const
  return undefined
})
const shellHtml = computed(() => (props.block.source === 'shell' ? renderMarkdown(props.block.text) : ''))

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
      <div v-if="caption" class="caption">
        <Icon :icon="caption.icon" />
        <span>{{ caption.text }}</span>
      </div>
      <!-- eslint-disable-next-line vue/no-v-html -->
      <div v-if="shellHtml" class="text shell prose" v-html="shellHtml" />
      <div v-else class="text">{{ block.text }}</div>
    </div>
    <ImageThumbs v-if="block.images.length > 0" :images="block.images" :height="USER_THUMBNAIL_HEIGHT" />
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
  --bar: transparent;
  max-width: 100%;
  padding: var(--user-block-vertical-padding) var(--user-block-horizontal-padding);
  border-radius: var(--radius-large);
  background: var(--user-bubble);
  box-shadow:
    inset 3px 0 0 var(--bar),
    inset 0 0 0 1px var(--user-bubble-border);
}

.text {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.caption {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
  color: var(--tone);
  font-size: var(--small-font-size);
  font-weight: bold;
}

.source-peer .bubble,
.source-spawn .bubble,
.source-reminder .bubble,
.source-shell .bubble {
  background: var(--surface);
  box-shadow: inset 3px 0 0 var(--bar);
}

.source-peer .bubble,
.source-spawn .bubble {
  --bar: var(--info);
  --tone: var(--info);
}

.source-reminder .bubble,
.source-shell .bubble {
  --bar: var(--warning);
  --tone: var(--warning);
}

.source-shell .bubble {
  background: var(--code-background);
}

.shell {
  font: var(--code-font-size) / 1.45 var(--font-mono);
  white-space: normal;
}
</style>
