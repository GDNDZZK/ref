<script setup lang="ts">
// The sidebar (Sidebar.qml): running sessions, searchable history, MCP status and settings.
// The content keeps its width and is clipped by the frame, so collapsing slides it out to the left.
import { computed, onBeforeUnmount, ref, useTemplateRef, watch } from 'vue'
import type { ComponentPublicInstance } from 'vue'
import { useI18n } from 'vue-i18n'
import lockupDark from '../../../assets/app/ref-lockup-small-dark.svg'
import lockupLight from '../../../assets/app/ref-lockup-small-light.svg'
import { clamp } from '../clamp.ts'
import { useTheme } from '../composables/useTheme.ts'
import {
  creatorKeywords,
  creatorLabel,
  isWorkingSession,
  matchesQuery,
  todoProgress,
} from '../shell/sessionRow.ts'
import type { SessionInfo } from '../proto/generated'
import { metrics } from '../theme/generated/theme.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useMcpStore } from '../stores/mcp.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import Chevron from './Chevron.vue'
import ContextMenu from './ContextMenu.vue'
import type { MenuItem } from './ContextMenu.vue'
import HistoryRow from './HistoryRow.vue'
import Icon from './Icon.vue'
import McpDots from './McpDots.vue'
import SidebarRow from './SidebarRow.vue'
import SidebarTodo from './SidebarTodo.vue'
const LOCKUP_WIDTH = 60
const LOCKUP_HEIGHT = 24
const RUNNING_ROW_GAP = 2
const AUTO_SCROLL_EDGE_PX = 24
const AUTO_SCROLL_MAX_STEP_PX = 8
const SECTION_RESIZER_HEIGHT = 6
const SEARCH_HEIGHT = 32
const SEARCH_GAP = 4
const TODO_MIN_BODY_HEIGHT = 38 // 4px section margin + 2px list padding + 6px ul padding + 26px todo row
const MIN_TODO_HEIGHT = metrics.sidebarSectionHeight + TODO_MIN_BODY_HEIGHT
const MIN_RUNNING_HEIGHT = metrics.sidebarRowHeight
const MIN_HISTORY_HEIGHT = SEARCH_HEIGHT + SEARCH_GAP + metrics.sidebarHistoryRowHeight
const { t } = useI18n()
const { isDark } = useTheme()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const connection = useConnectionStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const mcp = useMcpStore()
const notices = useNoticesStore()

const HISTORY_SEARCH_DEBOUNCE_MS = 150
const query = computed({
  get: () => workspace.historyQuery,
  set: (value: string) => (workspace.historyQuery = value),
})

// The session on screen: the open one, or the focused tile in split view.
const todos = computed(() => (workspace.focusedId ? workspace.infoOf(workspace.focusedId)?.todos : undefined))
const hasTodo = computed(() => todoProgress(todos.value) !== undefined)
const liveIds = computed(() => new Set(workspace.live.map((session) => session.id)))
const sessionById = computed(() => {
  const map = new Map<string, SessionInfo>()
  for (const info of workspace.history) map.set(info.id, info)
  for (const info of workspace.sidebarHistory) map.set(info.id, info)
  for (const info of workspace.live) map.set(info.id, info)
  return map
})
const hasActiveHistoryFilter = computed(
  () =>
    Boolean(workspace.historyCreator) ||
    layout.sidebarHistoryCreatorFilter !== 'all' ||
    layout.sidebarHistoryCurrentDir,
)
const history = computed(() =>
  workspace.sidebarHistory.filter((session) => {
    if (liveIds.value.has(session.id)) return false
    if (workspace.historyCreator) {
      if (session.parent !== workspace.historyCreator) return false
    } else if (layout.sidebarHistoryCreatorFilter === 'user' && session.parent) {
      return false
    }
    if (layout.sidebarHistoryCurrentDir && workspace.effectiveCwd && session.cwd !== workspace.effectiveCwd) {
      return false
    }
    if (workspace.sidebarHistorySynced) return true
    const creator = creatorKeywords(sessionById.value, session)
    return matchesQuery(session, workspace.historyQuery, creator)
  }),
)

let searchTimer: ReturnType<typeof setTimeout> | undefined
function clearSearchTimer(): void {
  if (searchTimer === undefined) return
  clearTimeout(searchTimer)
  searchTimer = undefined
}
function refreshHistoryNow(): void {
  clearSearchTimer()
  report(workspace.refreshSidebarHistory())
}
watch(
  () => workspace.historyQuery,
  (next) => {
    clearSearchTimer()
    if (next.trim() === '') {
      report(workspace.refreshSidebarHistory())
      return
    }
    searchTimer = setTimeout(() => {
      searchTimer = undefined
      report(workspace.refreshSidebarHistory())
    }, HISTORY_SEARCH_DEBOUNCE_MS)
  },
)
const filterButton = useTemplateRef<HTMLButtonElement>('filterButton')
const filterMenuOpen = ref(false)
const filterMenuMounted = ref(false)
const filterMenuCoords = ref({ x: 0, y: 0 })
let suppressFilterClick = false

