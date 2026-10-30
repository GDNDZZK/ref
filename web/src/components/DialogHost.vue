<script setup lang="ts">
// Shows the open dialog over a scrim (ModalDialog.qml's scrim and entrance). A click on the scrim and
// Esc close it. Opening one dialog while another is open cross-fades the panels.
import { computed, nextTick, onBeforeUnmount, onMounted, shallowRef, watch } from 'vue'
import { useDialogsStore } from '../stores/dialogs.ts'
import type { DialogRequest } from '../stores/dialogs.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import CommandPalette from './CommandPalette.vue'
import AgentsDialog from './dialogs/AgentsDialog.vue'
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

const dialogs = useDialogsStore()
const workspace = useWorkspaceStore()

// What stays on screen while the scrim fades out.
const shown = shallowRef<DialogRequest>()
watch(
  () => dialogs.current,
  (request) => {
    if (request) shown.value = request
  },
  { immediate: true },
)

const isPalette = computed(() => shown.value?.kind === 'palette')

// After a dialog closes the focus goes back to the composer of the session in use.
dialogs.setOnClosed(() => {
  const id = workspace.focusedId
  if (id === undefined) return
  void nextTick(() => document.querySelector<HTMLElement>(`[data-pane="${id}"] textarea`)?.focus())
})

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape' && dialogs.current) {
    event.preventDefault()
    dialogs.close()
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Transition name="scrim" @after-leave="shown = undefined">
    <div v-if="dialogs.current" class="scrim" :class="{ top: isPalette }" @mousedown.self="dialogs.close()">
      <Transition name="panel" mode="out-in" appear>
        <CommandPalette v-if="shown?.kind === 'palette'" key="palette" />
        <ModelsDialog v-else-if="shown?.kind === 'models'" key="models" :session-id="shown.sessionId" />
        <AgentsDialog v-else-if="shown?.kind === 'agents'" key="agents" :session-id="shown.sessionId" />
        <VariantsDialog v-else-if="shown?.kind === 'variants'" key="variants" :session-id="shown.sessionId" />
        <SessionsDialog v-else-if="shown?.kind === 'sessions'" key="sessions" />
        <PathDialog v-else-if="shown?.kind === 'newSession'" key="newSession" mode="newSession" />
        <PathDialog v-else-if="shown?.kind === 'cd'" key="cd" mode="cd" :session-id="shown.sessionId" />
        <RenameDialog v-else-if="shown?.kind === 'rename'" key="rename" :session-id="shown.sessionId" />
        <ExportDialog v-else-if="shown?.kind === 'export'" key="export" :session-id="shown.sessionId" />
        <ConfirmDialog
          v-else-if="shown?.kind === 'closeSession' || shown?.kind === 'deleteHistory'"
          :key="shown.kind"
          :kind="shown.kind"
          :session-id="shown.sessionId"
        />
        <McpDialog v-else-if="shown?.kind === 'mcp'" key="mcp" />
        <PeersDialog
          v-else-if="shown?.kind === 'peers'"
          key="peers"
          :session-id="shown.sessionId"
          :tab="shown.tab"
          :name="shown.name"
        />
        <HelpDialog v-else-if="shown?.kind === 'help'" key="help" />
        <SettingsDialog v-else-if="shown?.kind === 'settings'" key="settings" />
      </Transition>
    </div>
  </Transition>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--scrim);
}

.scrim.top {
  align-items: flex-start;
  padding-top: max(48px, calc(var(--palette-top-ratio) * 100vh));
}

.scrim-enter-active,
.scrim-leave-active {
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.scrim-enter-from,
.scrim-leave-to {
  opacity: 0;
}

.panel-enter-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease);
}

.panel-leave-active {
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.panel-enter-from {
  opacity: 0;
  translate: 0 calc(-1 * var(--motion-dialog-drop));
  scale: var(--motion-pop-scale);
}

.panel-leave-to {
  opacity: 0;
}
</style>
