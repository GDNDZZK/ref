<script setup lang="ts">
// Editable ordered string list (instructions, command args, provider env/whitelist/blacklist) with
// inline editing, ↑/↓ reordering, item removal, and Enter / button append.
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'

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
const draft = ref('')

function updateAt(index: number, value: string): void {
  const next = [...items.value]
  next[index] = value
  items.value = next
}

function moveItem(index: number, delta: -1 | 1): void {
  const target = index + delta
  if (target < 0 || target >= items.value.length) return
  const next = [...items.value]
  const current = next[index]!
  next[index] = next[target]!
  next[target] = current
  items.value = next
}

function removeAt(index: number): void {
  items.value = items.value.filter((_, itemIndex) => itemIndex !== index)
}

function addDraft(): void {
  const trimmed = draft.value.trim()
  if (trimmed === '') return
  items.value = [...items.value, trimmed]
  draft.value = ''
}
</script>

<template>
  <div class="string-list" :class="{ disabled }">
    <div v-for="(item, index) in items" :key="index" class="item-row">
      <input
        :value="item"
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
        :disabled="disabled || index === items.length - 1"
        @click="moveItem(index, 1)"
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
