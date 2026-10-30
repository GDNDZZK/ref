<script setup lang="ts">
// The row above the input box: what the session is doing and for how long, and how full its
// context is. It is always there, so nothing jumps when a run starts.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNow } from '../composables/useNow.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { formatElapsed, formatTokens, secondsUntil } from '../model/format.ts'
import Icon from './Icon.vue'
import StatusDot from './StatusDot.vue'
import { SHIMMER_PERIOD_MS } from './toolLimits.ts'

const props = defineProps<{ session: ChatSession }>()

const MIN_ELAPSED_MS = 1000
const PERCENT = 100

const { t } = useI18n()
const now = useNow()

const activity = computed(() => props.session.activity)
const retry = computed(() => props.session.retry)
const elapsed = computed(() => {
  const since = props.session.busySince
  if (since === undefined || now.value - since < MIN_ELAPSED_MS) return ''
  return formatElapsed(now.value - since)
})
const retryText = computed(() =>
  retry.value
    ? t('status.retrying', {
        seconds: secondsUntil(retry.value.at, now.value),
        attempt: retry.value.attempt,
        error: retry.value.error.replace(/\s+/g, ' ').trim(),
      })
    : '',
)

const context = computed(() => props.session.context)
const usedFraction = computed(() => {
  const { used, max } = context.value ?? { used: 0, max: 0 }
  return max > 0 ? Math.min(1, used / max) : 0
})
const contextTone = computed(() => {
  const { used, recommend, auto } = context.value ?? { used: 0, recommend: 0, auto: 0 }
  if (auto > 0 && used >= auto) return 'danger'
  if (recommend > 0 && used >= recommend) return 'warning'
  return ''
})
const contextTitle = computed(() => {
  const { used, max, recommend, auto } = context.value ?? { used: 0, max: 0, recommend: 0, auto: 0 }
  return t('status.contextTitle', {
    used,
    max,
    recommend,
    auto,
    percent: Math.round(usedFraction.value * PERCENT),
  })
})
</script>

<template>
  <div class="statusline" :style="{ '--shimmer-period': `${SHIMMER_PERIOD_MS}ms` }">
    <div class="left">
      <template v-if="activity === 'thinking' || activity === 'running'">
        <StatusDot running :failed="false" />
        <span class="label shimmer">{{
          activity === 'thinking' ? t('status.working') : t('status.runningTools')
        }}</span>
      </template>
      <template v-else-if="activity === 'waiting_permission'">
        <Icon icon="lock-16" class="warning" />
        <span class="label warning">{{ t('status.waitingPermission') }}</span>
      </template>
      <template v-else-if="activity === 'compacting'">
        <StatusDot running :failed="false" />
        <span class="label shimmer accent">{{ t('status.compacting') }}</span>
      </template>
      <template v-else-if="activity === 'retrying'">
        <Icon icon="retry-16" class="warning" />
        <span class="label warning">{{ retryText }}</span>
      </template>
      <span v-if="elapsed && activity !== 'idle'" class="elapsed">· {{ elapsed }}</span>
    </div>
    <div v-if="context && context.max > 0" class="context" :class="contextTone" :title="contextTitle">
      <span>{{ formatTokens(context.used) }} / {{ formatTokens(context.max) }}</span>
      <span class="bar"><span class="fill" :style="{ width: `${usedFraction * PERCENT}%` }" /></span>
    </div>
  </div>
</template>

<style scoped>
.statusline {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  height: 28px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.elapsed {
  flex: none;
}

.warning {
  color: var(--warning);
}

/* A highlight three characters wide sweeps over the label once per period. */
.shimmer {
  --shimmer-high: var(--text);
  background: linear-gradient(
    90deg,
    var(--text-muted) calc(50% - 1.5ch),
    var(--shimmer-high) 50%,
    var(--text-muted) calc(50% + 1.5ch)
  );
  background-size: 250% 100%;
  background-clip: text;
  color: transparent;
  animation: shimmer var(--shimmer-period) linear infinite;
}

.shimmer.accent {
  --shimmer-high: var(--accent);
}

@keyframes shimmer {
  from {
    background-position: 100% 0;
  }

  to {
    background-position: 0 0;
  }
}

.context {
  display: flex;
  flex: none;
  align-items: center;
  gap: 8px;
}

.context.warning {
  color: var(--warning);
}

.context.danger {
  color: var(--danger);
}

.bar {
  width: 48px;
  height: 4px;
  overflow: hidden;
  border-radius: 2px;
  background: var(--border);
}

.fill {
  display: block;
  height: 100%;
  background: var(--accent);
  transition: width var(--motion-standard) var(--motion-ease);
}

.context.warning .fill {
  background: var(--warning);
}

.context.danger .fill {
  background: var(--danger);
}
</style>
