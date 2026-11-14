<script setup lang="ts">
// Rename a session. Names are unique among live sessions: the backend's refusal shows under the field.
import { onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDialogSubmit } from '../../composables/useDialogSubmit.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import ModalDialog from '../ModalDialog.vue'
import TextField from '../TextField.vue'

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const dialogs = useDialogsStore()
const workspace = useWorkspaceStore()

const info = workspace.infoOf(props.sessionId)
const name = ref(info?.name ?? '')
const field = useTemplateRef<InstanceType<typeof TextField>>('field')
onMounted(() => field.value?.select())

const { busy, error, submit } = useDialogSubmit(
  async () => {
    await workspace.renameSession(props.sessionId, name.value.trim())
    dialogs.close()
  },
  () => name.value.trim() !== '',
)
</script>

<template>
  <ModalDialog :title="t('dialog.rename.title')" @close="dialogs.close()">
    <TextField ref="field" v-model="name" :label="t('dialog.rename.label')" :error="error" @enter="submit" />
    <template #footer>
      <DialogButton :label="t('dialog.cancel')" @click="dialogs.close()" />
      <DialogButton
        kind="primary"
        :label="t('dialog.rename.action')"
        :disabled="busy || name.trim() === ''"
        @click="submit"
      />
    </template>
  </ModalDialog>
</template>
