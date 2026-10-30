<script setup lang="ts">
// The prompts waiting for the session's next safe point, above the input box. Your own can be
// taken back; messages from other sessions and reminders only show who sent them.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Message } from '../proto/generated'
import Icon from './Icon.vue'

const QUEUE_MAX_ROWS = 4

const props = defineProps<{ messages: readonly Message[]; removing: ReadonlySet<string> }>()
defineEmits<{ remove: [messageId: string] }>()

const { t } = useI18n()

interface Row {
  id: string
  text: string
  imageCount: number
  sender: string | undefined // who it is from, when not you
  removable: boolean
}

function senderOf(message: Message): string | undefined {
  const source = message.source
  switch (source.kind) {
    case 'human':
    case 'shell':
    case 'compaction':
      return undefined
    case 'peer':
    case 'spawn':
      return t('message.from', { name: source.from_name })
    case 'reminder':
      return t('composer.queue.reminder')
  }
}

const rows = computed<Row[]>(() =>
  props.messages.map((message) => {
    const text = message.parts.flatMap((part) => (part.type === 'text' ? [part.text] : [])).join('\n\n')
    return {
      id: message.id,
      text: text.replace(/\s+/g, ' ').trim(),
      imageCount: message.parts.filter((part) => part.type === 'image').length,
      sender: senderOf(message),
      removable: message.source.kind === 'human',
    }
  }),
)
</script>

<template>
  <div class="queue" :class="{ shown: rows.length > 0 }">
    <div class="queue-inner">
      <TransitionGroup tag="ul" name="row" class="rows" :style="{ '--max-rows': QUEUE_MAX_ROWS }">
        <li v-for="row in rows" :key="row.id" class="row" :class="{ removing: removing.has(row.id) }">
          <div class="row-clip">
            <div class="row-content">
              <Icon icon="clock-16" class="muted" />
              <span class="text">{{ row.text }}</span>
              <span v-if="row.imageCount > 0" class="muted images">
                <Icon icon="image-16" />{{ row.imageCount }}
              </span>
              <span v-if="row.sender" class="muted sender">{{ row.sender }}</span>
              <button
                v-if="row.removable"
                type="button"
                class="recall"
                :disabled="removing.has(row.id)"
                @click="$emit('remove', row.id)"
              >
                <Icon icon="recall-16" />{{ t('composer.queue.remove') }}
              </button>
            </div>
          </div>
        </li>
      </TransitionGroup>
    </div>
  </div>
</template>

<style scoped>
.queue {
  --queue-gap: 8px;
  --row-height: 32px;
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.queue.shown {
  grid-template-rows: 1fr;
}

.queue-inner {
  min-height: 0;
  overflow: hidden;
}

.rows {
  box-sizing: content-box;
  max-height: calc(var(--max-rows) * (var(--row-height) + 1px));
  overflow-y: auto;
  margin: 0 0 var(--queue-gap);
  padding: 0;
  list-style: none;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.row {
  display: grid;
  grid-template-rows: 1fr;
  border-top: 1px solid var(--border);
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.row:first-child {
  border-top: 0;
}

.row-clip {
  min-height: 0;
  overflow: hidden;
}

.row-content {
  display: flex;
  align-items: center;
  gap: 8px;
  height: var(--row-height);
  padding: 0 12px;
  font-size: var(--small-font-size);
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.row.removing .row-content {
  opacity: 0.5;
}

.row-enter-from,
.row-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
}

.muted {
  color: var(--text-muted);
}

.text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.images,
.sender {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: none;
}

.recall {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: none;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition: color var(--motion-quick) var(--motion-ease);
}

.recall:hover:not(:disabled) {
  color: var(--text);
}
</style>
