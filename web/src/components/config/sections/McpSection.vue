<script setup lang="ts">
// MCP Servers configuration section (`mcp.*`): Local (`command[]`, paste-command splitter, `cwd`,
// `environment`) and Remote (`url`, `headers`), plus `enabled` and `timeout`.
import { computed, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  getAt,
  getBooleanAt,
  getNumberAt,
  getObjectAt,
  getStringArrayAt,
  getStringAt,
  getStringMapAt,
  isPlainObject,
  setAt,
  type JsonObject,
} from '../../../config/jsonPatch.ts'
import { renameObjectKey, splitShellWords, uniqueObjectKey } from '../../../config/sectionHelpers.ts'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import Segmented from '../../Segmented.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import { FormRow, KeyValueEditor, NumberField, StringListEditor } from '../index.ts'

type McpKind = 'local' | 'remote'

const props = defineProps<{
  doc: JsonObject
  envStatus?: Readonly<Record<string, boolean>>
}>()

const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

const openServers = reactive(new Set<string>())
const pasteLines = reactive<Record<string, string>>({})
const renameDrafts = reactive<Record<string, string>>({})
const addingKind = ref<McpKind | null>(null)
const newServerName = ref('')

const mcpMap = computed(() => getObjectAt(props.doc, ['mcp']) ?? {})
const serverNames = computed(() => Object.keys(mcpMap.value))

const kindOptions = computed(() => [
  { value: 'local' as const, label: t('config.editor.mcp.typeLocal') },
  { value: 'remote' as const, label: t('config.editor.mcp.typeRemote') },
])

function serverObj(name: string): JsonObject {
  const raw = mcpMap.value[name]
  return isPlainObject(raw) ? raw : {}
}

function kindOf(name: string): McpKind {
  const obj = serverObj(name)
  const rawType = getStringAt(obj, ['type']) ?? ''
  if (rawType === 'remote' || rawType === 'http' || rawType === 'sse' || rawType === 'streamable-http') {
    return 'remote'
  }
  if (rawType === 'local' || rawType === 'stdio') return 'local'
  if (getAt(obj, ['url']) !== undefined) return 'remote'
  return 'local'
}

function startAdd(kind: McpKind): void {
  addingKind.value = kind
  newServerName.value = uniqueObjectKey(mcpMap.value, kind === 'local' ? 'local-mcp' : 'remote-mcp')
}

function confirmAdd(): void {
  const kind = addingKind.value
  const name = newServerName.value.trim()
  if (!kind || name === '' || Object.hasOwn(mcpMap.value, name)) return
  addingKind.value = null
  newServerName.value = ''
  openServers.add(name)
  const initial: JsonObject =
    kind === 'local' ? { type: 'local', command: ['npx'] } : { type: 'remote', url: 'https://' }
  emit('update:doc', setAt(props.doc, ['mcp', name], initial))
}

function toggleServer(name: string): void {
  if (openServers.has(name)) openServers.delete(name)
  else openServers.add(name)
}

function removeServer(name: string): void {
  openServers.delete(name)
  delete pasteLines[name]
  delete renameDrafts[name]
  emit('update:doc', setAt(props.doc, ['mcp', name], undefined))
}

function applyRename(oldName: string): void {
  const target = (renameDrafts[oldName] ?? oldName).trim()
  if (target === '' || target === oldName || Object.hasOwn(mcpMap.value, target)) return
  const renamed = renameObjectKey(mcpMap.value, oldName, target)
  delete renameDrafts[oldName]
  if (openServers.has(oldName)) {
    openServers.delete(oldName)
    openServers.add(target)
  }
  emit('update:doc', setAt(props.doc, ['mcp'], renamed))
}

function patchServer(name: string, subPath: readonly string[], value: unknown): void {
  emit('update:doc', setAt(props.doc, ['mcp', name, ...subPath], value))
}

function patchServerString(name: string, subPath: readonly string[], value: string): void {
  const trimmed = value.trim()
  patchServer(name, subPath, trimmed === '' ? undefined : trimmed)
}

