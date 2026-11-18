<script setup lang="ts">
// Agents configuration section: built-in overrides (`build`, `plan`, `general`, `explore`,
// `compaction`) with Customized badge & Reset override, plus custom agent CRUD.
import { computed, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  BUILTIN_AGENTS,
  BUILTIN_AGENT_META,
  getAt,
  getBooleanAt,
  getNumberAt,
  getObjectAt,
  getStringAt,
  isPlainObject,
  setAt,
  type BuiltinAgentMeta,
  type JsonObject,
} from '../../../config/jsonPatch.ts'
import { configuredModelOptions } from '../../../config/sectionHelpers.ts'
import type { ModelInfo } from '../../../proto/generated'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import TextButton from '../../TextButton.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import {
  ColorSwatchPicker,
  FormRow,
  ModelField,
  NumberField,
  PermissionRulesEditor,
  SelectField,
  TextAreaField,
  ToolDelaysEditor,
} from '../index.ts'

const MODE_OPTIONS = [
  { value: 'primary', label: 'primary' },
  { value: 'subagent', label: 'subagent' },
  { value: 'all', label: 'all' },
] as const

const props = withDefaults(
  defineProps<{
    doc: JsonObject
    serverModels?: readonly ModelInfo[]
  }>(),
  {
    serverModels: () => [],
  },
)

const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

const openAgents = reactive(new Set<string>())
const newAgentName = ref('')

const builtinSet = new Set<string>(BUILTIN_AGENTS)
const agentsMap = computed(() => getObjectAt(props.doc, ['agent']) ?? {})
const customAgentNames = computed(() => Object.keys(agentsMap.value).filter((name) => !builtinSet.has(name)))
const modelOptions = computed(() => configuredModelOptions(props.doc, props.serverModels))

function isCustomized(name: string): boolean {
  return getObjectAt(props.doc, ['agent', name]) !== undefined
}

function agentObj(name: string): JsonObject {
  const raw = agentsMap.value[name]
  return isPlainObject(raw) ? raw : {}
}

function toggleAgent(name: string): void {
  if (openAgents.has(name)) openAgents.delete(name)
  else openAgents.add(name)
}

function patchAgent(name: string, subPath: readonly string[], value: unknown): void {
  emit('update:doc', setAt(props.doc, ['agent', name, ...subPath], value))
}

function patchAgentString(name: string, subPath: readonly string[], value: string): void {
  const trimmed = value.trim()
  patchAgent(name, subPath, trimmed === '' ? undefined : trimmed)
}

function patchPrompt(name: string, value: string): void {
  patchAgent(name, ['prompt'], value === '' ? undefined : value)
}

function resetBuiltin(name: string): void {
  openAgents.delete(name)
  emit('update:doc', setAt(props.doc, ['agent', name], undefined))
}

function addCustomAgent(): void {
  const trimmed = newAgentName.value.trim()
  if (trimmed === '' || builtinSet.has(trimmed) || Object.hasOwn(agentsMap.value, trimmed)) return
  newAgentName.value = ''
  openAgents.add(trimmed)
  emit('update:doc', setAt(props.doc, ['agent', trimmed], { description: '' }))
}

function removeCustomAgent(name: string): void {
  openAgents.delete(name)
  emit('update:doc', setAt(props.doc, ['agent', name], undefined))
}

function builtinMeta(name: string): BuiltinAgentMeta | undefined {
  return BUILTIN_AGENT_META.find((meta) => meta.name === name)
}

function readAgentDisabled(name: string): boolean {
  const obj = agentObj(name)
  return (getBooleanAt(obj, ['disable']) ?? getBooleanAt(obj, ['disabled'])) === true
}

function writeAgentDisabled(name: string, disabled: boolean): void {
  let next = setAt(props.doc, ['agent', name, 'disabled'], undefined)
  next = setAt(next, ['agent', name, 'disable'], disabled ? true : undefined)
  emit('update:doc', next)
}
</script>