function selectCreatorFilter(choice: 'all' | 'user'): void {
  workspace.historyCreator = undefined
  layout.setSidebarHistoryCreatorFilter(choice)
  refreshHistoryNow()
}
function toggleCurrentDirFilter(): void {
  layout.toggleSidebarHistoryCurrentDir()
  refreshHistoryNow()
}
function filterHistoryByCreator(creatorId: string): void {
  workspace.historyCreator = creatorId
  if (!layout.sidebarHistoryExpanded) layout.toggleSidebarHistory()
  refreshHistoryNow()
}
const filterMenuItems = computed<MenuItem[]>(() => {
  const ephemeralId = workspace.historyCreator
  const creatorFilter = layout.sidebarHistoryCreatorFilter
  return [
    {
      label: t('dialog.sessions.filterAll'),
      checked: !ephemeralId && creatorFilter === 'all',
      run: () => selectCreatorFilter('all'),
    },
    {
      label: t('dialog.sessions.filterUser'),
      checked: !ephemeralId && creatorFilter === 'user',
      run: () => selectCreatorFilter('user'),
    },
    ...(ephemeralId
      ? [
          {
            label: t('dialog.sessions.filterByName', { name: creatorLabel(sessionById.value, ephemeralId) }),
            checked: true,
            run: () => {
              filterMenuOpen.value = false
            },
          },
        ]
      : []),
    {
      label: t('sidebar.onlyCurrentDir'),
      checked: layout.sidebarHistoryCurrentDir,
      separatorBefore: true,
      run: toggleCurrentDirFilter,
    },
  ]
})
function onFilterButtonPointerDown(): void {
  suppressFilterClick = filterMenuOpen.value
}
function toggleFilterMenu(): void {
  if (suppressFilterClick) {
    suppressFilterClick = false
    return
  }
  if (filterMenuOpen.value) {
    filterMenuOpen.value = false
    return
  }
  const rect = filterButton.value?.getBoundingClientRect()
  filterMenuCoords.value = { x: rect ? rect.left : 0, y: rect ? rect.bottom + SEARCH_GAP : 0 }
  filterMenuMounted.value = true
  filterMenuOpen.value = true
}

function report(promise: Promise<unknown>): void {
  promise.catch(notices.pushError)
}

function onStashButtonClick(): void {
  if (workspace.live.length > 0) {
    report(workspace.stashAll())
    return
  }
  dialogs.open({ kind: 'stash' })
}

function openStashDialog(): void {
  dialogs.open({ kind: 'stash' })
}
function open(id: string): void {
  layout.closeDrawer()
  if (workspace.mode === 'single' && id === workspace.currentId) {
    report(workspace.detachCurrent())
    return
  }
  if (workspace.mode === 'split' && workspace.tileIds.includes(id)) workspace.focusTile(id)
  else report(workspace.openSession(id))
}

function close(id: string): void {
  if (isWorkingSession(workspace.infoOf(id))) dialogs.open({ kind: 'closeSession', sessionId: id })
  else report(workspace.closeSession(id))
}

function reloadConfig(): void {
  if (commands.find('reload')) {
    report(commands.run('reload'))
    return
  }
  report(
    connection.reloadConfig(workspace.effectiveCwd).then(() => {
      notices.push('info', t('command.reload.done'))
    }),
  )
}

const sidebarStyle = computed(() =>
  layout.narrow ? undefined : { '--sidebar-width': `${layout.sidebarWidth}px` },
)

const showResizeHandle = computed(() => !layout.narrow && layout.sidebarVisible)
const resizing = ref(false)
let dragPointerId: number | undefined
let dragStartX = 0
let dragStartWidth = 0

function resetDragState(): void {
  resizing.value = false
  dragPointerId = undefined
}

watch(showResizeHandle, (shown) => {
  if (!shown) resetDragState()
})

function startResize(event: PointerEvent): void {
  if (event.button !== 0) return
  event.preventDefault()
  const handle = event.currentTarget as HTMLElement | null
  dragPointerId = event.pointerId
  dragStartX = event.clientX
  dragStartWidth = layout.sidebarWidth
  resizing.value = true
  handle?.setPointerCapture?.(event.pointerId)
}

function moveResize(event: PointerEvent): void {
  if (!resizing.value || event.pointerId !== dragPointerId) return
  layout.setSidebarWidth(dragStartWidth + (event.clientX - dragStartX))
}

