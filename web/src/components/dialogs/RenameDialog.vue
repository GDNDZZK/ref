<script setup lang="ts">
// Rename a session. Names are unique among live sessions: the backend's refusal shows under the field.
import { onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { describeError } from '../../stores/notices.ts'
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
const error = ref('')
const busy = ref(false)
const field = useTemplateRef<InstanceType<typeof TextField>>('field')
onMounted(() => field.value?.select())

async function submit(): Promise<void> {
  const next = name.value.trim()
  if (next === '' || busy.value) return
  busy.value = true
  try {
    await workspace.renameSession(props.sessionId, next)
    dialogs.close()
  } catch (failure) {
    error.value = describeError(failure)
  } finally {
    busy.value = false
  }
}
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
