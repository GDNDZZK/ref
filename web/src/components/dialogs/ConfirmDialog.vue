<script setup lang="ts">
// Closing a working session and deleting one from history ask first.
import { onMounted, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { displayTitle } from '../../shell/sessionRow.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import ModalDialog from '../ModalDialog.vue'

const props = defineProps<{ kind: 'closeSession' | 'deleteHistory'; sessionId: string }>()
const { t } = useI18n()
const dialogs = useDialogsStore()
const workspace = useWorkspaceStore()
const notices = useNoticesStore()

const info = workspace.infoOf(props.sessionId) ?? workspace.history.find((s) => s.id === props.sessionId)
const body = t(`dialog.${props.kind}.body`, { title: info ? displayTitle(info) : props.sessionId })

const confirmButton = useTemplateRef<InstanceType<typeof DialogButton>>('confirm')
onMounted(() => (confirmButton.value?.$el as HTMLElement | undefined)?.focus())

function confirm(): void {
  dialogs.close()
  const done =
    props.kind === 'closeSession'
      ? workspace.closeSession(props.sessionId)
      : workspace.deleteHistory(props.sessionId)
  done.catch(notices.pushError)
}
</script>

<template>
  <ModalDialog :title="t(`dialog.${kind}.title`)" @close="dialogs.close()">
    <p class="body">{{ body }}</p>
    <template #footer>
      <DialogButton :label="t('dialog.cancel')" @click="dialogs.close()" />
      <DialogButton ref="confirm" kind="danger" :label="t(`dialog.${kind}.action`)" @click="confirm" />
    </template>
  </ModalDialog>
</template>

<style scoped>
.body {
  margin: 0;
  color: var(--text-muted);
}
</style>