function stopResize(event: PointerEvent): void {
  if (!resizing.value || event.pointerId !== dragPointerId) return
  const handle = event.currentTarget as HTMLElement | null
  if (handle?.hasPointerCapture?.(event.pointerId)) {
    handle.releasePointerCapture(event.pointerId)
  }
  resetDragState()
}

// --- Vertical section splitters (Todo <-> Running, Running <-> History).
type SectionDragKind = 'todo' | 'running'
const bodyEl = useTemplateRef<HTMLElement>('body')
const listsEl = useTemplateRef<HTMLElement>('lists')
const runningSlotEl = useTemplateRef<HTMLElement>('runningSlot')
const activeSectionDrag = ref<SectionDragKind>()
let sectionDragPointerId: number | undefined
let sectionDragStartY = 0
let sectionDragStartHeight = 0

const showTodoResizer = computed(
  () =>
    hasTodo.value &&
    layout.sidebarTodoExpanded &&
    (layout.sidebarRunningExpanded || layout.sidebarHistoryExpanded),
)
const showRunningResizer = computed(() => layout.sidebarRunningExpanded && layout.sidebarHistoryExpanded)
const listsCollapsed = computed(() => !layout.sidebarRunningExpanded && !layout.sidebarHistoryExpanded)
const runningCustomActive = computed(() => layout.sidebarRunningHeight > 0)
const runningSlotStyle = computed(() =>
  runningCustomActive.value ? { '--running-height': `${layout.sidebarRunningHeight}px` } : undefined,
)
const todoMaxHeightCss = computed(() => {
  const reserved = [
    '2 * var(--sidebar-section-height)',
    ...(showTodoResizer.value ? ['var(--section-resizer-height)'] : []),
    ...(layout.sidebarRunningExpanded ? ['var(--sidebar-row-height)'] : []),
    ...(showRunningResizer.value ? ['var(--section-resizer-height)'] : []),
    ...(layout.sidebarHistoryExpanded
      ? ['var(--search-height)', 'var(--search-gap)', 'var(--sidebar-history-row-height)']
      : []),
  ]
  return `max(${MIN_TODO_HEIGHT}px, calc(100% - ${reserved.join(' - ')}))`
})

function resetSectionDragState(): void {
  activeSectionDrag.value = undefined
  sectionDragPointerId = undefined
}

watch(
  [showTodoResizer, showRunningResizer, () => layout.sidebarVisible],
  ([todoShown, runningShown, visible]) => {
    if (!visible) {
      resetSectionDragState()
      return
    }
    if (activeSectionDrag.value === 'todo' && !todoShown) resetSectionDragState()
    if (activeSectionDrag.value === 'running' && !runningShown) resetSectionDragState()
  },
)

function tryCapturePointer(target: HTMLElement | null, pointerId: number): void {
  if (!target?.setPointerCapture) return
  try {
    target.setPointerCapture(pointerId)
  } catch (error) {
    if (!(error instanceof DOMException && error.name === 'NotFoundError')) throw error
  }
}

function startSectionResize(kind: SectionDragKind, event: PointerEvent): void {
  if (event.button !== 0) return
  event.preventDefault()
  const sectionEl =
    kind === 'todo' ? bodyEl.value?.querySelector<HTMLElement>('.todo-slot') : runningSlotEl.value
  const measured = sectionEl?.getBoundingClientRect().height ?? 0
  const fallback =
    kind === 'todo'
      ? layout.sidebarTodoHeight || MIN_TODO_HEIGHT
      : layout.sidebarRunningHeight || MIN_RUNNING_HEIGHT
  activeSectionDrag.value = kind
  sectionDragPointerId = event.pointerId
  sectionDragStartY = event.clientY
  sectionDragStartHeight = measured > 0 ? measured : fallback
  tryCapturePointer(event.currentTarget as HTMLElement | null, event.pointerId)
}

function moveSectionResize(event: PointerEvent): void {
  if (!activeSectionDrag.value || event.pointerId !== sectionDragPointerId) return
  const deltaY = event.clientY - sectionDragStartY
  if (activeSectionDrag.value === 'todo') {
    const bodyHeight = bodyEl.value?.getBoundingClientRect().height ?? 0
    const reservedBelow =
      2 * metrics.sidebarSectionHeight +
      SECTION_RESIZER_HEIGHT +
      (layout.sidebarRunningExpanded ? MIN_RUNNING_HEIGHT : 0) +
      (showRunningResizer.value ? SECTION_RESIZER_HEIGHT : 0) +
      (layout.sidebarHistoryExpanded ? MIN_HISTORY_HEIGHT : 0)
    const maxTodo =
      bodyHeight > 0
        ? Math.max(MIN_TODO_HEIGHT, Math.floor(bodyHeight - reservedBelow))
        : Number.POSITIVE_INFINITY
    layout.setSidebarTodoHeight(clamp(Math.round(sectionDragStartHeight + deltaY), MIN_TODO_HEIGHT, maxTodo))
    return
  }
  const listsHeight = listsEl.value?.getBoundingClientRect().height ?? 0
  const reservedInLists = 2 * metrics.sidebarSectionHeight + SECTION_RESIZER_HEIGHT + MIN_HISTORY_HEIGHT
  const maxRunning =
    listsHeight > 0
      ? Math.max(MIN_RUNNING_HEIGHT, Math.floor(listsHeight - reservedInLists))
      : Number.POSITIVE_INFINITY
  layout.setSidebarRunningHeight(
    clamp(Math.round(sectionDragStartHeight + deltaY), MIN_RUNNING_HEIGHT, maxRunning),
  )
}

