<script setup lang="ts">
// The sidebar (Sidebar.qml): running sessions, searchable history, MCP status and settings.
// The content keeps its width and is clipped by the frame, so collapsing slides it out to the left.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { matchesQuery } from '../shell/sessionRow.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useMcpStore } from '../stores/mcp.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import HistoryRow from './HistoryRow.vue'
import Icon from './Icon.vue'
import McpDots from './McpDots.vue'
import SidebarRow from './SidebarRow.vue'

const { t } = useI18n()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const mcp = useMcpStore()
const notices = useNoticesStore()

const query = ref('')

const liveIds = computed(() => new Set(workspace.live.map((session) => session.id)))
const history = computed(() =>
  workspace.history.filter((session) => !liveIds.value.has(session.id) && matchesQuery(session, query.value)),
)

function report(promise: Promise<unknown>): void {
  promise.catch(notices.pushError)
}

function open(id: string): void {
  layout.closeDrawer()
  if (workspace.mode === 'split' && workspace.tileIds.includes(id)) workspace.focusTile(id)
  else report(workspace.openSession(id))
}
</script>

<template>
  <aside class="sidebar" :class="{ open: layout.sidebarVisible, drawer: layout.narrow }">
    <div class="content">
      <header class="header">
        <span class="brand">ref</span>
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
        <h2 class="section">{{ t('sidebar.running') }}</h2>
        <TransitionGroup tag="div" name="session" class="running" role="list">
          <SidebarRow
            v-for="info in workspace.live"
            :key="info.id"
            :info="info"
            :current="info.id === workspace.focusedId"
            :tile="workspace.mode === 'split' && workspace.tileIds.includes(info.id)"
            @open="open(info.id)"
            @close="report(commands.run('close', '', info.id))"
            @rename="(name) => report(workspace.renameSession(info.id, name))"
          />
        </TransitionGroup>

        <h2 class="section">{{ t('sidebar.history') }}</h2>
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
        <button
          type="button"
          class="icon-button"
          :aria-label="t('sidebar.settings')"
          :title="t('sidebar.settings')"
          @click="report(commands.run('settings'))"
        >
          <Icon icon="sliders-20" />
        </button>
      </footer>
    </div>
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

/* Narrow windows: over the content instead of beside it. */
.sidebar.drawer {
  position: fixed;
  z-index: 15;
  left: 0;
  top: 0;
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

.header {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  height: var(--sidebar-header-height);
  padding-left: 10px;
}

.brand {
  font-size: calc(var(--font-size) + 1px);
  font-weight: 700;
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
  --search-height: 32px;
  --search-gap: 4px;

  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.section {
  flex: none;
  display: flex;
  align-items: center;
  height: var(--sidebar-section-height);
  margin: 0;
  padding-left: 10px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 400;
}

.running {
  /* Running keeps its rows and scrolls only when History would get fewer than the minimum rows. */
  display: flex;
  flex: 0 1 auto;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  max-height: max(
    calc(
      100% - 2 * var(--sidebar-section-height) - var(--search-height) - var(--search-gap) -
        var(--sidebar-min-history-rows) * var(--sidebar-history-row-height)
    ),
    calc(var(--sidebar-min-running-rows) * (var(--sidebar-row-height) + 2px))
  );
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
  box-shadow: inset 0 0 0 1px var(--accent);
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
</style>