function switchKind(name: string, nextKind: McpKind): void {
  patchServer(name, ['type'], nextKind)
}

function readCommandList(name: string): string[] {
  const obj = serverObj(name)
  const raw = getAt(obj, ['command'])
  if (typeof raw === 'string') {
    const base = raw.trim() ? raw.trim().split(/\s+/) : []
    return [...base, ...getStringArrayAt(obj, ['args'])]
  }
  return getStringArrayAt(obj, ['command'])
}

function writeCommandList(name: string, items: readonly string[]): void {
  let next = setAt(props.doc, ['mcp', name, 'args'], undefined)
  next = setAt(next, ['mcp', name, 'command'], [...items])
  emit('update:doc', next)
}

function applyPastedCommand(name: string): void {
  const raw = (pasteLines[name] ?? '').trim()
  if (raw === '') return
  const argv = splitShellWords(raw)
  if (argv.length === 0) return
  pasteLines[name] = ''
  writeCommandList(name, argv)
}

function readEnvironment(name: string): Record<string, string> {
  const obj = serverObj(name)
  if (getObjectAt(obj, ['environment'])) return getStringMapAt(obj, ['environment'])
  return getStringMapAt(obj, ['env'])
}

function writeEnvironment(name: string, map: Record<string, string>): void {
  let next = setAt(props.doc, ['mcp', name, 'env'], undefined)
  next = setAt(next, ['mcp', name, 'environment'], Object.keys(map).length > 0 ? map : undefined)
  emit('update:doc', next)
}

function writeHeaders(name: string, map: Record<string, string>): void {
  patchServer(name, ['headers'], Object.keys(map).length > 0 ? map : undefined)
}
</script>

