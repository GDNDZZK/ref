<script setup lang="ts">
// Commands & Skills configuration section: custom slash `command.*`, `instructions[]`, and `skills.paths[]`.
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  getAt,
  getObjectAt,
  getStringArrayAt,
  getStringAt,
  isPlainObject,
  setAt,
  type JsonObject,
} from '../../../config/jsonPatch.ts'
import {
  configuredAgentOptions,
  configuredModelOptions,
  normalizeProseInput,
  normalizeTrimmedString,
  parseSkillsPaths,
  patchSkillsPaths,
  renameObjectKey,
} from '../../../config/sectionHelpers.ts'
import { useConfigSubEditorScope, useInlineRenameDrafts } from '../../../config/subEditorState.ts'
import type { AgentInfo, ModelInfo } from '../../../proto/generated'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import { FormRow, ModelField, SelectField, StringListEditor, TextAreaField } from '../index.ts'

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

const openCommands = reactive(new Set<string>())
const newCommandName = ref('')
let lastEmittedDoc: JsonObject | undefined

const commandsMap = computed(() => getObjectAt(props.doc, ['command']) ?? {})
const commandNames = computed(() => Object.keys(commandsMap.value))
const modelOptions = computed(() => configuredModelOptions(props.doc, props.serverModels))
const agentOptions = computed(() => configuredAgentOptions(props.doc, props.serverAgents, false))

function emitDoc(next: JsonObject): void {
  lastEmittedDoc = next
  emit('update:doc', next)
}

const renames = useInlineRenameDrafts({
  normalize: (raw) => raw.trim().replace(/^\/+/, ''),
  exists: (key) => Object.hasOwn(commandsMap.value, key),
  collisionMessage: () => t('config.editor.renameExists'),
  onCommit(oldName, target) {
    const renamed = renameObjectKey(commandsMap.value, oldName, target)
    if (openCommands.has(oldName)) {
      openCommands.delete(oldName)
      openCommands.add(target)
    }
    emitDoc(setAt(props.doc, ['command'], renamed))
  },
})

const subEditorScope = useConfigSubEditorScope(
  () => ({ dirty: renames.hasDirty.value, committable: renames.hasCommittable.value, invalid: false }),
  () => renames.flushAll(),
)

watch(
  () => props.doc,
  (nextDoc) => {
    if (nextDoc === lastEmittedDoc) return
    renames.reconcileKeys(commandNames.value, !subEditorScope.hasHost)
  },
)

if (subEditorScope.resetVersion) {
  watch(subEditorScope.resetVersion, () => renames.clearAll())
}
function commandObj(name: string): JsonObject {
  const raw = commandsMap.value[name]
  return isPlainObject(raw) ? raw : {}
}

function toggleCommand(name: string): void {
  if (openCommands.has(name)) openCommands.delete(name)
  else openCommands.add(name)
}

function addCommand(): void {
  const cleaned = newCommandName.value.trim().replace(/^\/+/, '')
  if (cleaned === '' || Object.hasOwn(commandsMap.value, cleaned)) return
  newCommandName.value = ''
  openCommands.add(cleaned)
  emitDoc(setAt(props.doc, ['command', cleaned], { description: '', template: '$ARGUMENTS' }))
}

function removeCommand(name: string): void {
  openCommands.delete(name)
  renames.remove(name)
  emitDoc(setAt(props.doc, ['command', name], undefined))
}

function patchCommand(name: string, key: string, value: unknown): void {
  const nextCommand = setAt(commandObj(name), [key], value)
  emitDoc(setAt(props.doc, ['command', name], nextCommand))
}

function patchCommandProse(name: string, key: string, value: string): void {
  patchCommand(name, key, normalizeProseInput(value))
}

function patchCommandString(name: string, key: string, value: string): void {
  patchCommand(name, key, normalizeTrimmedString(value))
}
function readInstructions(): string[] {
  const raw = getAt(props.doc, ['instructions'])
  if (typeof raw === 'string') return raw.trim() ? [raw] : []
  return getStringArrayAt(props.doc, ['instructions'])
}

function writeInstructions(items: readonly string[]): void {
  const cleaned = items.filter((entry) => entry.trim() !== '')
  emitDoc(setAt(props.doc, ['instructions'], cleaned.length > 0 ? cleaned : undefined))
}

function writeSkills(items: readonly string[]): void {
  emitDoc(patchSkillsPaths(props.doc, items))
}
</script>

