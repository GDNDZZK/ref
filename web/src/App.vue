<script setup lang="ts">
// The window (Main.qml): the sidebar beside the content, which is one session or the split view,
// with the corner buttons, toasts and dialogs on top.
import { computed, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import BackendBanner from './components/BackendBanner.vue'
import CircleButton from './components/CircleButton.vue'
import ConnectionBanner from './components/ConnectionBanner.vue'
import ContextMenu from './components/ContextMenu.vue'
import type { MenuItem } from './components/ContextMenu.vue'
import DialogHost from './components/DialogHost.vue'
import Icon from './components/Icon.vue'
import KeyEntry from './components/KeyEntry.vue'
import NoticeList from './components/NoticeList.vue'
import NotificationPrompt from './components/NotificationPrompt.vue'
import SessionPane from './components/SessionPane.vue'
import Sidebar from './components/Sidebar.vue'
import SplitView from './components/SplitView.vue'
import TextButton from './components/TextButton.vue'
import ThemeToggle from './components/ThemeToggle.vue'
import { registerBuiltinCommands } from './commands/builtin.ts'
import { installCustomCommands } from './commands/custom.ts'
import { setBaseTitle } from './composables/useAttention.ts'
import { usePermissionKeys } from './composables/usePermissionKeys.ts'
import { useShortcuts } from './composables/useShortcuts.ts'
import { APP_TITLE } from './notify/center.ts'
import type { SessionInfo } from './proto/generated'
import { creatorLabel, liveLabel } from './shell/sessionRow.ts'
import { useCommandsStore } from './stores/commands.ts'
import { useConnectionStore } from './stores/connection.ts'
import { useDialogsStore } from './stores/dialogs.ts'
import { useLayoutStore } from './stores/layout.ts'
import { useNoticesStore } from './stores/notices.ts'
import { useWorkspaceStore } from './stores/workspace.ts'

const { t } = useI18n()
const connection = useConnectionStore()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const SPLIT_FILTER_MENU_GAP_PX = 6
const notices = useNoticesStore()

registerBuiltinCommands()
installCustomCommands()
useShortcuts()
usePermissionKeys()

// The tab is named after the open session.
watch(
  () => (workspace.current?.info ? liveLabel(workspace.current.info) : ''),
  (title) => setBaseTitle(title ? `${title} · ${APP_TITLE}` : APP_TITLE),
  { immediate: true },
)

// A narrow window has no split view.
watch(
  () => layout.narrow && workspace.mode === 'split',
  (mustLeave) => {
    if (mustLeave) workspace.leaveSplit().catch(notices.pushError)
  },
  { immediate: true },
)

// Leaving split view while zoomed hands over instantaneously to the full-bleed SessionPane.
const leavingZoomed = ref(false)
let splitZoomedId: string | undefined
watch(
  () => workspace.zoomedId,
  (id) => {
    if (workspace.mode === 'split') splitZoomedId = id
  },
  { flush: 'sync', immediate: true },
)
watch(
  () => workspace.mode,
  (mode, prevMode) => {
    if (mode === 'split') {
      splitZoomedId = workspace.zoomedId
      leavingZoomed.value = false
      return
    }
    if (prevMode === 'split') {
      leavingZoomed.value = splitZoomedId !== undefined
      splitZoomedId = undefined
    }
  },
  { flush: 'sync' },
)

const splitLabel = computed(() => (workspace.mode === 'split' ? t('split.leave') : t('split.enter')))
const showSplitFilter = computed(() => !layout.narrow && workspace.mode === 'split' && !workspace.zoomedId)
const liveById = computed(() => new Map<string, SessionInfo>(workspace.live.map((s) => [s.id, s])))
const splitCreators = computed(() => {
  const ids = new Set<string>()
  for (const session of workspace.live) {
    if (session.live && session.id !== workspace.currentId && session.attached === 0 && session.parent) {
      ids.add(session.parent)
    }
  }
  return [...ids].map((id) => ({ id, name: creatorLabel(liveById.value, id) }))
})
const activeCreatorFilterName = computed(() => {
  const filter = workspace.splitFilter
  if (filter === 'all' || filter === 'user') return undefined
  return creatorLabel(liveById.value, filter)
})
const splitFilterButton = useTemplateRef<HTMLButtonElement>('splitFilterButton')
const splitFilterMenuOpen = ref(false)
const splitFilterMenuMounted = ref(false)
const splitFilterMenuCoords = ref({ x: 0, y: 0 })
const splitFilterMenuItems = computed<MenuItem[]>(() => [
  {
    label: t('dialog.sessions.filterAll'),
    checked: workspace.splitFilter === 'all',
    run: () => workspace.setSplitFilter('all'),
  },
  {
    label: t('dialog.sessions.filterUser'),
    checked: workspace.splitFilter === 'user',
    run: () => workspace.setSplitFilter('user'),
  },
  ...splitCreators.value.map((creator, index) => ({
    label: t('dialog.sessions.filterByName', { name: creator.name }),
    checked: workspace.splitFilter === creator.id,
    separatorBefore: index === 0,
    run: () => workspace.setSplitFilter(creator.id),
  })),
])

function toggleSplitFilterMenu(): void {
  if (splitFilterMenuOpen.value) {
    splitFilterMenuOpen.value = false
    return
  }
  const rect = splitFilterButton.value?.getBoundingClientRect()
  splitFilterMenuCoords.value = {
    x: rect ? rect.left : 0,
    y: rect ? rect.bottom + SPLIT_FILTER_MENU_GAP_PX : 0,
  }
  splitFilterMenuMounted.value = true
  splitFilterMenuOpen.value = true
}

function toggleSplit(): void {
  commands.run('watch').catch(notices.pushError)
}
</script>

<template>
  <KeyEntry v-if="connection.keyRequired" />
  <div v-else class="app">
    <div class="shell" :inert="dialogs.current !== undefined">
      <Sidebar />
      <Transition name="fade">
        <div
          v-if="layout.narrow && layout.sidebarVisible"
          class="drawer-scrim"
          @click="layout.closeDrawer()"
        />
      </Transition>
      <main class="content">
        <BackendBanner />
        <ConnectionBanner />
        <NotificationPrompt />
        <div class="stage">
          <SessionPane
            v-if="workspace.mode !== 'split' && workspace.current"
            :key="workspace.current.id"
            :session="workspace.current"
          />
          <div
            v-else-if="workspace.mode !== 'split' && connection.state === 'open' && !connection.backendTooOld"
            class="empty-workspace"
          >
            <p class="empty-title">{{ t('workspace.empty') }}</p>
            <div class="empty-actions">
              <TextButton
                class="primary-action"
                :label="t('sidebar.newSessionIn')"
                @click="dialogs.open({ kind: 'newSession' })"
              />
              <TextButton
                :label="t('sidebar.newSession')"
                @click="commands.run('new').catch(notices.pushError)"
              />
            </div>
            <p v-if="workspace.cwd" class="empty-cwd">{{ workspace.cwd }}</p>
          </div>
          <Transition name="split-fade" :css="!leavingZoomed">
            <SplitView v-if="workspace.mode === 'split'" />
          </Transition>
        </div>

        <Transition name="fade">
          <div v-if="!layout.sidebarVisible" class="corner left">
            <CircleButton :label="t('sidebar.show')" @click="layout.toggleSidebar()">
              <Icon icon="sidebar-20" />
            </CircleButton>
            <CircleButton
              :label="t('sidebar.newSession')"
              @click="commands.run('new').catch(notices.pushError)"
            >
              <Icon icon="plus-20" />
            </CircleButton>
          </div>
        </Transition>
        <Transition name="fade">
          <div v-if="showSplitFilter" class="split-filter" role="group" :aria-label="t('split.filterLabel')">
            <button
              type="button"
              class="split-filter-pill"
              :class="{ active: workspace.splitFilter === 'all' }"
              :aria-pressed="workspace.splitFilter === 'all'"
              @click="workspace.setSplitFilter('all')"
            >
              {{ t('dialog.sessions.filterAll') }}
            </button>
            <button
              type="button"
              class="split-filter-pill"
              :class="{ active: workspace.splitFilter === 'user' }"
              :aria-pressed="workspace.splitFilter === 'user'"
              @click="workspace.setSplitFilter('user')"
            >
              {{ t('dialog.sessions.filterUser') }}
            </button>
            <button
              v-if="activeCreatorFilterName"
              type="button"
              class="split-filter-pill active creator-pill"
              :title="t('split.clearFilter')"
              @click="workspace.setSplitFilter('all')"
            >
              {{ t('dialog.sessions.filterByName', { name: activeCreatorFilterName }) }} ×
            </button>
            <button
              v-if="splitCreators.length > 0"
              ref="splitFilterButton"
              type="button"
              class="split-filter-more"
              :aria-label="t('split.filterCreators')"
              :title="t('split.filterCreators')"
              :aria-expanded="splitFilterMenuOpen"
              aria-haspopup="menu"
              @click="toggleSplitFilterMenu"
            >
              <Icon icon="filter-16" />
            </button>
            <ContextMenu
              v-if="splitFilterMenuMounted"
              :open="splitFilterMenuOpen"
              :x="splitFilterMenuCoords.x"
              :y="splitFilterMenuCoords.y"
              :items="splitFilterMenuItems"
              @close="splitFilterMenuOpen = false"
            />
          </div>
        </Transition>
        <div class="corner right">
          <CircleButton
            v-if="!layout.narrow && workspace.mode === 'split' && workspace.zoomedId"
            class="unzoom-button"
            :label="t('split.unzoom')"
            @click="workspace.unzoom()"
          >
            <Icon icon="restore-20" />
          </CircleButton>
          <CircleButton
            v-if="!layout.narrow"
            class="split-button"
            :active="workspace.mode === 'split'"
            :label="splitLabel"
            @click="toggleSplit"
          >
            <Icon icon="split-20" />
          </CircleButton>
          <ThemeToggle />
        </div>
      </main>
      <NoticeList />
    </div>
    <DialogHost />
  </div>
</template>

<style scoped>
.app,
.shell {
  display: flex;
  height: 100%;
}

.app {
  position: relative;
}

.shell {
  flex: 1;
  min-width: 0;
}

.content {
  position: relative;
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
}

.stage {
  position: relative;
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.empty-workspace {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: var(--page-padding);
  color: var(--text-muted);
}

.empty-title {
  margin: 0;
}

.empty-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.primary-action :deep(.face) {
  color: var(--accent-text);
}

.empty-cwd {
  margin: 0;
  max-width: min(var(--content-width), 100%);
  overflow: hidden;
  font: var(--small-font-size) var(--font-mono);
  opacity: 0.75;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.corner {
  position: absolute;
  top: var(--corner-inset);
  z-index: 5;
  display: flex;
  gap: var(--corner-gap);
}

.corner.left {
  left: var(--corner-inset);
}

.corner.right {
  right: var(--corner-inset);
}
.split-filter {
  position: absolute;
  top: var(--corner-inset);
  left: 50%;
  translate: -50% 0;
  z-index: 5;
  display: flex;
  align-items: center;
  gap: 2px;
  height: 34px;
  --split-filter-corner-reserve: 260px;
  max-width: calc(100% - var(--split-filter-corner-reserve));
  padding: 3px;
  border-radius: 17px;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.split-filter-pill,
.split-filter-more {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 28px;
  border-radius: 14px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  white-space: nowrap;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.split-filter-pill {
  padding: 0 10px;
}

.creator-pill {
  max-width: 150px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.split-filter-more {
  width: 28px;
}

.split-filter-pill:hover,
.split-filter-more:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.split-filter-pill.active {
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  color: var(--text);
}

.drawer-scrim {
  position: fixed;
  inset: 0;
  z-index: 14;
  background: var(--scrim);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.split-fade-enter-active,
.split-fade-leave-active {
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.split-fade-leave-active {
  position: absolute;
  inset: 0;
  z-index: 2;
  pointer-events: none;
}

.split-fade-enter-from,
.split-fade-leave-to {
  opacity: 0;
}
</style>
