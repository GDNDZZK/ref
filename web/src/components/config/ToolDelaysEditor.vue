<script setup lang="ts">
// Ordered `delay` map editor (`{ "bash": 3, "delete": 5, "*": 0 }`, or scalar number normalized to `*`).
// Last matching tool-glob key wins on the backend, so row order is preserved and reorderable.
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { isPlainObject, setOwn, type JsonObject } from '../../config/jsonPatch.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import NumberField from './NumberField.vue'

interface DelayRow {
  id: number
  pattern: string
  seconds: number | undefined
}

withDefaults(defineProps<{ disabled?: boolean }>(), {
  disabled: false,
})

const delayValue = defineModel<unknown>({ required: true })
const { t } = useI18n()

let nextRowId = 1

function parseDelayRows(raw: unknown): DelayRow[] {
  if (typeof raw === 'number' && Number.isFinite(raw) && raw >= 0) {
    return [{ id: nextRowId++, pattern: '*', seconds: raw }]
  }
  if (!isPlainObject(raw)) return []
  const parsed: DelayRow[] = []
  for (const [pattern, seconds] of Object.entries(raw)) {
    if (typeof seconds === 'number' && Number.isFinite(seconds) && seconds >= 0) {
      parsed.push({ id: nextRowId++, pattern, seconds })
    }
  }
  return parsed
}

function serializeDelayRows(list: readonly DelayRow[]): JsonObject | undefined {
  const out: JsonObject = {}
  let count = 0
  for (const row of list) {
    const pattern = row.pattern.trim()
    if (pattern === '' || row.seconds === undefined || row.seconds < 0) continue
    setOwn(out, pattern, row.seconds)
    count += 1
  }
  return count > 0 ? out : undefined
}

function sameJsonValue(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a === undefined || b === undefined) return false
  return JSON.stringify(a) === JSON.stringify(b)
}

const rows = ref<DelayRow[]>(parseDelayRows(delayValue.value))
const lastEmitted = ref<unknown>(delayValue.value)

watch(
  delayValue,
  (next) => {
    if (sameJsonValue(next, lastEmitted.value)) return
    rows.value = parseDelayRows(next)
    lastEmitted.value = next
  },
  { deep: true },
)

function emitCurrent(): void {
  const serialized = serializeDelayRows(rows.value)
  if (!sameJsonValue(serialized, delayValue.value)) {
    lastEmitted.value = serialized
    delayValue.value = serialized
  }
}

function updatePattern(index: number, pattern: string): void {
  const row = rows.value[index]
  if (!row) return
  row.pattern = pattern
  emitCurrent()
}

function updateSeconds(index: number, seconds: number | undefined): void {
  const row = rows.value[index]
  if (!row) return
  row.seconds = seconds
  emitCurrent()
}

function moveRow(index: number, delta: -1 | 1): void {
  const target = index + delta
  if (target < 0 || target >= rows.value.length) return
  const current = rows.value[index]!
  rows.value[index] = rows.value[target]!
  rows.value[target] = current
  emitCurrent()
}

function removeRow(index: number): void {
  rows.value.splice(index, 1)
  emitCurrent()
}

function addRow(): void {
  rows.value.push({ id: nextRowId++, pattern: '', seconds: 3 })
}
</script>

<template>
  <div class="delays-editor" :class="{ disabled }">
    <p v-if="rows.length === 0" class="empty">{{ t('config.editor.permissions.emptyDelays') }}</p>

    <div v-for="(row, index) in rows" :key="row.id" class="delay-row">
      <input
        :value="row.pattern"
        type="text"
        class="pattern-input"
        spellcheck="false"
        autocomplete="off"
        :disabled="disabled"
        :placeholder="t('config.editor.permissions.delayToolPlaceholder')"
        :aria-label="t('config.editor.permissions.delayToolPlaceholder')"
        @input="updatePattern(index, ($event.target as HTMLInputElement).value)"
      />
      <div class="seconds-cell">
        <NumberField
          :model-value="row.seconds"
          :min="0"
          :step="0.5"
          :disabled="disabled"
          :placeholder="t('config.editor.permissions.delaySecondsPlaceholder')"
          :label="t('config.editor.permissions.delaySecondsPlaceholder')"
          @update:model-value="updateSeconds(index, $event)"
        />
      </div>
      <button
        type="button"
        class="icon-btn"
        :aria-label="t('config.moveUp')"
        :title="t('config.moveUp')"
        :disabled="disabled || index === 0"
        @click="moveRow(index, -1)"
      >
        ↑
      </button>
      <button
        type="button"
        class="icon-btn"
        :aria-label="t('config.moveDown')"
        :title="t('config.moveDown')"
        :disabled="disabled || index === rows.length - 1"
        @click="moveRow(index, 1)"
      >
        ↓
      </button>
      <button
        type="button"
        class="icon-btn danger"
        :aria-label="t('config.remove')"
        :title="t('config.remove')"
        :disabled="disabled"
        @click="removeRow(index)"
      >
        <Icon icon="close-16" />
      </button>
    </div>

    <div class="footer">
      <DialogButton
        small
        :label="t('config.editor.permissions.addDelay')"
        :disabled="disabled"
        @click="addRow"
      />
    </div>
  </div>
</template>

<style scoped>
.delays-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
}

.delays-editor.disabled {
  opacity: var(--disabled-opacity);
}

.empty {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.delay-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.pattern-input {
  flex: 1 1 55%;
  min-width: 0;
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

.pattern-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.seconds-cell {
  flex: 0 0 120px;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 28px;
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

.footer {
  display: flex;
}
</style>