<template>
  <div class="section-body">
    <h3 class="group-heading">{{ t('config.editor.agents.builtinHeading') }}</h3>

    <div v-for="meta in BUILTIN_AGENT_META" :key="meta.name" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleAgent(meta.name)">
          <Chevron :open="openAgents.has(meta.name)" :size="12" />
          <span class="card-id">{{ meta.name }}</span>
          <span class="meta-pill">{{ meta.mode }}</span>
          <span v-if="isCustomized(meta.name)" class="custom-badge">
            {{ t('config.editor.agents.customized') }}
          </span>
        </button>
        <TextButton
          v-if="isCustomized(meta.name)"
          :label="t('config.editor.agents.resetOverride')"
          @click="resetBuiltin(meta.name)"
        />
      </div>

      <div v-if="openAgents.has(meta.name)" class="card-body">
        <FormRow :label="t('config.editor.agents.description')">
          <input
            :value="getStringAt(agentObj(meta.name), ['description']) ?? ''"
            type="text"
            class="text-input"
            @input="patchAgentString(meta.name, ['description'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.mode')"
          :can-reset="getAt(agentObj(meta.name), ['mode']) !== undefined"
          @reset="patchAgent(meta.name, ['mode'], undefined)"
        >
          <SelectField
            :model-value="getStringAt(agentObj(meta.name), ['mode']) ?? ''"
            :options="MODE_OPTIONS"
            :placeholder="meta.mode"
            :label="t('config.editor.agents.mode')"
            @update:model-value="patchAgentString(meta.name, ['mode'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.model')"
          :can-reset="getAt(agentObj(meta.name), ['model']) !== undefined"
          @reset="patchAgent(meta.name, ['model'], undefined)"
        >
          <ModelField
            :model-value="getStringAt(agentObj(meta.name), ['model']) ?? ''"
            :options="modelOptions"
            :label="t('config.editor.agents.model')"
            @update:model-value="patchAgentString(meta.name, ['model'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.variant')">
          <input
            :value="getStringAt(agentObj(meta.name), ['variant']) ?? ''"
            type="text"
            class="text-input mono"
            spellcheck="false"
            @input="patchAgentString(meta.name, ['variant'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.color')">
          <ColorSwatchPicker
            :model-value="getStringAt(agentObj(meta.name), ['color']) ?? ''"
            :label="t('config.editor.agents.color')"
            @update:model-value="patchAgentString(meta.name, ['color'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.creatable')"
          :can-reset="getAt(agentObj(meta.name), ['creatable']) !== undefined"
          @reset="patchAgent(meta.name, ['creatable'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(meta.name), ['creatable']) ?? !meta.hidden"
              :label="t('config.editor.agents.creatable')"
              @update:model-value="patchAgent(meta.name, ['creatable'], $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.canSpawn')"
          :can-reset="getAt(agentObj(meta.name), ['can_spawn']) !== undefined"
          @reset="patchAgent(meta.name, ['can_spawn'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(meta.name), ['can_spawn']) ?? meta.canSpawnDefault"
              :label="t('config.editor.agents.canSpawn')"
              @update:model-value="patchAgent(meta.name, ['can_spawn'], $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.hidden')"
          :can-reset="getAt(agentObj(meta.name), ['hidden']) !== undefined"
          @reset="patchAgent(meta.name, ['hidden'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(meta.name), ['hidden']) ?? meta.hidden"
              :label="t('config.editor.agents.hidden')"
              @update:model-value="patchAgent(meta.name, ['hidden'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.disable')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="readAgentDisabled(meta.name)"
              :label="t('config.editor.agents.disable')"
              @update:model-value="writeAgentDisabled(meta.name, $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.temperature')">
          <NumberField
            :model-value="getNumberAt(agentObj(meta.name), ['temperature'])"
            :min="0"
            :max="2"
            :step="0.1"
            @update:model-value="patchAgent(meta.name, ['temperature'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.topP')">
          <NumberField
            :model-value="getNumberAt(agentObj(meta.name), ['top_p'])"
            :min="0"
            :max="1"
            :step="0.05"
            @update:model-value="patchAgent(meta.name, ['top_p'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.prompt')">
          <TextAreaField
            :rows="5"
            :model-value="getStringAt(agentObj(meta.name), ['prompt']) ?? ''"
            :placeholder="t('config.editor.agents.promptPlaceholder')"
            :label="t('config.editor.agents.prompt')"
            @update:model-value="patchPrompt(meta.name, $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.permission')">
          <PermissionRulesEditor
            :model-value="getAt(agentObj(meta.name), ['permission'])"
            :label="t('config.editor.agents.permission')"
            @update:model-value="patchAgent(meta.name, ['permission'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.delay')">
          <ToolDelaysEditor
            :model-value="getAt(agentObj(meta.name), ['delay'])"
            @update:model-value="patchAgent(meta.name, ['delay'], $event)"
          />
        </FormRow>
      </div>
    </div>

    <h3 class="group-heading divider">{{ t('config.editor.agents.customHeading') }}</h3>

    <p v-if="customAgentNames.length === 0" class="empty">{{ t('config.editor.agents.customEmpty') }}</p>

    <div v-for="name in customAgentNames" :key="name" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleAgent(name)">
          <Chevron :open="openAgents.has(name)" :size="12" />
          <span class="card-id">{{ name }}</span>
          <span v-if="getStringAt(agentObj(name), ['description'])" class="card-sub">
            {{ getStringAt(agentObj(name), ['description']) }}
          </span>
        </button>
        <button
          type="button"
          class="icon-btn danger"
          :aria-label="t('config.editor.delete')"
          :title="t('config.editor.delete')"
          @click="removeCustomAgent(name)"
        >
          <Icon icon="trash-16" />
        </button>
      </div>

      <div v-if="openAgents.has(name)" class="card-body">
        <FormRow :label="t('config.editor.agents.description')">
          <input
            :value="getStringAt(agentObj(name), ['description']) ?? ''"
            type="text"
            class="text-input"
            @input="patchAgentString(name, ['description'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.mode')"
          :can-reset="getAt(agentObj(name), ['mode']) !== undefined"
          @reset="patchAgent(name, ['mode'], undefined)"
        >
          <SelectField
            :model-value="getStringAt(agentObj(name), ['mode']) ?? ''"
            :options="MODE_OPTIONS"
            :placeholder="builtinMeta(name)?.mode ?? 'all'"
            :label="t('config.editor.agents.mode')"
            @update:model-value="patchAgentString(name, ['mode'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.model')"
          :can-reset="getAt(agentObj(name), ['model']) !== undefined"
          @reset="patchAgent(name, ['model'], undefined)"
        >
          <ModelField
            :model-value="getStringAt(agentObj(name), ['model']) ?? ''"
            :options="modelOptions"
            :label="t('config.editor.agents.model')"
            @update:model-value="patchAgentString(name, ['model'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.variant')">
          <input
            :value="getStringAt(agentObj(name), ['variant']) ?? ''"
            type="text"
            class="text-input mono"
            spellcheck="false"
            @input="patchAgentString(name, ['variant'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.color')">
          <ColorSwatchPicker
            :model-value="getStringAt(agentObj(name), ['color']) ?? ''"
            :label="t('config.editor.agents.color')"
            @update:model-value="patchAgentString(name, ['color'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.creatable')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(name), ['creatable']) ?? true"
              :label="t('config.editor.agents.creatable')"
              @update:model-value="patchAgent(name, ['creatable'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.canSpawn')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(name), ['can_spawn']) ?? false"
              :label="t('config.editor.agents.canSpawn')"
              @update:model-value="patchAgent(name, ['can_spawn'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.hidden')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(name), ['hidden']) ?? false"
              :label="t('config.editor.agents.hidden')"
              @update:model-value="patchAgent(name, ['hidden'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.disable')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="readAgentDisabled(name)"
              :label="t('config.editor.agents.disable')"
              @update:model-value="writeAgentDisabled(name, $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.temperature')">
          <NumberField
            :model-value="getNumberAt(agentObj(name), ['temperature'])"
            :min="0"
            :max="2"
            :step="0.1"
            @update:model-value="patchAgent(name, ['temperature'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.topP')">
          <NumberField
            :model-value="getNumberAt(agentObj(name), ['top_p'])"
            :min="0"
            :max="1"
            :step="0.05"
            @update:model-value="patchAgent(name, ['top_p'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.prompt')">
          <TextAreaField
            :rows="5"
            :model-value="getStringAt(agentObj(name), ['prompt']) ?? ''"
            :placeholder="t('config.editor.agents.promptPlaceholder')"
            :label="t('config.editor.agents.prompt')"
            @update:model-value="patchPrompt(name, $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.permission')">
          <PermissionRulesEditor
            :model-value="getAt(agentObj(name), ['permission'])"
            :label="t('config.editor.agents.permission')"
            @update:model-value="patchAgent(name, ['permission'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.delay')">
          <ToolDelaysEditor
            :model-value="getAt(agentObj(name), ['delay'])"
            @update:model-value="patchAgent(name, ['delay'], $event)"
          />
        </FormRow>
      </div>
    </div>

    <div class="add-inline">
      <input
        v-model="newAgentName"
        type="text"
        class="text-input mono"
        spellcheck="false"
        :placeholder="t('config.editor.agents.agentNamePlaceholder')"
        :aria-label="t('config.editor.agents.agentNamePlaceholder')"
        @keydown.enter.prevent="!$event.isComposing && addCustomAgent()"
      />
      <DialogButton
        small
        kind="primary"
        :label="t('config.editor.agents.addAgent')"
        :disabled="
          newAgentName.trim() === '' ||
          builtinSet.has(newAgentName.trim()) ||
          Object.hasOwn(agentsMap, newAgentName.trim())
        "
        @click="addCustomAgent"
      />
    </div>
  </div>
</template>

<style scoped>
.section-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group-heading {
  margin: 0 0 2px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.group-heading.divider {
  margin-top: 12px;
  padding-top: 14px;
  border-top: 1px solid var(--border);
}

.empty {
  margin: 2px 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.card {
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
  overflow: hidden;
}

.card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
}

.expand-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  text-align: left;
}

.card-id {
  font-family: var(--font-mono);
  font-size: var(--font-size);
  font-weight: 600;
}

.card-sub {
  color: var(--text-muted);
  font-size: var(--small-font-size);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta-pill {
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--surface);
  color: var(--text-muted);
  font-size: var(--micro-font-size);
}

.custom-badge {
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  color: var(--accent-text);
  font-size: var(--micro-font-size);
  font-weight: 500;
}

.card-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border);
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
}

.text-input.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.text-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
}

.icon-btn.danger:hover {
  background: var(--surface-hover);
  color: var(--danger-text);
}

.switch-wrap {
  display: flex;
  justify-content: flex-end;
}

.add-inline {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}

.add-inline .text-input {
  flex: 1;
}
</style>
