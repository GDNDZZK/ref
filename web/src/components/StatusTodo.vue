<script setup lang="ts">
// The to-do list in the status line (the TUI's progress line): the item being worked on and
// done/total, or a tick and done/total once everything is done. A click opens the full list in a
// popover above the button; Esc, a click outside and a change of session close it.
import { computed, nextTick, onBeforeUnmount, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { oneLine } from '../model/oneLine.ts'
import type { Todo } from '../proto/generated'
import { placeInPane } from '../shell/popoverPlacement.ts'
import { currentTodo, todoProgress } from '../shell/sessionRow.ts'
import Icon from './Icon.vue'
import TodoList from './TodoList.vue'
import TodoMarker from './TodoMarker.vue'

const props = defineProps<{ sessionId: string; todos: readonly Todo[] | undefined }>()

const POPOVER_GAP = 6 // between the button and the popover
const POPOVER_WIDTH = 360
const EDGE_MARGIN = 8

const { t } = useI18n()
const button = useTemplateRef<HTMLElement>('button')
const popover = useTemplateRef<HTMLElement>('popover')
const open = ref(false)
const position = ref({ left: 0, width: POPOVER_WIDTH, bottom: 0 })

const progress = computed(() => todoProgress(props.todos))
const current = computed(() => currentTodo(props.todos))
const list = computed(() => [...(props.todos ?? [])])
const countText = computed(() => (progress.value ? `${progress.value.done}/${progress.value.total}` : ''))

// Inside the pane the button belongs to (a split tile, or the single pane), else the viewport.
function place(): void {
  const rect = button.value?.getBoundingClientRect()
  if (!rect) return
  const paneRect = button.value?.closest('[data-pane]')?.getBoundingClientRect()
  const pane = paneRect ?? { left: 0, right: window.innerWidth }
  const { left, width } = placeInPane(rect, pane, POPOVER_WIDTH, EDGE_MARGIN)
  const bottom = window.innerHeight - rect.top + POPOVER_GAP
  const current = position.value
  if (left !== current.left || width !== current.width || bottom !== current.bottom) {
    position.value = { left, width, bottom }
  }
}

// The button moves when the layout around it does (a tile gets the focus and unfolds its input
// box, the window is resized), so the popover follows it for as long as it is open.
let followFrame = 0
function follow(): void {
  place()
  followFrame = requestAnimationFrame(follow)
}

function close(): void {
  open.value = false
}

async function toggle(): Promise<void> {
  open.value = !open.value
  if (open.value) {
    place()
    await nextTick()
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation() // not an interrupt, and not for the composer
  close()
}

function onPointerDown(event: PointerEvent): void {
  const target = event.target as Node
  if (!button.value?.contains(target) && !popover.value?.contains(target)) close()
}

// Capture phase: before the composer sees the Esc.
function listen(): void {
  follow()
  window.addEventListener('keydown', onKeydown, true)
  window.addEventListener('pointerdown', onPointerDown, true)
}

function unlisten(): void {
  cancelAnimationFrame(followFrame)
  window.removeEventListener('keydown', onKeydown, true)
  window.removeEventListener('pointerdown', onPointerDown, true)
}

watch(open, (isOpen) => (isOpen ? listen() : unlisten()))
watch(() => props.sessionId, close)
watch(progress, (value) => {
  if (!value) close()
})
onBeforeUnmount(unlisten)
</script>

<template>
  <div v-if="progress" class="todo">
    <button
      ref="button"
      type="button"
      class="summary"
      :class="{ complete: progress.complete }"
      :aria-expanded="open"
      :title="t('todo.show')"
      @click="toggle"
    >
      <template v-if="current">
        <TodoMarker :status="current.status" />
        <span class="current" :class="current.status">{{ oneLine(current.content) }}</span>
        <span class="count">· {{ countText }}</span>
      </template>
      <template v-else>
        <Icon icon="check-16" />
        <span class="count">{{ countText }}</span>
      </template>
    </button>
    <Teleport to="body">
      <div
        v-if="open"
        ref="popover"
        class="todo-popover"
        role="region"
        :aria-label="t('todo.title', { done: progress.done, total: progress.total })"
        :style="{ left: `${position.left}px`, width: `${position.width}px`, bottom: `${position.bottom}px` }"
      >
        <div class="head">{{ t('todo.title', { done: progress.done, total: progress.total }) }}</div>
        <TodoList :todos="list" />
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.todo {
  display: flex;
  flex: 0 1 auto;
  min-width: 0;
  max-width: 50%;
}

.summary {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  height: 24px;
  padding: 0 6px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.summary:hover {
  background: var(--surface-hover);
}

.summary.complete {
  color: var(--success);
}

.current {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.current.in_progress {
  color: var(--text);
}

.count {
  flex: none;
}

.todo-popover {
  position: fixed;
  z-index: 20;
  max-height: 50vh;
  overflow-y: auto;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 6px 24px rgb(0 0 0 / 0.18);
}

.head {
  padding: 10px 14px 6px;
  font-weight: 600;
}

.todo-popover :deep(.todos) {
  border-top: 0;
}
</style>