function stopSectionResize(event: PointerEvent): void {
  if (!activeSectionDrag.value || event.pointerId !== sectionDragPointerId) return
  const handle = event.currentTarget as HTMLElement | null
  if (handle?.hasPointerCapture?.(event.pointerId)) {
    handle.releasePointerCapture(event.pointerId)
  }
  resetSectionDragState()
}
const runningRef = useTemplateRef<ComponentPublicInstance>('running')
const draggingId = ref<string>()
const dragOffset = ref(0)
let dragStartClientY = 0
let dragLastClientY = 0
let dragStartScrollTop = 0
let dragStartIndex = -1
let dragStride = metrics.sidebarRowHeight + RUNNING_ROW_GAP
let scrollFrameId: number | undefined

function runningListElement(): HTMLElement | undefined {
  const el = runningRef.value?.$el
  return el instanceof HTMLElement ? el : undefined
}

function stopAutoScroll(): void {
  if (scrollFrameId === undefined) return
  cancelAnimationFrame(scrollFrameId)
  scrollFrameId = undefined
}

function measureRowStride(): number {
  const fallback = metrics.sidebarRowHeight + RUNNING_ROW_GAP
  const list = runningListElement()
  if (!list) return fallback
  const rows = list.querySelectorAll<HTMLElement>(':scope > .row')
  const first = rows[0]
  if (!first) return fallback
  const firstRect = first.getBoundingClientRect()
  if (firstRect.height <= 0) return fallback
  const second = rows[1]
  if (second) {
    const stride = second.getBoundingClientRect().top - firstRect.top
    if (stride > 0) return stride
  }
  const gap = Number.parseFloat(getComputedStyle(list).rowGap || getComputedStyle(list).gap)
  return firstRect.height + (Number.isFinite(gap) ? gap : RUNNING_ROW_GAP)
}

function startSessionDrag(id: string, clientY: number): void {
  if (workspace.live.length <= 1) return
  const index = workspace.live.findIndex((session) => session.id === id)
  if (index < 0) return
  draggingId.value = id
  dragOffset.value = 0
  dragStartClientY = clientY
  dragLastClientY = clientY
  dragStartScrollTop = runningListElement()?.scrollTop ?? 0
  dragStartIndex = index
  dragStride = measureRowStride()
}

function applyDragPosition(clientY: number): void {
  const id = draggingId.value
  if (!id) return
  const count = workspace.live.length
  if (count <= 1) {
    endSessionDrag()
    return
  }
  const scrollTop = runningListElement()?.scrollTop ?? 0
  const totalDelta = clientY - dragStartClientY + (scrollTop - dragStartScrollTop)
  const minDelta = -dragStartIndex * dragStride
  const maxDelta = (count - 1 - dragStartIndex) * dragStride
  const clampedDelta = clamp(totalDelta, minDelta, maxDelta)
  const targetIndex = clamp(dragStartIndex + Math.round(clampedDelta / dragStride), 0, count - 1)
  workspace.moveRunningSession(id, targetIndex)
  const currentIndex = workspace.live.findIndex((session) => session.id === id)
  if (currentIndex < 0) {
    endSessionDrag()
    return
  }
  dragOffset.value = clampedDelta - (currentIndex - dragStartIndex) * dragStride
}

function autoScrollStep(clientY: number): number {
  const list = runningListElement()
  if (!list || list.scrollHeight <= list.clientHeight) return 0
  const rect = list.getBoundingClientRect()
  if (rect.height <= 0) return 0
  if (clientY < rect.top + AUTO_SCROLL_EDGE_PX && list.scrollTop > 0) {
    const ratio = clamp((rect.top + AUTO_SCROLL_EDGE_PX - clientY) / AUTO_SCROLL_EDGE_PX, 0, 1)
    return -Math.max(1, Math.round(ratio * AUTO_SCROLL_MAX_STEP_PX))
  }
  if (clientY > rect.bottom - AUTO_SCROLL_EDGE_PX && list.scrollTop + list.clientHeight < list.scrollHeight) {
    const ratio = clamp((clientY - (rect.bottom - AUTO_SCROLL_EDGE_PX)) / AUTO_SCROLL_EDGE_PX, 0, 1)
    return Math.max(1, Math.round(ratio * AUTO_SCROLL_MAX_STEP_PX))
  }
  return 0
}

