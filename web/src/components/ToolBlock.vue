<script setup lang="ts">
// A tool call (ToolBlock.qml): a header with the eight states, and a body laid out by tool.
// A permission request is part of the card, which opens for it.
import { computed, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChatScroll } from '../composables/chatScroll.ts'
import { useChatSession } from '../composables/chatSessionContext.ts'
import { useCountdown } from '../composables/useCountdown.ts'
import { useNow } from '../composables/useNow.ts'
import { useStuck } from '../composables/useStuck.ts'
import type { ToolBlock } from '../model/blocks.ts'
import { formatClock, formatDuration, formatElapsed } from '../model/format.ts'
import { parseTodos, toolDetail, toolIcon } from '../model/toolContent.ts'
import { useNoticesStore } from '../stores/notices.ts'
import Chevron from './Chevron.vue'
import Icon from './Icon.vue'
import TextButton from './TextButton.vue'
import TodoList from './TodoList.vue'
import ToolBody from './ToolBody.vue'
import ToolPermission from './ToolPermission.vue'
import ToolStateMark from './ToolStateMark.vue'

const props = defineProps<{ block: ToolBlock }>()

const MS_PER_SECOND = 1000

const { t } = useI18n()
const chatScroll = useChatScroll()
const session = useChatSession()
const notices = useNoticesStore()
const now = useNow()
const root = useTemplateRef<HTMLElement>('root')
const header = useTemplateRef<HTMLElement>('header')
const sentinel = useTemplateRef<HTMLElement>('sentinel')
const stuck = useStuck(sentinel)

const state = computed(() => props.block.state)
const delayed = computed(() => state.value === 'delayed')
const countdown = useCountdown(
  computed(() => props.block.runAt),
  delayed,
)
const delayFraction = computed(() =>
  countdown.total.value > 0 ? countdown.remaining.value / countdown.total.value : 0,
)

const isTodo = computed(() => props.block.name === 'todo')
const todos = computed(() => (isTodo.value ? parseTodos(props.block.input) : []))
const detail = computed(() => toolDetail(props.block.name, props.block.input))
const awaitingPermission = computed(() => state.value === 'awaiting_permission')
const hasContent = computed(() => {
  const { input, rawInput, output, images } = props.block
  const hasInput = input !== null && !(typeof input === 'object' && Object.keys(input).length === 0)
  return (
    !isTodo.value &&
    (hasInput || rawInput !== '' || !!output || images.length > 0 || !!props.block.permission)
  )
})

// The default comes from the session; a click flips this card against it.
const flipped = ref(false)
const expanded = computed(
  () => hasContent.value && (awaitingPermission.value || session.detailsExpanded !== flipped.value),
)
const showPermission = ref(false) // once a request was there, the section stays to fold away
const bodyBuilt = ref(false) // built on first open, so a long list stays cheap

watch(
  () => props.block.permission,
  (request) => request && (showPermission.value = true),
  { immediate: true },
)
watch(expanded, (isExpanded) => isExpanded && (bodyBuilt.value = true), { immediate: true })

const firstErrorLine = computed(() =>
  state.value === 'error' && !expanded.value
    ? props.block.output
        ?.split('\n')
        .find((line) => line.trim() !== '')
        ?.trim()
    : undefined,
)

const statusText = computed(() => {
  const { time, timeEnd } = props.block
  switch (state.value) {
    case 'streaming':
      return t('tool.writing')
    case 'awaiting_permission':
      return t('tool.needsPermission')
    case 'delayed':
      return t('tool.runsIn', { seconds: Math.ceil(countdown.remaining.value / MS_PER_SECOND) })
    case 'running':
      return t('tool.running', { elapsed: formatElapsed(now.value - time) })
    case 'done':
      return timeEnd === null ? '' : formatDuration(timeEnd - time)
    case 'error':
      return `${t('tool.error')}${timeEnd === null ? '' : ` ${formatDuration(timeEnd - time)}`}`
    case 'denied':
      return t('tool.denied')
    case 'cancelled':
      return t('tool.cancelledState')
    default:
      return ''
  }
})

function toggle(): void {
  if (!hasContent.value || awaitingPermission.value) return
  if (expanded.value && stuck.value && root.value && header.value) {
    chatScroll.holdHeader(root.value, header.value.offsetHeight)
  }
  flipped.value = !flipped.value
}

function onFolded(event: TransitionEvent): void {
  if (event.propertyName === 'grid-template-rows') chatScroll.releaseHeader()
}

function answer(allow: boolean, always: boolean): void {
  session.answerPermission(props.block.callId, allow, always).catch((error) => notices.pushError(error))
}

