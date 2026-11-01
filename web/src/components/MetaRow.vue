<script setup lang="ts">
// The footer under a message (the meta row of the spec): time, who answered, tokens, and the
// copy and undo actions. Dim until the pointer is over the message.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChatScroll } from '../composables/chatScroll.ts'
import { useChatSession } from '../composables/chatSessionContext.ts'
import { useNow } from '../composables/useNow.ts'
import { formatClock, formatDuration, formatElapsed, formatTokens } from '../model/format.ts'
import type { MetaBlock } from '../model/blocks.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { agentColorByName } from '../theme/agentColor.ts'
import MetaCopy from './MetaCopy.vue'
import Icon from './Icon.vue'
import StatusDot from './StatusDot.vue'

const props = defineProps<{ block: MetaBlock }>()

const MS_PER_SECOND = 1000

const { t } = useI18n()
const session = useChatSession()
const notices = useNoticesStore()
const connection = useConnectionStore()
const { hoveredMessageId } = useChatScroll()
const now = useNow()

const lit = computed(() => hoveredMessageId.value === props.block.messageId)
const assistant = computed(() => props.block.role === 'assistant')
const agentName = computed(() => props.block.agent ?? '')
const color = computed(() =>
  props.block.failed
    ? 'var(--text-muted)'
    : agentColorByName(connection.config?.agents ?? [], agentName.value),
)
const span = computed(() => (props.block.timeEnd === null ? 0 : props.block.timeEnd - props.block.time))
const tokensPerSecond = computed(() =>
  props.block.tokensOut > 0 && span.value > 0
    ? Math.round((props.block.tokensOut * MS_PER_SECOND) / span.value)
    : 0,
)
const fullDate = computed(() => new Date(props.block.time).toLocaleString())

function undo(): void {
  session.undo(props.block.messageId).catch((error) => notices.pushError(error))
}
</script>

<template>
  <div class="meta" :class="{ lit, user: !assistant }">
    <div v-if="assistant" class="facts">
      <span class="fact agent" :style="{ '--agent': color }">
        <StatusDot :running="block.streaming" :failed="false" :color="color" />
        <span class="agent-name">{{ agentName }}</span>
      </span>
      <span v-if="block.model" class="fact">
        <span class="sep">·</span>
        <span
          >{{ block.model }}<span v-if="block.variant" class="variant"> {{ block.variant }}</span></span
        >
      </span>
      <span class="fact" :title="fullDate">
        <span class="sep">·</span>
        <template v-if="block.streaming">{{ formatElapsed(now - block.time) }}</template>
        <template v-else
          >{{ formatClock(block.time)
          }}<template v-if="block.timeEnd !== null">
            → {{ formatClock(block.timeEnd) }} ({{ formatDuration(span) }})</template
          ></template
        >
      </span>
      <span v-if="block.tokensIn > 0 || block.tokensOut > 0" class="fact">
        <span class="sep">·</span>
        <span>↑ {{ formatTokens(block.tokensIn) }} ↓ {{ formatTokens(block.tokensOut) }}</span>
      </span>
      <span v-if="tokensPerSecond > 0" class="fact rate">{{
        t('meta.tokensPerSecond', { rate: tokensPerSecond })
      }}</span>
      <span v-if="block.interrupted" class="fact interrupted">{{ t('meta.interrupted') }}</span>
    </div>
    <template v-else>
      <span :title="fullDate">{{ formatClock(block.time) }}</span>
      <template v-if="block.canUndo">
        <span>·</span>
        <button type="button" class="action" @click="undo">
          <Icon icon="undo-16" />{{ t('meta.undo') }}
        </button>
      </template>
      <span>·</span>
    </template>
    <MetaCopy v-if="block.copyable" class="copy" :message-id="block.messageId" />
  </div>
</template>

<style scoped>
.meta {
  --row-height: 24px;

  display: flex;
  align-items: center;
  gap: 8px;
  min-height: var(--row-height);
  white-space: nowrap;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  opacity: 0.6;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.meta.lit {
  opacity: 1;
}

.meta.user {
  justify-content: flex-end;
}

/* The reply's facts, left. A fact that does not fit wraps below the row and is clipped with all
   that follow it (as in the GUI), so a fact never breaks inside and never runs under the actions. */
.facts {
  display: flex;
  flex: 1 1 0;
  flex-wrap: wrap;
  align-content: flex-start;
  align-items: center;
  gap: 0 8px;
  min-width: 0;
  height: var(--row-height);
  overflow: hidden;
}

.fact {
  display: flex;
  align-items: center;
  gap: 8px;
  height: var(--row-height); /* a whole row, so the wrapped part starts below the clip */
}

.copy {
  flex: none;
}

.agent {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--agent);
}

.agent-name {
  text-transform: capitalize;
}

.variant,
.interrupted {
  color: var(--warning);
}

.rate {
  opacity: 0.6;
}

.action {
  display: flex;
  align-items: center;
  gap: 4px;
  color: inherit;
}

.action:hover {
  color: var(--text);
}
</style>
