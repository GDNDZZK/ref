<script setup lang="ts">
// General configuration section: `$name`, `model`, `small_model`, `default_agent`, `compaction.*`,
// `retry.max_delay`, `images.auto_resize`, `tui.{sidebar,thinking,details}`, and provider filters.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  getAt,
  getBooleanAt,
  getNumberAt,
  getStringArrayAt,
  getStringAt,
  setAt,
  type JsonObject,
} from '../../../config/jsonPatch.ts'
import {
  configuredAgentOptions,
  configuredModelOptions,
  normalizeProseInput,
  normalizeTrimmedString,
} from '../../../config/sectionHelpers.ts'
import type { AgentInfo, ModelInfo } from '../../../proto/generated'
import Segmented from '../../Segmented.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import { FormRow, ModelField, NumberField, SelectField, StringListEditor } from '../index.ts'

type TriState = 'default' | 'on' | 'off'

const props = withDefaults(
  defineProps<{
    doc: JsonObject
    serverModels?: readonly ModelInfo[]
    serverAgents?: readonly AgentInfo[]
  }>(),
  {
    serverModels: () => [],
    serverAgents: () => [],
  },
)

const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

const modelOptions = computed(() => configuredModelOptions(props.doc, props.serverModels))
const agentOptions = computed(() => configuredAgentOptions(props.doc, props.serverAgents, false))

const triOptions = computed(() => [
  { value: 'default' as const, label: t('config.editor.triDefault') },
  { value: 'on' as const, label: t('config.editor.triOn') },
  { value: 'off' as const, label: t('config.editor.triOff') },
])

function patch(path: readonly string[], value: unknown): void {
  emit('update:doc', setAt(props.doc, path, value))
}

function patchProseString(path: readonly string[], value: string): void {
  patch(path, normalizeProseInput(value))
}

function patchString(path: readonly string[], value: string): void {
  patch(path, normalizeTrimmedString(value))
}

function patchList(path: readonly string[], items: readonly string[]): void {
  const cleaned = items.filter((entry) => entry.trim() !== '')
  patch(path, cleaned.length > 0 ? cleaned : undefined)
}

function readTri(path: readonly string[]): TriState {
  const raw = getBooleanAt(props.doc, path)
  if (raw === true) return 'on'
  if (raw === false) return 'off'
  return 'default'
}

function writeTri(path: readonly string[], state: TriState): void {
  if (state === 'on') {
    patch(path, true)
    return
  }
  if (state === 'off') {
    patch(path, false)
    return
  }
  patch(path, undefined)
}
</script>