function scheduleAutoScroll(): void {
  if (scrollFrameId !== undefined) return
  const tick = () => {
    scrollFrameId = undefined
    if (!draggingId.value) return
    const step = autoScrollStep(dragLastClientY)
    if (step === 0) return
    const list = runningListElement()
    if (!list) return
    list.scrollTop += step
    applyDragPosition(dragLastClientY)
    if (autoScrollStep(dragLastClientY) !== 0) {
      scrollFrameId = requestAnimationFrame(tick)
    }
  }
  scrollFrameId = requestAnimationFrame(tick)
}

function moveSessionDrag(clientY: number): void {
  if (!draggingId.value) return
  dragLastClientY = clientY
  applyDragPosition(clientY)
  if (autoScrollStep(clientY) !== 0) scheduleAutoScroll()
  else stopAutoScroll()
}

function endSessionDrag(): void {
  stopAutoScroll()
  const list = runningListElement()
  if (list?.querySelector('.row.dragging.session-move')) {
    for (const row of list.querySelectorAll('.row.session-move')) {
      const event =
        typeof TransitionEvent === 'function'
          ? new TransitionEvent('transitionend', { propertyName: 'transform' })
          : new Event('transitionend')
      if (!(event as TransitionEvent).propertyName) {
        Object.defineProperty(event, 'propertyName', { value: 'transform' })
      }
      row.dispatchEvent(event)
    }
  }
  draggingId.value = undefined
  dragOffset.value = 0
  dragStartIndex = -1
}

watch(
  [() => layout.sidebarRunningExpanded, () => layout.sidebarVisible, () => workspace.live.length],
  ([expanded, visible, count]) => {
    if (!draggingId.value) return
    if (!expanded || !visible || count <= 1 || !workspace.live.some((s) => s.id === draggingId.value)) {
      endSessionDrag()
    }
  },
)

