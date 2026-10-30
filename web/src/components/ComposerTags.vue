<script setup lang="ts">
// The pills at the bottom left of the input box: shell mode, agent, model, variant.
// What they show is the session's own info, so a change made anywhere (Tab, a dialog,
// another window) morphs them when the backend confirms it.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ChatSession } from '../model/chatSession.ts'
import { useConnectionStore } from '../stores/connection.ts'
import { agentColor } from '../theme/agentColor.ts'
import TagPill from './TagPill.vue'

const props = defineProps<{ session: ChatSession; shell: boolean }>()
defineEmits<{ agentClicked: []; modelClicked: []; variantClicked: [] }>()

const { t } = useI18n()
const connection = useConnectionStore()

const info = computed(() => props.session.info)
const agentIndex = computed(
  () => connection.config?.agents.findIndex((a) => a.name === info.value?.agent) ?? -1,
)
const agentSpec = computed(() => connection.config?.agents[agentIndex.value]?.color ?? '')
const model = computed(() =>
  connection.config?.models.find(
    (m) => m.provider === info.value?.model.provider && m.id === info.value?.model.model,
  ),
)
const variants = computed(() => model.value?.variants ?? [])
const variantLabel = computed(() => info.value?.model.variant ?? t('composer.tags.defaultVariant'))
const modelTooltip = computed(() => {
  const provider = model.value?.provider_name ?? info.value?.model.provider ?? ''
  return `${t('composer.tags.model')} · ${provider}`
})
</script>

<template>
  <div class="tags">
    <TagPill :shown="shell" icon="terminal-16" :label="t('composer.tags.shell')" accent />
    <template v-if="info">
      <TagPill
        shown
        icon="agent-16"
        :label="info.agent"
        :icon-color="agentColor(agentSpec, agentIndex)"
        :tooltip="t('composer.tags.agent')"
        clickable
        label-hover
        @click="$emit('agentClicked')"
      />
      <TagPill
        shown
        icon="model-16"
        :label="model?.name ?? info.model.model"
        :tooltip="modelTooltip"
        clickable
        @click="$emit('modelClicked')"
      />
      <TagPill
        :shown="variants.length > 0"
        icon="variant-16"
        :label="variantLabel"
        :tooltip="t('composer.tags.variant')"
        clickable
        @click="$emit('variantClicked')"
      />
    </template>
  </div>
</template>

<style scoped>
.tags {
  display: flex;
  align-items: center;
  min-width: 0;
}
</style>