<template>
  <div class="section-body">
    <h3 class="group-heading">{{ t('config.editor.commands.commandsHeading') }}</h3>

    <p v-if="commandNames.length === 0" class="empty">{{ t('config.editor.commands.commandsEmpty') }}</p>

    <div v-for="name in commandNames" :key="name" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleCommand(name)">
          <Chevron :open="openCommands.has(name)" :size="12" />
          <span class="card-id">/{{ name }}</span>
          <span v-if="getStringAt(commandObj(name), ['description'])" class="card-sub">
            {{ getStringAt(commandObj(name), ['description']) }}
          </span>
        </button>
        <button
          type="button"
          class="icon-btn danger"
          :aria-label="t('config.editor.delete')"
          :title="t('config.editor.delete')"
          @click="removeCommand(name)"
        >
          <Icon icon="trash-16" />
        </button>
      </div>

      <div v-if="openCommands.has(name)" class="card-body">
        <FormRow :label="t('config.editor.rename')" :error="renames.errorOf(name)">
          <div class="rename-row">
            <input
              :value="renames.read(name)"
              type="text"
              class="text-input mono"
              spellcheck="false"
              @input="renames.set(name, ($event.target as HTMLInputElement).value)"
              @blur="renames.blur(name)"
              @keydown.enter.prevent="!$event.isComposing && renames.commit(name)"
            />
            <DialogButton
              small
              :label="t('config.editor.rename')"
              :disabled="!renames.canCommit(name)"
              @mousedown.prevent
              @click="renames.commit(name)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.commands.description')">
          <input
            :value="getStringAt(commandObj(name), ['description']) ?? ''"
            type="text"
            class="text-input"
            @input="patchCommandProse(name, 'description', ($event.target as HTMLInputElement).value)"
            @blur="patchCommandString(name, 'description', ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.commands.agent')"
          :can-reset="getAt(commandObj(name), ['agent']) !== undefined"
          @reset="patchCommand(name, 'agent', undefined)"
        >
          <SelectField
            :model-value="getStringAt(commandObj(name), ['agent']) ?? ''"
            :options="agentOptions"
            :placeholder="t('config.editor.triDefault')"
            :label="t('config.editor.commands.agent')"
            @update:model-value="patchCommandString(name, 'agent', $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.commands.model')"
          :can-reset="getAt(commandObj(name), ['model']) !== undefined"
          @reset="patchCommand(name, 'model', undefined)"
        >
          <ModelField
            :model-value="getStringAt(commandObj(name), ['model']) ?? ''"
            :options="modelOptions"
            :label="t('config.editor.commands.model')"
            @update:model-value="patchCommandString(name, 'model', $event)"
          />
        </FormRow>

        <FormRow
          vertical
          :label="t('config.editor.commands.template')"
          :description="t('config.editor.commands.templateDesc')"
        >
          <TextAreaField
            monospace
            :rows="5"
            :model-value="getStringAt(commandObj(name), ['template']) ?? ''"
            placeholder="Review the changes: $ARGUMENTS"
            :label="t('config.editor.commands.template')"
            @update:model-value="patchCommand(name, 'template', $event)"
          />
        </FormRow>
      </div>
    </div>

    <div class="add-inline">
      <input
        v-model="newCommandName"
        type="text"
        class="text-input mono"
        spellcheck="false"
        :placeholder="t('config.editor.commands.commandNamePlaceholder')"
        :aria-label="t('config.editor.commands.commandNamePlaceholder')"
        @keydown.enter.prevent="!$event.isComposing && addCommand()"
      />
      <DialogButton
        small
        kind="primary"
        :label="t('config.editor.commands.addCommand')"
        :disabled="
          newCommandName.trim().replace(/^\/+/, '') === '' ||
          Object.hasOwn(commandsMap, newCommandName.trim().replace(/^\/+/, ''))
        "
        @click="addCommand"
      />
    </div>

    <FormRow
      vertical
      class="divider"
      :label="t('config.editor.commands.instructionsHeading')"
      :description="t('config.editor.commands.instructionsDesc')"
    >
      <StringListEditor
        monospace
        :model-value="readInstructions()"
        :placeholder="t('config.editor.commands.instructionsPlaceholder')"
        @update:model-value="writeInstructions"
      />
    </FormRow>

    <FormRow
      vertical
      class="divider"
      :label="t('config.editor.commands.skillsHeading')"
      :description="t('config.editor.commands.skillsDesc')"
    >
      <StringListEditor
        monospace
        :model-value="parseSkillsPaths(doc)"
        :placeholder="t('config.editor.commands.skillsPlaceholder')"
        @update:model-value="writeSkills"
      />
    </FormRow>
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

.divider {
  margin-top: 10px;
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

.card-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border);
}

.rename-row {
  display: flex;
  align-items: center;
  gap: 8px;
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
