<script setup lang="ts">
// One session in the split view (SplitView.qml's tile): a header with its state and a zoom button,
// and the session itself below. Zoomed, the header folds away and the frame fills the area.
import { computed, nextTick, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNow } from '../composables/useNow.ts'
import type { ChatSession } from '../model/chatSession.ts'
import type { SessionInfo } from '../proto/generated'
import {
  creatorLabel,
  liveLabel,
  needsAttention,
  rootTaskOf,
  rowState,
  runningTime,
  todoProgress,
} from '../shell/sessionRow.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { paletteColorFor } from '../theme/agentColor.ts'
import AlertBadge from './AlertBadge.vue'
import ContextMenu from './ContextMenu.vue'
import type { MenuItem } from './ContextMenu.vue'
import Icon from './Icon.vue'
import SessionDot from './SessionDot.vue'
import SessionPane from './SessionPane.vue'
const BADGE_SIZE = 14
const PRESS_TARGETS = 'button, a, summary'
const NO_DESELECT_TARGETS = `${PRESS_TARGETS}, input, textarea, [role="button"], .composer-area`
const props = defineProps<{
  session: ChatSession
  focused: boolean
  zoomed: boolean
  hidden: boolean
  clearance: { left: number; right: number } // header room for the floating window buttons
}>()
const emit = defineEmits<{ focus: []; unfocus: []; zoom: [] }>()

const { t } = useI18n()
const workspace = useWorkspaceStore()
const now = useNow()
// A press on a button focuses the tile only once the click is through: the composer unfolding
// would move the button from under the pointer, and the click would be lost.
// The press itself moves the keyboard focus off the box, so a tile that took the focus with this
// click puts the caret into its box again once the click is through.
const pane = useTemplateRef<InstanceType<typeof SessionPane>>('pane')
let focusedByPress = false

function onPointerDown(event: PointerEvent): void {
  focusedByPress = !props.focused
  if (event.target instanceof Element && event.target.closest(PRESS_TARGETS)) return
  if (!props.focused) emit('focus')
}

async function onClick(event: MouseEvent): Promise<void> {
  if (focusedByPress) {
    focusedByPress = false
    emit('focus')
    await nextTick()
    pane.value?.focusComposer()
    return
  }
  if (props.zoomed) return
  const target = event.target instanceof Element ? event.target : null
  if (target?.closest(NO_DESELECT_TARGETS)) return
  if (window.getSelection()?.toString()) return
  emit('unfocus')
}

const info = computed(() => props.session.info)
const state = computed(() => (info.value ? rowState(info.value) : 'idle'))
const title = computed(() => (info.value ? liveLabel(info.value) : props.session.id))
const progress = computed(() => todoProgress(info.value?.todos))
const elapsed = computed(() => (info.value ? runningTime(info.value, now.value) : undefined))
const rootTask = computed(() => (info.value ? rootTaskOf(workspace.live, info.value) : undefined))
const rootTint = computed(() => (rootTask.value ? paletteColorFor(rootTask.value.id) : undefined))
const liveById = computed(() => new Map<string, SessionInfo>(workspace.live.map((s) => [s.id, s])))
const headerMenuOpen = ref(false)
const headerMenuMounted = ref(false)
const headerMenuCoords = ref({ x: 0, y: 0 })
const headerMenuItems = computed<MenuItem[]>(() => {
  const items: MenuItem[] = []
  if (info.value?.parent) {
    const parentId = info.value.parent
    items.push({
      label: t('split.filterByCreator', { name: creatorLabel(liveById.value, parentId) }),
      checked: workspace.splitFilter === parentId,
      run: () => workspace.setSplitFilter(parentId),
    })
  }
  items.push({
    label: t('split.filterSpawnedBy', { name: title.value }),
    checked: workspace.splitFilter === props.session.id,
    run: () => workspace.setSplitFilter(props.session.id),
  })
  if (workspace.splitFilter !== 'all') {
    items.push({
      label: t('split.filterAll'),
      separatorBefore: true,
      run: () => workspace.setSplitFilter('all'),
    })
  }
  return items
})

