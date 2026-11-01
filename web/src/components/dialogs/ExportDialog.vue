<script setup lang="ts">
// Write the transcript as markdown on the backend's machine.
import { onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDialogSubmit } from '../../composables/useDialogSubmit.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import ModalDialog from '../ModalDialog.vue'
import TextField from '../TextField.vue'

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const dialogs = useDialogsStore()
const notices = useNoticesStore()
const workspace = useWorkspaceStore()

const path = ref(`ref-session-${props.sessionId}.md`)
const field = useTemplateRef<InstanceType<typeof TextField>>('field')
onMounted(() => field.value?.select())

const { busy, error, submit } = useDialogSubmit(
  async () => {
    const written = await workspace.exportSession(props.sessionId, path.value.trim())
    dialogs.close()
    notices.push('info', t('dialog.export.done', { path: written }))
  },
  () => path.value.trim() !== '',
)
</script>

<template>
  <ModalDialog :title="t('dialog.export.title')" @close="dialogs.close()">
    <p class="body">{{ t('dialog.export.body') }}</p>
    <TextField ref="field" v-model="path" :label="t('dialog.path.label')" :error="error" @enter="submit" />
    <template #footer>
      <DialogButton :label="t('dialog.cancel')" @click="dialogs.close()" />
      <DialogButton
        kind="primary"
        :label="t('dialog.export.action')"
        :disabled="busy || path.trim() === ''"
        @click="submit"
      />
    </template>
  </ModalDialog>
</template>

<style scoped>
.body {
  margin: 0 0 12px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}
</style>
