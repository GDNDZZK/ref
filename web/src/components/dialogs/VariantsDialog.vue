<script setup lang="ts">
// Choose the variant (reasoning effort) of the session's model.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import PickDialog from './PickDialog.vue'

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()

const DEFAULT_KEY = ''

const model = computed(() => {
  const ref = workspace.infoOf(props.sessionId)?.model
  return connection.config?.models.find((m) => m.provider === ref?.provider && m.id === ref?.model)
})

// The model's own variants, with "default" (none) first.
const items = computed(() => [
  { key: DEFAULT_KEY, label: t('dialog.variants.default') },
  ...(model.value?.variants ?? []).map((name) => ({ key: name, label: name })),
])

function choose(item: { key: string }): void {
  dialogs.close()
  const change =
    item.key === DEFAULT_KEY
      ? workspace.clearVariant(props.sessionId)
      : workspace.setVariant(props.sessionId, item.key)
  change.catch(notices.pushError)
}
</script>

<template>
  <PickDialog
    :title="t('dialog.variants.title')"
    :search-label="t('dialog.variants.title')"
    :items="items"
    :text="(item) => item.label"
    :checked="workspace.infoOf(sessionId)?.model.variant ?? DEFAULT_KEY"
    @pick="choose"
    @close="dialogs.close()"
  >
    <template #row="{ item }">
      <span class="name">{{ item.label }}</span>
    </template>
  </PickDialog>
</template>

<style scoped src="./pickRow.css"></style>
