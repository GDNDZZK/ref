<script setup lang="ts">
// The window (Main.qml): the sidebar beside the content, which is one session or the split view,
// with the corner buttons, toasts and dialogs on top.
import { watch } from 'vue'
import { useI18n } from 'vue-i18n'
import BackendBanner from './components/BackendBanner.vue'
import CircleButton from './components/CircleButton.vue'
import ConnectionBanner from './components/ConnectionBanner.vue'
import DialogHost from './components/DialogHost.vue'
import Icon from './components/Icon.vue'
import NoticeList from './components/NoticeList.vue'
import NotificationPrompt from './components/NotificationPrompt.vue'
import SessionPane from './components/SessionPane.vue'
import Sidebar from './components/Sidebar.vue'
import SplitView from './components/SplitView.vue'
import ThemeToggle from './components/ThemeToggle.vue'
import { registerBuiltinCommands } from './commands/builtin.ts'
import { usePermissionKeys } from './composables/usePermissionKeys.ts'
import { useShortcuts } from './composables/useShortcuts.ts'
import { useCommandsStore } from './stores/commands.ts'
import { useDialogsStore } from './stores/dialogs.ts'
import { useLayoutStore } from './stores/layout.ts'
import { useNoticesStore } from './stores/notices.ts'
import { useWorkspaceStore } from './stores/workspace.ts'

const { t } = useI18n()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const notices = useNoticesStore()

registerBuiltinCommands()
useShortcuts()
usePermissionKeys()

// A narrow window has no split view.
watch(
  () => layout.narrow && workspace.mode === 'split',
  (mustLeave) => {
    if (mustLeave) workspace.leaveSplit().catch(notices.pushError)
  },
  { immediate: true },
)

function toggleSplit(): void {
  if (workspace.zoomedId) workspace.unzoom()
  else commands.run('watch').catch(notices.pushError)
}
</script>

<template>
  <div class="app">
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
        <SplitView v-if="workspace.mode === 'split'" />
        <SessionPane v-else-if="workspace.current" :key="workspace.current.id" :session="workspace.current" />

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
        <div class="corner right">
          <CircleButton
            v-if="!layout.narrow"
            :label="
              workspace.zoomedId
                ? t('split.unzoom')
                : workspace.mode === 'split'
                  ? t('split.leave')
                  : t('split.enter')
            "
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

.corner {
  position: absolute;
  top: 14px;
  z-index: 5;
  display: flex;
  gap: 8px;
}

.corner.left {
  left: 14px;
}

.corner.right {
  right: 14px;
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
</style>