<template>
  <div class="section-body">
    <FormRow
      :label="t('config.editor.general.profileName')"
      :description="t('config.editor.general.profileNameDesc')"
    >
      <input
        :value="getStringAt(doc, ['$name']) ?? ''"
        type="text"
        class="text-input"
        spellcheck="false"
        autocomplete="off"
        :placeholder="t('config.editor.general.profileNamePlaceholder')"
        :aria-label="t('config.editor.general.profileName')"
        @input="patchProseString(['$name'], ($event.target as HTMLInputElement).value)"
        @blur="patchString(['$name'], ($event.target as HTMLInputElement).value)"
      />
    </FormRow>

    <FormRow
      :label="t('config.editor.general.model')"
      :description="t('config.editor.general.modelDesc')"
      :can-reset="getAt(doc, ['model']) !== undefined"
      @reset="patch(['model'], undefined)"
    >
      <ModelField
        :model-value="getStringAt(doc, ['model']) ?? ''"
        :options="modelOptions"
        :label="t('config.editor.general.model')"
        @update:model-value="patchString(['model'], $event)"
      />
    </FormRow>

    <FormRow
      :label="t('config.editor.general.smallModel')"
      :description="t('config.editor.general.smallModelDesc')"
      :can-reset="getAt(doc, ['small_model']) !== undefined"
      @reset="patch(['small_model'], undefined)"
    >
      <ModelField
        :model-value="getStringAt(doc, ['small_model']) ?? ''"
        :options="modelOptions"
        :label="t('config.editor.general.smallModel')"
        @update:model-value="patchString(['small_model'], $event)"
      />
    </FormRow>

    <FormRow
      :label="t('config.editor.general.defaultAgent')"
      :description="t('config.editor.general.defaultAgentDesc')"
      :can-reset="getAt(doc, ['default_agent']) !== undefined"
      @reset="patch(['default_agent'], undefined)"
    >
      <SelectField
        :model-value="getStringAt(doc, ['default_agent']) ?? ''"
        :options="agentOptions"
        placeholder="build"
        :label="t('config.editor.general.defaultAgent')"
        @update:model-value="patchString(['default_agent'], $event)"
      />
    </FormRow>

    <h3 class="sub-heading">{{ t('config.editor.general.compactionHeading') }}</h3>

    <FormRow
      :label="t('config.editor.general.compactionAuto')"
      :description="t('config.editor.general.compactionAutoDesc')"
      :can-reset="getAt(doc, ['compaction', 'auto']) !== undefined"
      @reset="patch(['compaction', 'auto'], undefined)"
    >
      <div class="switch-wrap">
        <ToggleSwitch
          :model-value="getBooleanAt(doc, ['compaction', 'auto']) ?? true"
          :label="t('config.editor.general.compactionAuto')"
          @update:model-value="patch(['compaction', 'auto'], $event)"
        />
      </div>
    </FormRow>

    <FormRow
      :label="t('config.editor.general.compactionRecommend')"
      :description="t('config.editor.general.compactionRecommendDesc')"
      :can-reset="getAt(doc, ['compaction', 'recommend']) !== undefined"
      @reset="patch(['compaction', 'recommend'], undefined)"
    >
      <NumberField
        :model-value="getNumberAt(doc, ['compaction', 'recommend'])"
        :min="0.01"
        :step="0.05"
        placeholder="0.6"
        :label="t('config.editor.general.compactionRecommend')"
        @update:model-value="patch(['compaction', 'recommend'], $event)"
      />
    </FormRow>

    <FormRow
      :label="t('config.editor.general.compactionThreshold')"
      :description="t('config.editor.general.compactionThresholdDesc')"
      :can-reset="getAt(doc, ['compaction', 'threshold']) !== undefined"
      @reset="patch(['compaction', 'threshold'], undefined)"
    >
      <NumberField
        :model-value="getNumberAt(doc, ['compaction', 'threshold'])"
        :min="0.01"
        :step="0.05"
        placeholder="0.9"
        :label="t('config.editor.general.compactionThreshold')"
        @update:model-value="patch(['compaction', 'threshold'], $event)"
      />
    </FormRow>

    <h3 class="sub-heading">{{ t('config.editor.general.runtimeHeading') }}</h3>

    <FormRow
      :label="t('config.editor.general.retryMaxDelay')"
      :description="t('config.editor.general.retryMaxDelayDesc')"
      :can-reset="getAt(doc, ['retry', 'max_delay']) !== undefined"
      @reset="patch(['retry', 'max_delay'], undefined)"
    >
      <NumberField
        :model-value="getNumberAt(doc, ['retry', 'max_delay'])"
        :min="0"
        :step="10"
        placeholder="600"
        :label="t('config.editor.general.retryMaxDelay')"
        @update:model-value="patch(['retry', 'max_delay'], $event)"
      />
    </FormRow>

    <FormRow
      :label="t('config.editor.general.imagesAutoResize')"
      :description="t('config.editor.general.imagesAutoResizeDesc')"
      :can-reset="getAt(doc, ['images', 'auto_resize']) !== undefined"
      @reset="patch(['images', 'auto_resize'], undefined)"
    >
      <div class="switch-wrap">
        <ToggleSwitch
          :model-value="getBooleanAt(doc, ['images', 'auto_resize']) ?? true"
          :label="t('config.editor.general.imagesAutoResize')"
          @update:model-value="patch(['images', 'auto_resize'], $event)"
        />
      </div>
    </FormRow>

    <h3 class="sub-heading">{{ t('config.editor.general.tuiHeading') }}</h3>

    <FormRow :label="t('config.editor.general.tuiSidebar')">
      <Segmented
        :model-value="readTri(['tui', 'sidebar'])"
        :options="triOptions"
        :label="t('config.editor.general.tuiSidebar')"
        @update:model-value="writeTri(['tui', 'sidebar'], $event)"
      />
    </FormRow>

    <FormRow :label="t('config.editor.general.tuiThinking')">
      <Segmented
        :model-value="readTri(['tui', 'thinking'])"
        :options="triOptions"
        :label="t('config.editor.general.tuiThinking')"
        @update:model-value="writeTri(['tui', 'thinking'], $event)"
      />
    </FormRow>

    <FormRow :label="t('config.editor.general.tuiDetails')">
      <Segmented
        :model-value="readTri(['tui', 'details'])"
        :options="triOptions"
        :label="t('config.editor.general.tuiDetails')"
        @update:model-value="writeTri(['tui', 'details'], $event)"
      />
    </FormRow>

    <h3 class="sub-heading">{{ t('config.editor.general.providerFilterHeading') }}</h3>

    <FormRow
      vertical
      :label="t('config.editor.general.enabledProviders')"
      :description="t('config.editor.general.enabledProvidersDesc')"
    >
      <StringListEditor
        monospace
        :model-value="getStringArrayAt(doc, ['enabled_providers'])"
        placeholder="anthropic"
        @update:model-value="patchList(['enabled_providers'], $event)"
      />
    </FormRow>

    <FormRow
      vertical
      :label="t('config.editor.general.disabledProviders')"
      :description="t('config.editor.general.disabledProvidersDesc')"
    >
      <StringListEditor
        monospace
        :model-value="getStringArrayAt(doc, ['disabled_providers'])"
        placeholder="openai"
        @update:model-value="patchList(['disabled_providers'], $event)"
      />
    </FormRow>
  </div>
</template>

<style scoped>
.section-body {
  display: flex;
  flex-direction: column;
}

.sub-heading {
  margin: 18px 0 4px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.text-input {
  width: 100%;
  height: var(--field-height);
  padding: 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.text-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.switch-wrap {
  display: flex;
  justify-content: flex-end;
}
</style>
