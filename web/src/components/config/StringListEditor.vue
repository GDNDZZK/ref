<script setup lang="ts">
// Editable ordered string list (instructions, command args, provider env/whitelist/blacklist) with
// stable row ids so clearing an item never deletes or shifts focus to the next row mid-edit.
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConfigSubEditorReset } from '../../config/subEditorState.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'

interface ListRow {
  id: number
  value: string
}

withDefaults(
  defineProps<{
    placeholder?: string
    addLabel?: string
    monospace?: boolean
    disabled?: boolean
  }>(),
  {
    placeholder: undefined,
    addLabel: undefined,
    monospace: false,
    disabled: false,
  },
)

const items = defineModel<string[]>({ required: true })
const { t } = useI18n()
const { resetVersion } = useConfigSubEditorReset()
const draft = ref('')

let nextRowId = 1

function buildRows(source: readonly string[]): ListRow[] {
  return source.map((value) => ({ id: nextRowId++, value }))
}

function serializeRows(list: readonly ListRow[]): string[] {
  return list.map((row) => row.value).filter((value) => value.trim() !== '')
}

function sameStringList(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false
  return a.every((entry, index) => entry === b[index])
}

const rows = ref<ListRow[]>(buildRows(items.value))

watch(
  items,
  (next) => {
    if (!sameStringList(serializeRows(rows.value), next)) {
      rows.value = buildRows(next)
    }
  },
  { deep: true },
)

if (resetVersion) {
  watch(resetVersion, () => {
    rows.value = buildRows(items.value)
    draft.value = ''
  })
}

function emitCurrent(): void {
  const serialized = serializeRows(rows.value)
  if (!sameStringList(serialized, items.value)) {
    items.value = serialized
  }
}

function updateAt(index: number, value: string): void {
  const row = rows.value[index]
  if (!row) return
  row.value = value
  emitCurrent()
}

function moveItem(index: number, delta: -1 | 1): void {
  const target = index + delta
  if (target < 0 || target >= rows.value.length) return
  const current = rows.value[index]!
  rows.value[index] = rows.value[target]!
  rows.value[target] = current
  emitCurrent()
}

function removeAt(index: number): void {
  rows.value.splice(index, 1)
  emitCurrent()
}

function addDraft(): void {
  const trimmed = draft.value.trim()
  if (trimmed === '') return
  rows.value.push({ id: nextRowId++, value: trimmed })
  draft.value = ''
  emitCurrent()
}
</script>

<template>
  <div class="string-list" :class="{ disabled }">
    <div v-for="(row, index) in rows" :key="row.id" class="item-row">
      <input
        :value="row.value"
        type="text"
        class="input"
        :class="{ mono: monospace }"
        spellcheck="false"
        autocomplete="off"
        :disabled="disabled"
        @input="updateAt(index, ($event.target as HTMLInputElement).value)"
      />
      <button
        type="button"
        class="icon-btn"
        :aria-label="t('config.moveUp')"
        :title="t('config.moveUp')"
        :disabled="disabled || index === 0"
        @click="moveItem(index, -1)"
      >
        ↑
      </button>
      <button
        type="button"
        class="icon-btn"
        :aria-label="t('config.moveDown')"
        :title="t('config.moveDown')"
        :disabled="disabled || index === rows.length - 1"
      >
        ↓
      </button>
      <button
        type="button"
        class="icon-btn danger"
        :aria-label="t('config.remove')"
        :title="t('config.remove')"
        :disabled="disabled"
        @click="removeAt(index)"
      >
        <Icon icon="close-16" />
      </button>
    </div>

    <div class="add-row">
      <input
        v-model="draft"
        type="text"
        class="input"
        :class="{ mono: monospace }"
        spellcheck="false"
        autocomplete="off"
        :disabled="disabled"
        :placeholder="placeholder ?? t('config.list.placeholder')"
        :aria-label="placeholder ?? t('config.list.placeholder')"
        @keydown.enter.prevent="!$event.isComposing && addDraft()"
      />
      <DialogButton
        small
        :label="addLabel ?? t('config.list.add')"
        :disabled="disabled || draft.trim() === ''"
        @click="addDraft"
      />
    </div>
  </div>
</template>

<style scoped>
.string-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
}

.string-list.disabled {
  opacity: var(--disabled-opacity);
}

.item-row,
.add-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.input {
  flex: 1;
  min-width: 0;
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

.input.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.icon-btn:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--text);
}

.icon-btn.danger:not(:disabled):hover {
  color: var(--danger-text);
}

.icon-btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}
</style>
