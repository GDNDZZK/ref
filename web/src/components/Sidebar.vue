<script setup lang="ts">
// The sidebar (Sidebar.qml): running sessions, searchable history, MCP status and settings.
// The content keeps its width and is clipped by the frame, so collapsing slides it out to the left.
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import lockupDark from '../../../assets/app/ref-lockup-small-dark.svg'
import lockupLight from '../../../assets/app/ref-lockup-small-light.svg'
import { useTheme } from '../composables/useTheme.ts'
import { isWorkingSession, matchesQuery } from '../shell/sessionRow.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useMcpStore } from '../stores/mcp.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import Chevron from './Chevron.vue'
import HistoryRow from './HistoryRow.vue'
import Icon from './Icon.vue'
import McpDots from './McpDots.vue'
import SidebarRow from './SidebarRow.vue'
import SidebarTodo from './SidebarTodo.vue'
const LOCKUP_WIDTH = 60
const LOCKUP_HEIGHT = 24

const { t } = useI18n()
const { isDark } = useTheme()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const connection = useConnectionStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const mcp = useMcpStore()
const notices = useNoticesStore()

const query = ref('')

// The session on screen: the open one, or the focused tile in split view.
const todos = computed(() => (workspace.focusedId ? workspace.infoOf(workspace.focusedId)?.todos : undefined))

const liveIds = computed(() => new Set(workspace.live.map((session) => session.id)))
const history = computed(() =>
  workspace.history.filter((session) => !liveIds.value.has(session.id) && matchesQuery(session, query.value)),
)

function report(promise: Promise<unknown>): void {
  promise.catch(notices.pushError)
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
</script>

<template>
  <aside
    class="sidebar"
    :class="{ open: layout.sidebarVisible, drawer: layout.narrow, resizing }"
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

      <div class="body">
        <SidebarTodo
          :todos="todos"
          :expanded="layout.sidebarTodoExpanded"
          @toggle="layout.toggleSidebarTodo()"
        />
        <div class="lists" :class="{ 'history-collapsed': !layout.sidebarHistoryExpanded }">
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
            class="running-slot"
            :class="{ open: layout.sidebarRunningExpanded }"
            :inert="!layout.sidebarRunningExpanded || undefined"
          >
            <div class="fold">
              <TransitionGroup tag="div" name="session" class="running" role="list">
                <SidebarRow
                  v-for="info in workspace.live"
                  :key="info.id"
                  :info="info"
                  :current="info.id === workspace.focusedId"
                  :tile="workspace.mode === 'split' && workspace.tileIds.includes(info.id)"
                  :can-detach="workspace.mode === 'single' && info.id === workspace.currentId"
                  @open="open(info.id)"
                  @detach="report(workspace.detachCurrent())"
                  @close="close(info.id)"
                  @rename="(name) => report(workspace.renameSession(info.id, name))"
                />
              </TransitionGroup>
            </div>
          </div>

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
              <label class="search">
                <Icon icon="search-16" class="search-icon" />
                <input v-model="query" type="text" :placeholder="t('sidebar.searchHistory')" />
                <button
                  v-if="query"
                  type="button"
                  class="clear"
                  :aria-label="t('sidebar.clearSearch')"
                  @click="query = ''"
                >
                  <Icon icon="close-16" />
                </button>
              </label>
              <div class="history" role="list">
                <HistoryRow
                  v-for="info in history"
                  :key="info.id"
                  :info="info"
                  @open="open(info.id)"
                  @remove="dialogs.open({ kind: 'deleteHistory', sessionId: info.id })"
                />
                <p v-if="!workspace.historyLoaded" class="empty">{{ t('sidebar.loading') }}</p>
                <p v-else-if="history.length === 0" class="empty">{{ t('sidebar.noMatch') }}</p>
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
.folder {
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
  gap: 6px;
  font-size: calc(var(--small-font-size) + 1px);
}

.folder {
  width: 36px;
  color: var(--text-muted);
}

.new:hover,
.folder:hover {
  background: var(--surface-hover);
}

.new:active,
.folder:active {
  scale: 0.96;
}

.body {
  --sidebar-todo-share: 3; /* the Todo section takes at most this fraction of the column */

  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

/* What is left under the Todo section: the percentages of Running count against this. */
.lists {
  --search-height: 32px;
  --search-gap: 4px;

  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
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
    opacity var(--motion-standard) var(--motion-ease);
}

.running-slot.open {
  grid-template-rows: 1fr;
  opacity: 1;
}

.lists.history-collapsed .running-slot {
  max-height: calc(100% - 2 * var(--sidebar-section-height));
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

.session-move {
  transition: translate var(--motion-standard) var(--motion-ease);
}

.search {
  position: relative;
  display: flex;
  flex: none;
  align-items: center;
  height: var(--search-height);
  margin-bottom: var(--search-gap);
  border-radius: 16px;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.search:focus-within {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.search-icon {
  position: absolute;
  left: 10px;
  color: var(--text-muted);
}

.search input {
  flex: 1;
  min-width: 0;
  height: 100%;
  padding: 0 32px;
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

.clear {
  position: absolute;
  right: 8px;
  display: flex;
  color: var(--text-muted);
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
