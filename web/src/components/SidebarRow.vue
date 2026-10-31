<script setup lang="ts">
// A live session in the sidebar (SessionRow.qml): state dot, title, todo progress, where it runs
// and for how long. A session that needs the user (permission, error) is drawn apart.
import { computed, nextTick, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNow } from '../composables/useNow.ts'
import type { SessionInfo } from '../proto/generated'
import {
  dirName,
  liveLabel,
  needsAttention,
  rowState,
  runningTime,
  todoProgress,
} from '../shell/sessionRow.ts'
import AlertBadge from './AlertBadge.vue'
import ContextMenu from './ContextMenu.vue'
import type { MenuItem } from './ContextMenu.vue'
import Flourish from './Flourish.vue'
import Icon from './Icon.vue'
import SessionDot from './SessionDot.vue'

const props = defineProps<{ info: SessionInfo; current: boolean; tile: boolean }>()
const emit = defineEmits<{ open: []; close: []; rename: [name: string] }>()

const BADGE_SIZE = 14

const { t } = useI18n()
const now = useNow()

const state = computed(() => rowState(props.info))
const attention = computed(() => needsAttention(state.value))
const progress = computed(() => todoProgress(props.info.todos))
const title = computed(() => liveLabel(props.info))

// Line 2: what the session is doing, or else where it is and for how long.
const note = computed(() => {
  switch (state.value) {
    case 'compacting':
      return { text: t('sidebar.compacting'), tone: 'normal' }
    case 'retrying':
      return { text: t('sidebar.retrying'), tone: 'warning' }
    case 'permission':
      return { text: t('sidebar.needsPermission'), tone: 'warning' }
    case 'error':
      return { text: props.info.last_error ?? '', tone: 'danger' }
    default:
      return undefined
  }
})
const location = computed(() => {
  const elapsed = runningTime(props.info, now.value)
  return elapsed ? `${dirName(props.info.cwd)} · ${elapsed}` : dirName(props.info.cwd)
})

// --- The mark that appears when attention is needed springs in; a second error springs it again.
const mark = useTemplateRef<HTMLElement>('mark')
const flourish = useTemplateRef<InstanceType<typeof Flourish>>('flourish')
watch(
  () => [state.value, props.info.last_error],
  async ([next], [previous]) => {
    if (!needsAttention(next as typeof state.value)) return
    const entering = next !== previous
    await nextTick()
    if (!mark.value) return
    if (entering) flourish.value?.enter(mark.value)
    else flourish.value?.play(mark.value)
  },
)

// --- Renaming in place.
const editing = ref(false)
const draft = ref('')
const field = useTemplateRef<HTMLInputElement>('field')

async function startRename(): Promise<void> {
  draft.value = props.info.name
  editing.value = true
  await nextTick()
  field.value?.select()
}

function commitRename(): void {
  if (!editing.value) return
  editing.value = false
  const name = draft.value.trim()
  if (name !== '' && name !== props.info.name) emit('rename', name)
}

function cancelRename(): void {
  editing.value = false
}

// --- Right click.
const menu = ref<{ x: number; y: number }>()
const menuItems = computed<MenuItem[]>(() => [
  { label: t('sidebar.rename'), run: () => void startRename() },
  { label: t('sidebar.close'), danger: true, run: () => emit('close') },
])

function openMenu(event: MouseEvent): void {
  menu.value = { x: event.clientX, y: event.clientY }
}
</script>

<template>
  <div
    class="row"
    :class="{ current, attention, [state]: true, tile }"
    role="button"
    tabindex="0"
    @click="editing || emit('open')"
    @keydown.enter.self="emit('open')"
    @contextmenu.prevent="openMenu"
  >
    <span v-if="tile" class="tile-bar" />
    <SessionDot class="state" :state="state" />
    <input
      v-if="editing"
      ref="field"
      v-model="draft"
      class="title edit"
      @click.stop
      @keydown.enter.prevent="commitRename"
      @keydown.esc.prevent.stop="cancelRename"
      @blur="commitRename"
    />
    <span v-else class="title" @dblclick.stop="startRename">{{ title }}</span>
    <span class="aside">
      <span class="status">
        <span v-if="attention" ref="mark" class="mark">
          <Flourish ref="flourish" :ring-reach="2.5" />
          <Icon v-if="state === 'permission'" icon="lock-16" class="lock" />
          <AlertBadge v-else :size="BADGE_SIZE" />
        </span>
        <span v-else-if="progress" class="progress" :class="{ complete: progress.complete }">
          <Icon v-if="progress.complete" icon="check-16" />
          {{ progress.done }}/{{ progress.total }}
        </span>
      </span>
      <button type="button" class="close" :aria-label="t('sidebar.close')" @click.stop="emit('close')">
        <Icon icon="close-16" />
      </button>
    </span>
    <span class="subtitle" :class="note?.tone">{{ note ? note.text : location }}</span>
    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menuItems" @close="menu = undefined" />
  </div>
</template>

<style scoped>
.row {
  position: relative;
  display: grid;
  grid-template-columns: 8px minmax(0, 1fr) auto;
  grid-template-rows: auto auto;
  column-gap: 10px;
  align-items: center;
  align-content: center;
  height: var(--sidebar-row-height);
  padding: 0 12px;
  border-radius: var(--radius);
  box-shadow: inset 0 0 0 1px transparent;
  cursor: pointer;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    box-shadow var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.row:hover {
  background: var(--surface-hover);
}

.row:active {
  scale: 0.98;
}

.row.current {
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
}

.row.attention.permission {
  background: var(--warning-surface);
  box-shadow: inset 0 0 0 1px var(--warning-border);
}

.row.attention.error {
  background: var(--error-surface);
  box-shadow: inset 0 0 0 1px var(--error-border);
}

.tile-bar {
  position: absolute;
  left: 0;
  top: 8px;
  bottom: 8px;
  width: 2px;
  border-radius: 1px;
  background: var(--accent);
}

.state {
  grid-row: 1;
  grid-column: 1;
}

.title {
  grid-row: 1;
  grid-column: 2;
  min-width: 0;
  overflow: hidden;
  font-size: calc(var(--font-size) - 2px);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.edit {
  padding: 0;
  border: 0;
  outline: 0;
  border-radius: var(--radius-small);
  background: var(--background);
  box-shadow: 0 0 0 2px var(--accent);
  color: var(--text);
  font: inherit;
  font-size: calc(var(--font-size) - 2px);
}

.aside {
  position: relative;
  grid-row: 1;
  grid-column: 3;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  min-width: 22px;
  height: 22px;
}

.status {
  display: flex;
  align-items: center;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.progress {
  display: flex;
  align-items: center;
  gap: 2px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.progress.complete {
  color: var(--success);
}

.mark {
  position: relative;
  display: flex;
}

.lock {
  color: var(--warning);
}

.close {
  position: absolute;
  right: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  opacity: 0;
  pointer-events: none;
  transition:
    opacity var(--motion-quick) var(--motion-ease),
    background-color var(--motion-quick) var(--motion-ease);
}

.close:hover {
  background: var(--surface);
  color: var(--text);
}

.row:hover .close,
.row:focus-within .close {
  opacity: 1;
  pointer-events: auto;
}

.row:hover .status,
.row:focus-within .status {
  opacity: 0;
}

.subtitle {
  grid-row: 2;
  grid-column: 2 / 4;
  min-width: 0;
  overflow: hidden;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.subtitle.warning {
  color: var(--warning);
}

.subtitle.danger {
  color: var(--danger);
}
</style>
