<script setup lang="ts">
// Lossless editor for ref.json `permission` values (`undefined | string | Record<string, unknown>`).
// Supports Default (unset), Global action (`allow` | `ask` | `deny`), and an ordered Rule list.
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  parsePermissions,
  serializePermissions,
  type PermissionAction,
  type PermissionRule,
} from '../../config/jsonPatch.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import Segmented from '../Segmented.vue'
import SelectField from './SelectField.vue'

type UiMode = 'unset' | 'global' | 'rules'

interface RuleRow extends PermissionRule {
  id: number
}

withDefaults(defineProps<{ disabled?: boolean; label?: string }>(), {
  disabled: false,
  label: undefined,
})

const permission = defineModel<unknown>({ required: true })
const { t } = useI18n()

let nextRuleId = 1

function toRuleRows(rules: readonly PermissionRule[]): RuleRow[] {
  return rules.map((rule) => ({ ...rule, id: nextRuleId++ }))
}

function deriveInitialState(raw: unknown): {
  mode: UiMode
  globalAction: PermissionAction
  rows: RuleRow[]
} {
  if (raw === undefined || raw === null) {
    return { mode: 'unset', globalAction: 'allow', rows: [] }
  }
  const parsed = parsePermissions(raw)
  if (parsed.mode === 'global') {
    return { mode: 'global', globalAction: parsed.globalAction, rows: [] }
  }
  return { mode: 'rules', globalAction: parsed.globalAction, rows: toRuleRows(parsed.rules) }
}

function sameJsonValue(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a === undefined || b === undefined) return false
  return JSON.stringify(a) === JSON.stringify(b)
}

const initial = deriveInitialState(permission.value)
const uiMode = ref<UiMode>(initial.mode)
const globalAction = ref<PermissionAction>(initial.globalAction)
const rows = ref<RuleRow[]>(initial.rows)
const lastEmitted = ref<unknown>(permission.value)

watch(
  permission,
  (next) => {
    if (sameJsonValue(next, lastEmitted.value)) return
    const state = deriveInitialState(next)
    uiMode.value = state.mode
    globalAction.value = state.globalAction
    rows.value = state.rows
    lastEmitted.value = next
  },
  { deep: true },
)

const modeOptions = computed(() => [
  { value: 'unset' as const, label: t('config.permission.modeUnset') },
  { value: 'global' as const, label: t('config.permission.modeGlobal') },
  { value: 'rules' as const, label: t('config.permission.modeRules') },
])

const actionOptions = computed(() => [
  { value: 'allow', label: t('config.permission.allow') },
  { value: 'ask', label: t('config.permission.ask') },
  { value: 'deny', label: t('config.permission.deny') },
])

function emitValue(next: unknown): void {
  lastEmitted.value = next
  permission.value = next
}

function emitRules(): void {
  const serialized = serializePermissions({
    mode: 'rules',
    globalAction: globalAction.value,
    rules: rows.value,
  })
  emitValue(serialized)
}

function setMode(nextMode: UiMode): void {
  const previousMode = uiMode.value
  uiMode.value = nextMode
  if (nextMode === 'unset') {
    emitValue(undefined)
    return
  }
  if (nextMode === 'global') {
    emitValue(globalAction.value)
    return
  }
  if (previousMode === 'global' && rows.value.length === 0) {
    rows.value = [{ id: nextRuleId++, tool: '*', pattern: '*', action: globalAction.value }]
  }
  emitRules()
}

function setGlobalAction(value: string): void {
  const next = (
    value === 'allow' || value === 'ask' || value === 'deny' ? value : 'allow'
  ) as PermissionAction
  globalAction.value = next
  if (uiMode.value === 'global') emitValue(next)
}

function addRule(): void {
  rows.value.push({ id: nextRuleId++, tool: '*', pattern: '*', action: 'ask' })
  emitRules()
}

function updateRuleTool(index: number, tool: string): void {
  const row = rows.value[index]
  if (!row) return
  row.tool = tool
  emitRules()
}