function openHeaderMenu(event: MouseEvent): void {
  headerMenuCoords.value = { x: event.clientX, y: event.clientY }
  headerMenuMounted.value = true
  headerMenuOpen.value = true
}
</script>
<template>
  <div
    class="tile"
    :class="{ focused, zoomed, hidden, tinted: !!rootTint }"
    :style="rootTint ? { '--root-tint': rootTint } : undefined"
    :data-tile="session.id"
    @pointerdown.capture="onPointerDown"
    @click="onClick"
  >
    <header
      class="header"
      :class="{ permission: state === 'permission' || state === 'question', error: state === 'error' }"
      :style="{ '--clear-left': `${clearance.left}px`, '--clear-right': `${clearance.right}px` }"
      :inert="zoomed || undefined"
      @dblclick="emit('zoom')"
      @contextmenu.prevent="openHeaderMenu"
    >
      <SessionDot :state="state" :tint="!focused ? rootTint : undefined" />
      <span class="title">{{ title }}</span>
      <span v-if="rootTask" class="root-chip" :title="rootTask.name">⟲ {{ rootTask.name }}</span>
      <span v-if="needsAttention(state)" class="mark">
        <Icon v-if="state === 'permission'" icon="lock-16" class="lock" />
        <Icon v-else-if="state === 'question'" icon="question-16" class="lock" />
        <AlertBadge v-else :size="BADGE_SIZE" />
      </span>
      <span v-else-if="progress" class="progress" :class="{ complete: progress.complete }">
        <Icon v-if="progress.complete" icon="check-16" />{{ progress.done }}/{{ progress.total }}
      </span>
      <span v-if="elapsed" class="elapsed">{{ elapsed }}</span>
      <button
        type="button"
        class="zoom"
        :aria-label="zoomed ? t('split.unzoom') : t('split.zoom')"
        :title="zoomed ? t('split.unzoom') : t('split.zoom')"
        @click="emit('zoom')"
      >
        <Icon icon="maximize-20" />
      </button>
    </header>
    <SessionPane ref="pane" class="body" :session="session" :composer-shown="focused || zoomed" />
    <ContextMenu
      v-if="headerMenuMounted"
      :open="headerMenuOpen"
      :x="headerMenuCoords.x"
      :y="headerMenuCoords.y"
      :items="headerMenuItems"
      @close="headerMenuOpen = false"
    />
  </div>
</template>

<style scoped>
.tile {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  transition:
    left var(--geometry-duration) var(--motion-ease),
    top var(--geometry-duration) var(--motion-ease),
    width var(--geometry-duration) var(--motion-ease),
    height var(--geometry-duration) var(--motion-ease),
    box-shadow var(--motion-standard) var(--motion-ease),
    border-radius var(--geometry-duration) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease);
}

.tile:not(.zoomed) {
  --page-padding: var(--tile-padding);
}

.tile.tinted:not(.focused):not(.zoomed) {
  box-shadow: inset 0 0 0 1px var(--root-tint);
}

.tile.focused {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.tile.zoomed {
  border-radius: 0;
  box-shadow: none;
}

.tile.hidden {
  opacity: 0;
  scale: var(--motion-tile-exit-scale);
  pointer-events: none;
}

.header {
  display: flex;
  flex: none;
  align-items: center;
  gap: 10px;
  height: var(--tile-header-height);
  padding: 0 calc(8px + var(--clear-right)) 0 calc(12px + var(--clear-left));
  overflow: hidden;
  background: var(--surface);
  box-shadow: inset 0 -1px 0 var(--border);
  transition:
    height var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    background-color var(--motion-standard) var(--motion-ease);
}

.zoomed .header {
  height: 0;
  opacity: 0;
}

.header.permission {
  background: var(--warning-surface);
}

.header.error {
  background: var(--error-surface);
}

.title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: calc(var(--font-size) - 2px);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.root-chip {
  flex: 0 1 auto;
  max-width: 120px;
  overflow: hidden;
  padding: 1px 6px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--root-tint) 12%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--root-tint) 55%, transparent);
  color: var(--text-muted);
  font-size: 11px;
  line-height: 16px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.progress,
.elapsed {
  flex: none;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.progress {
  display: flex;
  align-items: center;
  gap: 2px;
}

.progress.complete {
  color: var(--success-text);
}

.mark {
  display: flex;
  flex: none;
}

.lock {
  color: var(--warning-text);
}

.zoom {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.zoom:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.body {
  flex: 1;
  min-height: 0;
}
</style>