onBeforeUnmount(() => {
  clearSearchTimer()
  endSessionDrag()
  resetDragState()
  resetSectionDragState()
})
</script>
<template>
  <aside
    class="sidebar"
    :class="{
      open: layout.sidebarVisible,
      drawer: layout.narrow,
      resizing,
      'resizing-sections': activeSectionDrag !== undefined,
    }"
    :style="sidebarStyle"
  >
    <div class="content" :inert="!layout.sidebarVisible || undefined">
      <header class="header">
        <img
          class="lockup"
          :src="isDark ? lockupDark : lockupLight"
          :width="LOCKUP_WIDTH"
          :height="LOCKUP_HEIGHT"
          alt="ref"
        />
        <button
          type="button"
          class="icon-button"
          :aria-label="t('sidebar.hide')"
          :title="t('sidebar.hide')"
          @click="layout.toggleSidebar()"
        >
          <Icon icon="sidebar-20" />
        </button>
      </header>

      <div class="new-row">
        <button
          type="button"
          class="stash-btn"
          :aria-label="workspace.live.length > 0 ? t('sidebar.stashAll') : t('sidebar.stashed')"
          :title="workspace.live.length > 0 ? t('sidebar.stashAllHint') : t('sidebar.stashed')"
          @click="onStashButtonClick"
          @contextmenu.prevent="openStashDialog"
        >
          <Icon :icon="workspace.groups.length > 0 ? 'stash-count-20' : 'stash-20'" />
          <span v-if="workspace.groups.length > 0" class="stash-count">{{ workspace.groups.length }}</span>
        </button>
        <button type="button" class="new" @click="report(commands.run('new'))">
          <Icon icon="plus-20" />
          {{ t('sidebar.newSession') }}
        </button>
        <button
          type="button"
          class="folder"
          :aria-label="t('sidebar.newSessionIn')"
          :title="t('sidebar.newSessionIn')"
          @click="dialogs.open({ kind: 'newSession' })"
        >
          <Icon icon="folder-open-20" />
        </button>
      </div>

      <div ref="body" class="body" :class="{ 'lists-collapsed': listsCollapsed }">
        <SidebarTodo
          :todos="todos"
          :expanded="layout.sidebarTodoExpanded"
          :height="layout.sidebarTodoHeight"
          :max-height="todoMaxHeightCss"
          :fill="listsCollapsed"
          @toggle="layout.toggleSidebarTodo()"
        />
        <div
          v-if="showTodoResizer"
          class="section-resizer"
          :class="{ active: activeSectionDrag === 'todo' }"
          role="separator"
          aria-orientation="horizontal"
          :aria-label="t('sidebar.resizeTodo')"
          @pointerdown="startSectionResize('todo', $event)"
          @pointermove="moveSectionResize"
          @pointerup="stopSectionResize"
          @pointercancel="stopSectionResize"
          @lostpointercapture="stopSectionResize"
          @dblclick="layout.resetSidebarTodoHeight()"
        />
        <div ref="lists" class="lists" :class="{ 'history-collapsed': !layout.sidebarHistoryExpanded }">
          <h2 class="section">
            <button
              type="button"
              class="section-toggle"
              :aria-expanded="layout.sidebarRunningExpanded"
              @click="layout.toggleSidebarRunning()"
            >
              <span class="section-label">{{ t('sidebar.running') }}</span>
              <Chevron class="section-chevron" :class="{ open: layout.sidebarRunningExpanded }" />
            </button>
          </h2>
          <div
            ref="runningSlot"
            class="running-slot"
            :class="{ open: layout.sidebarRunningExpanded, 'custom-height': runningCustomActive }"
            :style="runningSlotStyle"
            :inert="!layout.sidebarRunningExpanded || undefined"
          >
            <div class="fold">
              <TransitionGroup ref="running" tag="div" name="session" class="running" role="list">
                <SidebarRow
                  v-for="info in workspace.live"
                  :key="info.id"
                  :info="info"
                  :current="info.id === workspace.focusedId"
                  :tile="workspace.mode === 'split' && workspace.tileIds.includes(info.id)"
                  :can-detach="workspace.mode === 'single' && info.id === workspace.currentId"
                  :reorderable="workspace.live.length > 1"
                  :dragging="draggingId === info.id"
                  :drag-offset="draggingId === info.id ? dragOffset : 0"
                  @open="open(info.id)"
                  @detach="report(workspace.detachCurrent())"
                  @close="close(info.id)"
                  @rename="(name) => report(workspace.renameSession(info.id, name))"
                  @drag-start="(y) => startSessionDrag(info.id, y)"
                  @drag-move="moveSessionDrag"
                  @drag-end="endSessionDrag"
                  @filter-spawned="filterHistoryByCreator(info.id)"
                />
              </TransitionGroup>
            </div>
          </div>
          <div
            v-if="showRunningResizer"
            class="section-resizer"
            :class="{ active: activeSectionDrag === 'running' }"
            role="separator"
            aria-orientation="horizontal"
            :aria-label="t('sidebar.resizeRunning')"
            @pointerdown="startSectionResize('running', $event)"
            @pointermove="moveSectionResize"
            @pointerup="stopSectionResize"
            @pointercancel="stopSectionResize"
            @lostpointercapture="stopSectionResize"
            @dblclick="layout.resetSidebarRunningHeight()"
          />
          <h2 class="section">
            <button
              type="button"
              class="section-toggle"
              :aria-expanded="layout.sidebarHistoryExpanded"
              @click="layout.toggleSidebarHistory()"
            >
              <span class="section-label">{{ t('sidebar.history') }}</span>
              <Chevron class="section-chevron" :class="{ open: layout.sidebarHistoryExpanded }" />
            </button>
          </h2>
          <div
            class="history-slot"
            :class="{ open: layout.sidebarHistoryExpanded }"
            :inert="!layout.sidebarHistoryExpanded || undefined"
          >
            <div class="fold">
              <div class="search">
                <Icon icon="search-16" class="search-icon" />
                <input
                  v-model="query"
                  type="text"
                  :aria-label="t('sidebar.searchHistory')"
                  :placeholder="t('sidebar.searchHistory')"
                />
                <button
                  v-if="query"
                  type="button"
                  class="clear"
                  :aria-label="t('sidebar.clearSearch')"
                  @click="query = ''"
                >
                  <Icon icon="close-16" />
                </button>
                <button
                  ref="filterButton"
                  type="button"
                  class="filter-button"
                  :class="{ active: hasActiveHistoryFilter }"
                  :aria-label="t('sidebar.filterHistory')"
                  :title="t('sidebar.filterHistory')"
                  :aria-expanded="filterMenuOpen"
                  aria-haspopup="menu"
                  @pointerdown="onFilterButtonPointerDown"
                  @click="toggleFilterMenu"
                >
                  <Icon icon="filter-16" />
                </button>
                <ContextMenu
                  v-if="filterMenuMounted"
                  :open="filterMenuOpen"
                  :x="filterMenuCoords.x"
                  :y="filterMenuCoords.y"
                  :items="filterMenuItems"
                  @close="filterMenuOpen = false"
                />
              </div>
              <div class="history" role="list">
                <HistoryRow
                  v-for="info in history"
                  :key="info.id"
                  :info="info"
                  :creator-name="info.parent ? creatorLabel(sessionById, info.parent) : undefined"
                  @open="open(info.id)"
                  @remove="dialogs.open({ kind: 'deleteHistory', sessionId: info.id })"
                  @filter-by-creator="filterHistoryByCreator"
                />
                <p v-if="!workspace.sidebarHistoryLoaded" class="empty">{{ t('sidebar.loading') }}</p>
              </div>
            </div>
          </div>
        </div>
      </div>

      <footer class="footer">
        <button
          v-if="mcp.servers.length > 0"
          type="button"
          class="mcp"
          :aria-label="t('command.mcp.title')"
          @click="report(commands.run('mcp'))"
        >
          <Icon icon="mcp-16" />
          {{ t('sidebar.mcp') }}
          <McpDots :servers="mcp.servers" />
        </button>
        <span v-else class="mcp-space" />
        <div class="footer-actions">
          <button
            type="button"
            class="icon-button"
            :aria-label="t('sidebar.config')"
            :title="t('sidebar.config')"
            @click="dialogs.open({ kind: 'config' })"
          >
            <Icon icon="file-text-16" />
          </button>
          <button
            type="button"
            class="icon-button"
            :aria-label="t('sidebar.reloadConfig')"
            :title="t('sidebar.reloadConfig')"
            @click="reloadConfig"
          >
            <Icon icon="retry-16" />
          </button>
          <button
            type="button"
            class="icon-button"
            :aria-label="t('sidebar.settings')"
            :title="t('sidebar.settings')"
            @click="report(commands.run('settings'))"
          >
            <Icon icon="sliders-20" />
          </button>
        </div>
      </footer>
    </div>
    <div
      v-if="showResizeHandle"
      class="resize-handle"
      role="separator"
      aria-orientation="vertical"
      :aria-label="t('sidebar.resize')"
      @pointerdown="startResize"
      @pointermove="moveResize"
      @pointerup="stopResize"
      @pointercancel="stopResize"
      @lostpointercapture="stopResize"
      @dblclick="layout.resetSidebarWidth()"
    />
  </aside>
