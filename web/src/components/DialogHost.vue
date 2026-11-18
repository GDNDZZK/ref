<script setup lang="ts">
// Shows the open dialog over a scrim (ModalDialog.qml's scrim and entrance). A click on the scrim and
// Esc close it. Opening one dialog while another is open cross-fades the panels.
import { computed, nextTick, onBeforeUnmount, onMounted, shallowRef, watch } from 'vue'
import { useComposerStore } from '../stores/composer.ts'
import { containScrimWheel } from '../composables/scrimWheel.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import type { DialogRequest } from '../stores/dialogs.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import CommandPalette from './CommandPalette.vue'
import AgentsDialog from './dialogs/AgentsDialog.vue'
import ConfigDialog from './dialogs/ConfigDialog.vue'
import ConfirmDialog from './dialogs/ConfirmDialog.vue'
import ExportDialog from './dialogs/ExportDialog.vue'
import HelpDialog from './dialogs/HelpDialog.vue'
import McpDialog from './dialogs/McpDialog.vue'
import ModelsDialog from './dialogs/ModelsDialog.vue'
import PathDialog from './dialogs/PathDialog.vue'
import PeersDialog from './dialogs/PeersDialog.vue'
import RenameDialog from './dialogs/RenameDialog.vue'
import SessionsDialog from './dialogs/SessionsDialog.vue'
import SettingsDialog from './dialogs/SettingsDialog.vue'
import VariantsDialog from './dialogs/VariantsDialog.vue'
import { Key } from '../keys.ts'

const dialogs = useDialogsStore()
const workspace = useWorkspaceStore()
const composers = useComposerStore()

// What stays on screen while the scrim fades out.
const shown = shallowRef<DialogRequest>()
watch(
  () => dialogs.current,
  (request) => {
    if (request) shown.value = request
  },
  { immediate: true },
)

const panelKey = computed(() => shown.value?.kind ?? '')

// After a dialog closes the focus goes back to the composer of the session in use.
dialogs.setOnClosed(() => {
  const id = workspace.focusedId
  if (id === undefined) return
  void nextTick(() => composers.find(id)?.requestFocus())
})

function onKeydown(event: KeyboardEvent): void {
  if (event.defaultPrevented) return
  if (event.key === Key.Escape && dialogs.current) {
    event.preventDefault()
    dialogs.requestClose('escape')
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Transition name="scrim" @after-leave="shown = undefined">
    <div
      v-if="dialogs.current"
      class="scrim"
      @mousedown.self="dialogs.requestClose('scrim')"
      @wheel="containScrimWheel"
    >
      <Transition name="panel">
        <div v-if="shown" :key="panelKey" class="panel-slot" :class="{ top: shown.kind === 'palette' }">
          <CommandPalette v-if="shown.kind === 'palette'" />
          <ModelsDialog v-else-if="shown.kind === 'models'" :session-id="shown.sessionId" />
          <AgentsDialog v-else-if="shown.kind === 'agents'" :session-id="shown.sessionId" />
          <VariantsDialog v-else-if="shown.kind === 'variants'" :session-id="shown.sessionId" />
          <SessionsDialog v-else-if="shown.kind === 'sessions'" :mode="shown.mode" />
          <PathDialog v-else-if="shown.kind === 'newSession'" mode="newSession" />
          <PathDialog v-else-if="shown.kind === 'cd'" mode="cd" :session-id="shown.sessionId" />
          <RenameDialog v-else-if="shown.kind === 'rename'" :session-id="shown.sessionId" />
          <ExportDialog v-else-if="shown.kind === 'export'" :session-id="shown.sessionId" />
          <ConfirmDialog
            v-else-if="shown.kind === 'closeSession' || shown.kind === 'deleteHistory'"
            :kind="shown.kind"
            :session-id="shown.sessionId"
          />
          <McpDialog v-else-if="shown.kind === 'mcp'" />
          <PeersDialog
            v-else-if="shown.kind === 'peers'"
            :session-id="shown.sessionId"
            :tab="shown.tab"
            :name="shown.name"
          />
          <HelpDialog v-else-if="shown.kind === 'help'" />
          <SettingsDialog v-else-if="shown.kind === 'settings'" />
          <ConfigDialog v-else-if="shown.kind === 'config'" :section="shown.section" />
        </div>
      </Transition>
    </div>
  </Transition>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  z-index: 30;
  display: grid;
  grid-template: 1fr / 1fr;
  background: var(--scrim);
  overscroll-behavior: contain;
}

.panel-slot {
  grid-area: 1 / 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
  min-height: 0;
  pointer-events: none;
}

.panel-slot.top {
  align-items: flex-start;
  padding-top: max(48px, calc(var(--palette-top-ratio) * 100vh));
}

.panel-slot > :deep(*) {
  pointer-events: auto;
}

.scrim-enter-active {
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.scrim-leave-active {
  transition: opacity var(--motion-quick) var(--motion-exit-ease);
}

.scrim-enter-from,
.scrim-leave-to {
  opacity: 0;
}

.scrim-enter-active .panel-slot > :deep(*) {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease);
}

.scrim-leave-active .panel-slot > :deep(*) {
  transition:
    opacity var(--motion-quick) var(--motion-exit-ease),
    scale var(--motion-quick) var(--motion-exit-ease);
}

.scrim-enter-from .panel-slot > :deep(*) {
  opacity: 0;
  translate: 0 calc(-1 * var(--motion-dialog-drop));
  scale: var(--motion-pop-scale);
}

.scrim-leave-to .panel-slot > :deep(*) {
  opacity: 0;
  scale: var(--motion-pop-scale);
}

.panel-enter-active > :deep(*) {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease);
}

.panel-leave-active {
  transition: opacity var(--motion-quick) var(--motion-exit-ease);
}

.panel-leave-active > :deep(*) {
  pointer-events: none;
  transition:
    opacity var(--motion-quick) var(--motion-exit-ease),
    scale var(--motion-quick) var(--motion-exit-ease);
}

.panel-enter-from > :deep(*) {
  opacity: 0;
  translate: 0 calc(-1 * var(--motion-dialog-drop));
  scale: var(--motion-pop-scale);
}

.panel-leave-to > :deep(*) {
  opacity: 0;
  scale: var(--motion-pop-scale);
}
</style>