function cancel(): void {
  session.stop().catch((error) => notices.pushError(error))
}
</script>

<template>
  <div
    ref="root"
    class="tool"
    :class="{
      warn: awaitingPermission || delayed,
      muted: state === 'denied' || state === 'cancelled',
      denied: state === 'denied',
    }"
  >
    <span ref="sentinel" class="sentinel" />
    <div
      ref="header"
      class="header"
      :class="{ stuck, clickable: hasContent && !awaitingPermission }"
      :role="hasContent ? 'button' : undefined"
      :tabindex="hasContent && !awaitingPermission ? 0 : undefined"
      :aria-expanded="hasContent ? expanded : undefined"
      @click="toggle"
      @keydown.enter.self.prevent="toggle"
      @keydown.space.self.prevent="toggle"
    >
      <ToolStateMark :state="state" :delay-fraction="delayFraction" />
      <Icon :icon="toolIcon(block.name)" class="type" />
      <span class="title">
        <span class="name">{{ block.name }}</span>
        <span v-if="block.purpose" class="purpose">{{ block.purpose }}</span>
        <span v-if="detail" class="detail">{{ detail }}</span>
      </span>
      <span v-if="delayed" @click.stop><TextButton :label="t('tool.cancel')" @click="cancel" /></span>
      <span class="status" :class="state">{{ statusText }}</span>
      <span v-if="block.time > 0" class="clock">{{ formatClock(block.time) }}</span>
      <Chevron class="chevron" :class="{ open: expanded, hidden: !hasContent || awaitingPermission }" />
    </div>
    <TodoList v-if="isTodo" :todos="todos" />
    <ToolPermission
      v-if="showPermission"
      :request="block.permission"
      :waiting="Math.max(0, session.permissions.length - 1)"
      @answer="answer"
    />
    <div v-if="firstErrorLine" class="error-line">└ {{ firstErrorLine }}</div>
    <div
      class="body"
      :class="{ open: expanded }"
      @transitionend.self="onFolded"
      @transitioncancel.self="onFolded"
    >
      <div class="fold">
        <ToolBody v-if="bodyBuilt" :block="block" />
      </div>
    </div>
    <span class="outline" />
  </div>
</template>

<style scoped>
.tool {
  --card: var(--surface);
  --edge: var(--border);
  position: relative;
  container-type: inline-size;
  border-radius: var(--radius);
  background: var(--card);
  overflow: clip;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.tool.warn {
  --edge: color-mix(in srgb, var(--warning) 60%, transparent);
}

.tool:has(.header.clickable:hover) {
  --card: var(--surface-hover);
}

/* The border sits on top, so a pinned header does not cover it. */
.outline {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  box-shadow: inset 0 0 0 1px var(--edge);
  pointer-events: none;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.sentinel {
  position: absolute;
  top: 0;
  left: 0;
  width: 1px;
  height: 1px;
  pointer-events: none;
}

/* Pinned to the top of the view while the output scrolls past under it. */
.header {
  position: sticky;
  top: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 38px;
  padding: 0 14px;
  background: var(--card);
  box-shadow: 0 1px 0 transparent;
  text-align: left;
  cursor: default;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.header.clickable {
  cursor: pointer;
}

.header.stuck {
  box-shadow: 0 1px 0 var(--border);
}

.type {
  flex: none;
  color: var(--text-muted);
}

.title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}

.name {
  font: bold var(--code-font-size) var(--font-mono);
}

.purpose,
.detail {
  margin-left: 10px;
}

.detail {
  color: var(--text-muted);
  font: var(--small-font-size) var(--font-mono);
}

.muted .purpose,
.muted .name {
  color: var(--text-muted);
}

.denied .name {
  text-decoration: line-through;
}

.status {
  flex: none;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition: color var(--motion-standard) var(--motion-ease);
}

.status.awaiting_permission,
.status.delayed {
  color: var(--warning);
}

.status.running {
  color: var(--accent);
}

.status.error {
  color: var(--danger);
}

.clock {
  display: none;
  flex: none;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

@container (min-width: 560px) {
  .clock {
    display: inline;
  }
}

.chevron {
  margin-left: -2px;
  color: var(--text-muted);
  transition:
    rotate var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.chevron.open {
  rotate: 90deg;
}

.chevron.hidden {
  opacity: 0;
}

.error-line {
  padding: 0 14px 8px;
  overflow: hidden;
  color: var(--danger);
  font: var(--small-font-size) var(--font-mono);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.body {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.body.open {
  grid-template-rows: 1fr;
}

.fold {
  min-height: 0;
  overflow: hidden;
}
</style>