</template>

<style scoped>
.sidebar {
  position: relative;
  flex: none;
  width: 0;
  height: 100%;
  overflow: hidden;
  border-right: 1px solid transparent;
  background: var(--surface);
  transition:
    width var(--motion-standard) var(--motion-ease),
    border-color var(--motion-standard) var(--motion-ease);
}

.sidebar.open {
  width: var(--sidebar-width);
  border-right-color: var(--border);
}

.sidebar.resizing {
  transition: none;
  user-select: none;
}

.sidebar.resizing-sections {
  user-select: none;
}

.sidebar.resizing-sections :deep(.todo-slot),
.sidebar.resizing-sections .lists,
.sidebar.resizing-sections .running-slot,
.sidebar.resizing-sections .history-slot {
  transition: none;
}
/* Narrow windows: over the content instead of beside it. */
.sidebar.drawer {
  position: fixed;
  z-index: 15;
  left: 0;
  top: 0;
}

.sidebar.drawer.open {
  width: min(var(--sidebar-width), 85vw);
}

.content {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  width: var(--sidebar-width);
  padding: 0 var(--sidebar-padding);
}

.sidebar.drawer .content {
  width: min(var(--sidebar-width), 85vw);
}

.resize-handle {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  z-index: 2;
  width: 6px;
  cursor: col-resize;
  touch-action: none;
}