<template>
  <div class="section-body">
    <div class="top-actions">
      <DialogButton
        small
        kind="primary"
        :label="`+ ${t('config.editor.mcp.addLocal')}`"
        @click="startAdd('local')"
      />
      <DialogButton small :label="`+ ${t('config.editor.mcp.addRemote')}`" @click="startAdd('remote')" />
    </div>

    <div v-if="addingKind" class="add-panel">
      <FormRow :label="t('config.editor.mcp.serverNamePlaceholder')">
        <input
          v-model="newServerName"
          type="text"
          class="text-input mono"
          spellcheck="false"
          :placeholder="t('config.editor.mcp.serverNamePlaceholder')"
          @keydown.enter.prevent="!$event.isComposing && confirmAdd()"
        />
      </FormRow>
      <div class="add-panel-footer">
        <DialogButton small :label="t('dialog.cancel')" @click="addingKind = null" />
        <DialogButton
          small
          kind="primary"
          :label="t('config.editor.create')"
          :disabled="newServerName.trim() === '' || Object.hasOwn(mcpMap, newServerName.trim())"
          @click="confirmAdd"
        />
      </div>
    </div>

    <p v-if="serverNames.length === 0 && !addingKind" class="empty">{{ t('config.editor.mcp.empty') }}</p>

    <div v-for="name in serverNames" :key="name" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleServer(name)">
          <Chevron :open="openServers.has(name)" :size="12" />
          <span class="card-id">{{ name }}</span>
          <span class="meta-pill">{{ kindOf(name) }}</span>
        </button>
        <div class="head-right">
          <ToggleSwitch
            :model-value="getBooleanAt(serverObj(name), ['enabled']) ?? true"
            :label="t('config.editor.mcp.enabled')"
            @update:model-value="patchServer(name, ['enabled'], $event)"
          />
          <button
            type="button"
            class="icon-btn danger"
            :aria-label="t('config.editor.delete')"
            :title="t('config.editor.delete')"
            @click="removeServer(name)"
          >
            <Icon icon="trash-16" />
          </button>
        </div>
      </div>

      <div v-if="openServers.has(name)" class="card-body">
        <FormRow :label="t('config.editor.rename')">
          <div class="rename-row">
            <input
              :value="renameDrafts[name] ?? name"
              type="text"
              class="text-input mono"
              spellcheck="false"
              @input="renameDrafts[name] = ($event.target as HTMLInputElement).value"
              @keydown.enter.prevent="!$event.isComposing && applyRename(name)"
            />
            <DialogButton
              v-if="(renameDrafts[name] ?? name).trim() !== name"
              small
              :label="t('config.editor.rename')"
              @click="applyRename(name)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.mcp.type')">
          <Segmented
            :model-value="kindOf(name)"
            :options="kindOptions"
            :label="t('config.editor.mcp.type')"
            @update:model-value="switchKind(name, $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.mcp.timeout')">
          <NumberField
            :model-value="getNumberAt(serverObj(name), ['timeout'])"
            :min="1"
            :step="1000"
            placeholder="30000"
            @update:model-value="patchServer(name, ['timeout'], $event)"
          />
        </FormRow>

        <template v-if="kindOf(name) === 'local'">
          <FormRow vertical :label="t('config.editor.mcp.pasteCommand')">
            <div class="paste-row">
              <input
                :value="pasteLines[name] ?? ''"
                type="text"
                class="text-input mono"
                spellcheck="false"
                :placeholder="t('config.editor.mcp.pasteCommandPlaceholder')"
                @input="pasteLines[name] = ($event.target as HTMLInputElement).value"
                @keydown.enter.prevent="!$event.isComposing && applyPastedCommand(name)"
              />
              <DialogButton
                small
                :label="t('config.editor.mcp.splitCommand')"
                :disabled="!(pasteLines[name] ?? '').trim()"
                @click="applyPastedCommand(name)"
              />
            </div>
          </FormRow>

          <FormRow vertical :label="t('config.editor.mcp.command')">
            <StringListEditor
              monospace
              :model-value="readCommandList(name)"
              :placeholder="t('config.editor.mcp.commandPlaceholder')"
              @update:model-value="writeCommandList(name, $event)"
            />
          </FormRow>

          <FormRow :label="t('config.editor.mcp.cwd')">
            <input
              :value="getStringAt(serverObj(name), ['cwd']) ?? ''"
              type="text"
              class="text-input mono"
              spellcheck="false"
              placeholder="/path/to/dir"
              @input="patchServerString(name, ['cwd'], ($event.target as HTMLInputElement).value)"
            />
          </FormRow>

          <FormRow vertical :label="t('config.editor.mcp.environment')">
            <KeyValueEditor
              secret-values
              :model-value="readEnvironment(name)"
              :env-status="envStatus"
              @update:model-value="writeEnvironment(name, $event)"
            />
          </FormRow>
        </template>

        <template v-else>
          <FormRow :label="t('config.editor.mcp.url')">
            <input
              :value="getStringAt(serverObj(name), ['url']) ?? ''"
              type="text"
              class="text-input mono"
              spellcheck="false"
              placeholder="https://mcp.example.com/mcp"
              @input="patchServerString(name, ['url'], ($event.target as HTMLInputElement).value)"
            />
          </FormRow>

          <FormRow vertical :label="t('config.editor.mcp.headers')">
            <KeyValueEditor
              :model-value="getStringMapAt(serverObj(name), ['headers'])"
              :env-status="envStatus"
              @update:model-value="writeHeaders(name, $event)"
            />
          </FormRow>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.section-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.top-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.add-panel {
  padding: 12px 14px;
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
}

.add-panel-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 8px;
}

.empty {
  margin: 4px 0;
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
  gap: 8px;
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

.head-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.card-id {
  font-family: var(--font-mono);
  font-size: var(--font-size);
  font-weight: 600;
}

.meta-pill {
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--surface);
  color: var(--text-muted);
  font-size: var(--micro-font-size);
}

.card-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border);
}

.rename-row,
.paste-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.paste-row .text-input {
  flex: 1;
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
</style>
