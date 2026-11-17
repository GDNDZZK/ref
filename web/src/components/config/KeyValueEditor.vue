<script setup lang="ts">
// Editable key-value map (`headers`, `environment`, etc.). Keeps stable per-row ids so typing into
// key inputs never remounts or drops focus, and delegates values to SecretField for `{env:VAR}`
// badges and automatic masking on secret maps / Authorization headers.
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { setOwn } from '../../config/jsonPatch.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import SecretField from './SecretField.vue'

interface KvRow {
  id: number
  key: string
  value: string
}

const SENSITIVE_HEADER_KEY = /^(authorization|proxy-authorization|x-api-key|api-key)$/i

withDefaults(
  defineProps<{
    keyPlaceholder?: string
    valuePlaceholder?: string
    secretValues?: boolean
    envStatus?: Readonly<Record<string, boolean>>
    disabled?: boolean
  }>(),
  {
    keyPlaceholder: undefined,
    valuePlaceholder: undefined,
    secretValues: false,
    envStatus: undefined,
    disabled: false,
  },
)

const entries = defineModel<Record<string, string>>({ required: true })
const { t } = useI18n()

let nextRowId = 1

function buildRows(source: Record<string, string>): KvRow[] {
  return Object.entries(source).map(([key, value]) => ({
    id: nextRowId++,
    key,
    value,
  }))
}

function serializeRows(list: readonly KvRow[]): Record<string, string> {
  const out: Record<string, string> = {}
  for (const row of list) {
    const trimmedKey = row.key.trim()
    if (trimmedKey !== '') setOwn(out, trimmedKey, row.value)
  }
  return out
}

function sameStringMap(a: Record<string, string>, b: Record<string, string>): boolean {
  const keysA = Object.keys(a)
  const keysB = Object.keys(b)
  if (keysA.length !== keysB.length) return false
  return keysA.every((key, index) => key === keysB[index] && a[key] === b[key])
}

const rows = ref<KvRow[]>(buildRows(entries.value))

watch(
  entries,
  (next) => {
    if (!sameStringMap(serializeRows(rows.value), next)) {
      rows.value = buildRows(next)
    }
  },
  { deep: true },
)

function emitCurrent(): void {
  const serialized = serializeRows(rows.value)
  if (!sameStringMap(serialized, entries.value)) {
    entries.value = serialized
  }
}

function updateKey(index: number, key: string): void {
  const row = rows.value[index]
  if (!row) return
  row.key = key
  emitCurrent()
}

function updateValue(index: number, value: string): void {
  const row = rows.value[index]
  if (!row) return
  row.value = value
  emitCurrent()
}

function removeRow(index: number): void {
  rows.value.splice(index, 1)
  emitCurrent()
}

function addRow(): void {
  rows.value.push({ id: nextRowId++, key: '', value: '' })
}

function shouldMaskRow(key: string, secretValues: boolean): boolean {
  return secretValues || SENSITIVE_HEADER_KEY.test(key.trim())
}
</script>

<template>
  <div class="kv-editor" :class="{ disabled }">
    <div v-for="(row, index) in rows" :key="row.id" class="kv-row">
      <input
        :value="row.key"
        type="text"
        class="key-input"
        spellcheck="false"
        autocomplete="off"
        :disabled="disabled"
        :placeholder="keyPlaceholder ?? t('config.kv.keyPlaceholder')"
        :aria-label="keyPlaceholder ?? t('config.kv.keyPlaceholder')"
        @input="updateKey(index, ($event.target as HTMLInputElement).value)"
      />
      <div class="value-cell">
        <SecretField
          :model-value="row.value"
          :mask-literal="shouldMaskRow(row.key, secretValues)"
          :env-status="envStatus"
          :disabled="disabled"
          :placeholder="valuePlaceholder ?? t('config.kv.valuePlaceholder')"
          :label="valuePlaceholder ?? t('config.kv.valuePlaceholder')"
          @update:model-value="updateValue(index, $event)"
        />
      </div>
      <button
        type="button"
        class="remove-btn"
        :aria-label="t('config.remove')"
        :title="t('config.remove')"
        :disabled="disabled"
        @click="removeRow(index)"
      >
        <Icon icon="close-16" />
      </button>
    </div>

    <div class="footer">
      <DialogButton small :label="t('config.kv.add')" :disabled="disabled" @click="addRow" />
    </div>
  </div>
</template>

<style scoped>
.kv-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
}

.kv-editor.disabled {
  opacity: var(--disabled-opacity);
}

.kv-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.key-input {
  flex: 0 0 36%;
  min-width: 120px;
  height: var(--field-height);
  padding: 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.key-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.value-cell {
  flex: 1;
  min-width: 0;
}

.remove-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 30px;
  height: var(--field-height);
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.remove-btn:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--danger-text);
}

.remove-btn:disabled {
  cursor: default;
}

.footer {
  display: flex;
}
</style>