function updateRulePattern(index: number, pattern: string): void {
  const row = rows.value[index]
  if (!row) return
  row.pattern = pattern
  emitRules()
}

function updateRuleAction(index: number, value: string): void {
  const row = rows.value[index]
  if (!row) return
  if (value === 'allow' || value === 'ask' || value === 'deny') {
    row.action = value
    emitRules()
  }
}

function moveRule(index: number, delta: -1 | 1): void {
  const target = index + delta
  if (target < 0 || target >= rows.value.length) return
  const current = rows.value[index]!
  rows.value[index] = rows.value[target]!
  rows.value[target] = current
  emitRules()
}

function removeRule(index: number): void {
  rows.value.splice(index, 1)
  emitRules()
}
</script>

<template>
  <div class="permission-editor" :class="{ disabled }">
    <div class="toolbar">
      <Segmented
        :model-value="uiMode"
        :options="modeOptions"
        :label="label ?? t('config.permission.modeLabel')"
        @update:model-value="setMode"
      />
    </div>

    <div v-if="uiMode === 'global'" class="global-panel">
      <SelectField
        :model-value="globalAction"
        :options="actionOptions"
        :label="t('config.permission.globalAction')"
        :disabled="disabled"
        @update:model-value="setGlobalAction"
      />
    </div>

    <div v-else-if="uiMode === 'rules'" class="rules-panel">
      <p v-if="rows.length === 0" class="empty">{{ t('config.permission.emptyRules') }}</p>

      <div v-for="(row, index) in rows" :key="row.id" class="rule-row">
        <input
          :value="row.tool"
          type="text"
          class="rule-input tool-input"
          spellcheck="false"
          autocomplete="off"
          :disabled="disabled"
          :placeholder="t('config.permission.toolPlaceholder')"
          :aria-label="t('config.permission.toolPlaceholder')"
          @input="updateRuleTool(index, ($event.target as HTMLInputElement).value)"
        />
        <input
          :value="row.pattern"
          type="text"
          class="rule-input pattern-input"
          spellcheck="false"
          autocomplete="off"
          :disabled="disabled"
          :placeholder="t('config.permission.patternPlaceholder')"
          :aria-label="t('config.permission.patternPlaceholder')"
          @input="updateRulePattern(index, ($event.target as HTMLInputElement).value)"
        />
        <div class="action-select">
          <SelectField
            :model-value="row.action"
            :options="actionOptions"
            :label="t('config.permission.globalAction')"
            :disabled="disabled"
            @update:model-value="updateRuleAction(index, $event)"
          />
        </div>
        <button
          type="button"
          class="icon-btn"
          :aria-label="t('config.moveUp')"
          :title="t('config.moveUp')"
          :disabled="disabled || index === 0"
          @click="moveRule(index, -1)"
        >
          ↑
        </button>
        <button
          type="button"
          class="icon-btn"
          :aria-label="t('config.moveDown')"
          :title="t('config.moveDown')"
          :disabled="disabled || index === rows.length - 1"
          @click="moveRule(index, 1)"
        >
          ↓
        </button>
        <button
          type="button"
          class="icon-btn danger"
          :aria-label="t('config.remove')"
          :title="t('config.remove')"
          :disabled="disabled"
          @click="removeRule(index)"
        >
          <Icon icon="close-16" />
        </button>
      </div>

      <div class="rules-footer">
        <DialogButton small :label="t('config.permission.addRule')" :disabled="disabled" @click="addRule" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.permission-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
}

.permission-editor.disabled {
  opacity: var(--disabled-opacity);
  pointer-events: none;
}

.toolbar {
  display: flex;
  align-items: center;
}

.global-panel {
  max-width: 240px;
}

.rules-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.empty {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.rule-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.rule-input {
  height: var(--field-height);
  padding: 0 10px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
  min-width: 0;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.rule-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.tool-input {
  flex: 1 1 28%;
}

.pattern-input {
  flex: 1 1 38%;
}

.action-select {
  flex: 0 0 112px;
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

.rules-footer {
  display: flex;
}
</style>
