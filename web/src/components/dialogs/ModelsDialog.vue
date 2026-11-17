<script setup lang="ts">
// Switch the model of a session (the models dialog). The current variant stays when the new model has it.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ModelInfo } from '../../proto/generated'
import { useCommandsStore } from '../../stores/commands.ts'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import TextButton from '../TextButton.vue'
import PickDialog from './PickDialog.vue'
const TOKENS_PER_K = 1000

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()
const commands = useCommandsStore()

interface ModelItem {
  key: string
  group: string
  model: ModelInfo
}

const keyOf = (provider: string, id: string) => `${provider}/${id}`

const items = computed<ModelItem[]>(() =>
  (connection.config?.models ?? []).map((model) => ({
    key: keyOf(model.provider, model.id),
    group: model.provider_name,
    model,
  })),
)

const current = computed(() => {
  const model = workspace.infoOf(props.sessionId)?.model
  return model ? keyOf(model.provider, model.model) : undefined
})

function details(model: ModelInfo): string {
  const parts = [t('dialog.models.context', { size: `${Math.round(model.context / TOKENS_PER_K)}k` })]
  if (model.reasoning) parts.push(t('dialog.models.reasoning'))
  if (model.images) parts.push(t('dialog.models.images'))
  return parts.join(' · ')
}

function choose({ model }: ModelItem): void {
  const variant = workspace.infoOf(props.sessionId)?.model.variant
  const kept = variant && model.variants.includes(variant) ? variant : null
  dialogs.close()
  workspace
    .setModel(props.sessionId, { provider: model.provider, model: model.id, variant: kept })
    .catch(notices.pushError)
}

function openConfiguration(): void {
  if (commands.find('config')) {
    commands.run('config').catch(notices.pushError)
    return
  }
  dialogs.open({ kind: 'settings' })
}
</script>

<template>
  <PickDialog
    :title="t('dialog.models.title')"
    :search-label="t('dialog.models.search')"
    :items="items"
    :text="(item) => `${item.model.name} ${item.group}`"
    :checked="current"
    :empty="t('dialog.models.empty')"
    :no-match="t('dialog.models.noMatch')"
    @pick="choose"
    @close="dialogs.close()"
  >
    <template #empty-action>
      <TextButton :label="t('dialog.openConfig')" @click="openConfiguration" />
    </template>
    <template #row="{ item }">
      <div class="text">
        <span class="name">{{ item.model.name }}</span>
        <span class="detail">{{ details(item.model) }}</span>
      </div>
    </template>
  </PickDialog>
</template>

<style scoped src="./pickRow.css"></style>
