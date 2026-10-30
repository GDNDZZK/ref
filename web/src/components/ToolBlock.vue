<script setup lang="ts">
// A tool call: status, title and, on demand, its input and output (ToolBlock.qml).
import { computed, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChatScroll } from '../composables/chatScroll.ts'
import { useStuck } from '../composables/useStuck.ts'
import type { ToolBlock } from '../model/blocks.ts'
import { useConversationStore } from '../stores/conversation.ts'
import { useNoticesStore } from '../stores/notices.ts'
import Chevron from './Chevron.vue'
import StatusDot from './StatusDot.vue'

const props = defineProps<{ block: ToolBlock }>()

const RUNNING_STATES: readonly string[] = ['streaming', 'delayed', 'running', 'awaiting_permission']
const MUTED_STATES: readonly string[] = ['denied', 'cancelled']
const JSON_INDENT = 2

const { t } = useI18n()
const chatScroll = useChatScroll()
const conversation = useConversationStore()
const notices = useNoticesStore()
const root = useTemplateRef<HTMLElement>('root')
const header = useTemplateRef<HTMLElement>('header')
const sentinel = useTemplateRef<HTMLElement>('sentinel')
const stuck = useStuck(sentinel)

const chosen = ref<boolean>() // set once the user toggles
const opened = ref(false) // the body is built on first open, so a long list stays cheap

const title = computed(() => `${props.block.name} ${props.block.purpose}`.trim())
const running = computed(() => RUNNING_STATES.includes(props.block.state))
const failed = computed(() => props.block.state === 'error')
const muted = computed(() => MUTED_STATES.includes(props.block.state))
const awaitingPermission = computed(() => props.block.permission !== undefined)
const hasInput = computed(() => {
  const { input } = props.block
  return input !== null && !(typeof input === 'object' && Object.keys(input).length === 0)
})
const inputText = computed(() => {
  if (!opened.value) return ''
  return hasInput.value ? JSON.stringify(props.block.input, null, JSON_INDENT) : props.block.rawInput
})
const hasContent = computed(
  () => hasInput.value || props.block.rawInput !== '' || !!props.block.output || awaitingPermission.value,
)
const expanded = computed(() => chosen.value ?? awaitingPermission.value)

watch(expanded, (isExpanded) => (opened.value ||= isExpanded), { immediate: true })

function toggle(): void {
  if (!hasContent.value) return
  if (expanded.value && stuck.value && root.value && header.value) {
    chatScroll.holdHeader(root.value, header.value.offsetHeight)
  }
  chosen.value = !expanded.value
}

function onFolded(event: TransitionEvent): void {
  if (event.propertyName === 'grid-template-rows') chatScroll.releaseHeader()
}

function answer(allow: boolean, always: boolean): void {
  conversation.answerPermission(props.block.callId, allow, always).catch((error) => notices.pushError(error))
}
</script>

<template>
  <div ref="root" class="tool">
    <span ref="sentinel" class="sentinel" />
    <button
      ref="header"
      type="button"
      class="header"
      :class="{ stuck, clickable: hasContent }"
      :aria-expanded="hasContent ? expanded : undefined"
      @click="toggle"
    >
      <StatusDot :running="running" :failed="failed" :muted="muted" />
      <span class="title">{{ title }}</span>
      <Chevron class="chevron" :class="{ open: expanded, hidden: !hasContent }" />
    </button>
    <div
      class="body"
      :class="{ open: expanded }"
      @transitionend.self="onFolded"
      @transitioncancel.self="onFolded"
    >
      <div class="fold">
        <div v-if="opened" class="content">
          <pre v-if="inputText" class="text" :aria-label="t('tool.input')">{{ inputText }}</pre>
          <div v-if="block.permission" class="permission">
            <button type="button" class="choice allow" @click="answer(true, false)">
              {{ t('tool.allow') }} <kbd>a</kbd>
            </button>
            <button type="button" class="choice" @click="answer(true, true)">
              {{ t('tool.allowSession') }} <kbd>s</kbd>
            </button>
            <button type="button" class="choice" @click="answer(false, false)">
              {{ t('tool.deny') }} <kbd>d</kbd>
            </button>
          </div>
          <pre v-if="block.output" class="text output">{{ block.output }}</pre>
        </div>
      </div>
    </div>
    <span class="outline" />
  </div>
</template>

<style scoped>
.tool {
  --card: var(--surface);
  position: relative;
  border-radius: var(--radius);
  background: var(--card);
  overflow: clip;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.tool:has(.header.clickable:hover) {
  --card: var(--surface-hover);
}

/* The border sits on top, so a pinned header does not cover it. */
.outline {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  box-shadow: inset 0 0 0 1px var(--border);
  pointer-events: none;
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

.title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font: var(--code-font-size) var(--font-mono);
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

.content {
  border-top: 1px solid var(--border);
}

.text {
  margin: 0;
  padding: 10px 14px 12px;
  color: var(--text-muted);
  font: var(--small-font-size) / 1.45 var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.output {
  border-top: 1px solid var(--border);
}

.content > .text:first-child {
  border-top: 0;
}

.permission {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 0 14px 12px;
}

.choice {
  padding: 4px 10px;
  border-radius: var(--radius-small);
  background: var(--surface-hover);
  font-size: var(--small-font-size);
}

.choice.allow {
  background: var(--accent);
  color: var(--on-accent);
}

kbd {
  margin-left: 4px;
  color: inherit;
  font: inherit;
  opacity: 0.6;
}
</style>