.resize-handle::after {
  content: '';
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 2px;
  background: var(--accent-text);
  opacity: 0;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.resize-handle:hover::after,
.sidebar.resizing .resize-handle::after {
  opacity: 0.65;
}

.header {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  height: var(--sidebar-header-height);
  padding-left: 10px;
}

.lockup {
  display: block; /* drawn at exactly its own size: other sizes lose the pixel alignment */
}

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.icon-button:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.icon-button:active {
  scale: 0.92;
}

.new-row {
  display: flex;
  flex: none;
  gap: 6px;
  margin-bottom: 4px;
}

.new,
.folder,
.stash-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 36px;
  border-radius: 18px;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.new {
  flex: 1;
  min-width: 0;
  gap: 6px;
  font-size: calc(var(--small-font-size) + 1px);
}

.folder,
.stash-btn {
  position: relative;
  flex: none;
  width: 36px;
  color: var(--text-muted);
}

.stash-count {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding-top: 2px;
  font-size: 10px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1;
  pointer-events: none;
}

.new:hover,
.folder:hover,
.stash-btn:hover {
  background: var(--surface-hover);
}

.new:active,
.folder:active,
.stash-btn:active {
  scale: 0.96;
}
.body {
  --sidebar-todo-share: 3; /* the Todo section takes at most this fraction of the column */
  --search-height: 32px;
  --search-gap: 4px;
  --section-resizer-height: 6px;

  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

/* What is left under the Todo section: the percentages of Running count against this. */
.lists {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  min-height: 0;
  transition: flex-grow var(--motion-standard) var(--motion-ease);
}

.body.lists-collapsed .lists {
  flex: 0 1 auto;
}

.section-resizer {
  position: relative;
  flex: none;
  height: var(--section-resizer-height);
  cursor: row-resize;
  touch-action: none;
}

.section-resizer::after {
  content: '';
  position: absolute;
  top: 2px;
  right: 0;
  left: 0;
  height: 2px;
  border-radius: 1px;
  background: var(--accent-text);
  opacity: 0;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.section-resizer:hover::after,
.section-resizer.active::after {
  opacity: 0.65;
}

.section {
  flex: none;
  height: var(--sidebar-section-height);
  margin: 0;
  font-size: var(--small-font-size);
  font-weight: 400;
}

.section-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  height: 100%;
  padding: 0 10px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  text-align: left;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.section-toggle:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.section-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.section-chevron {
  flex: none;
  color: var(--text-muted);
  transition: rotate var(--motion-standard) var(--motion-ease);
}

.section-chevron.open {
  rotate: 90deg;
}

.fold {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.running-slot {
  display: grid;
  flex: 0 1 auto;
  grid-template-rows: 0fr;
  min-height: 0;
  max-height: max(
    calc(
      100% - 2 * var(--sidebar-section-height) - var(--search-height) - var(--search-gap) -
        var(--sidebar-min-history-rows) * var(--sidebar-history-row-height)
    ),
    calc(var(--sidebar-min-running-rows) * (var(--sidebar-row-height) + 2px))
  );
  opacity: 0;
  transition:
    grid-template-rows var(--motion-standard) var(--motion-ease),
    height var(--motion-standard) var(--motion-ease),
    min-height var(--motion-standard) var(--motion-ease),
    max-height var(--motion-standard) var(--motion-ease),
    flex-grow var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.running-slot.custom-height {
  height: 0;
}

.running-slot.open {
  grid-template-rows: 1fr;
  opacity: 1;
}

.running-slot.open.custom-height {
  height: var(--running-height);
  min-height: var(--sidebar-row-height);
  max-height: max(
    var(--sidebar-row-height),
    calc(
      100% - 2 * var(--sidebar-section-height) - var(--section-resizer-height) - var(--search-height) -
        var(--search-gap) - var(--sidebar-history-row-height)
    )
  );
}

.lists.history-collapsed .running-slot {
  max-height: calc(100% - 2 * var(--sidebar-section-height));
}

.lists.history-collapsed .running-slot.open {
  flex: 1 1 auto;
}

.running {
  /* Running keeps its rows and scrolls only when History would get fewer than the minimum rows. */
  display: flex;
  flex: 0 1 auto;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  overflow-y: auto;
}

.running-slot.open.custom-height .running,
.lists.history-collapsed .running-slot.open .running {
  flex: 1 1 auto;
}
.session-enter-active,
.session-leave-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease);
}

.session-leave-active {
  position: absolute;
  width: calc(var(--sidebar-width) - 2 * var(--sidebar-padding));
  transition-timing-function: var(--motion-exit-ease);
}
.sidebar.drawer .session-leave-active {
  width: calc(min(var(--sidebar-width), 85vw) - 2 * var(--sidebar-padding));
}

.session-enter-from,
.session-leave-to {
  opacity: 0;
  scale: 0.96;
}

:deep(.row.row.session-move) {
  transition: transform var(--motion-standard) var(--motion-ease);
}

.search {
  position: relative;
  display: flex;
  flex: none;
  align-items: center;
  gap: 4px;
  height: var(--search-height);
  margin-bottom: var(--search-gap);
  padding: 0 6px 0 10px;
  border-radius: 16px;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.search:focus-within {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.search-icon {
  flex: none;
  color: var(--text-muted);
}

.search input {
  flex: 1;
  min-width: 0;
  height: 100%;
  padding: 0;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: calc(var(--small-font-size) + 1px);
}

.search input::placeholder {
  color: var(--text-muted);
}

.clear,
.filter-button {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.clear:hover,
.filter-button:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.filter-button.active {
  color: var(--accent-text);
}

.history-slot {
  display: grid;
  flex: 0 0 0px;
  grid-template-rows: 0fr;
  min-height: 0;
  opacity: 0;
  transition:
    flex-grow var(--motion-standard) var(--motion-ease),
    grid-template-rows var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease);
}

.history-slot.open {
  flex: 1 1 0px;
  grid-template-rows: 1fr;
  opacity: 1;
}

.history {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  overflow-y: auto;
}

.empty {
  margin: 16px 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  text-align: center;
}

.footer {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  height: var(--sidebar-footer-height);
  padding-left: 10px;
  border-top: 1px solid var(--border);
}

.mcp {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.mcp:hover {
  color: var(--text);
}
.footer-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}
</style>
