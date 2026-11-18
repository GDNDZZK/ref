<script setup lang="ts">
// Accessible native <select> styled with theme tokens and Chevron. Preserves unknown configured
// values in the option list so viewing a form never silently overwrites custom ref.json strings.
import { computed } from 'vue'
import Chevron from '../Chevron.vue'

export interface SelectOption {
  value: string
  label: string
  disabled?: boolean
}

const props = withDefaults(
  defineProps<{
    options: readonly SelectOption[]
    placeholder?: string
    disabled?: boolean
    label?: string
    id?: string
    monospace?: boolean
  }>(),
  {
    placeholder: undefined,
    disabled: false,
    label: undefined,
    id: undefined,
    monospace: false,
  },
)

const modelValue = defineModel<string>({ required: true })

const resolvedOptions = computed<SelectOption[]>(() => {
  const list: SelectOption[] = []
  const hasEmpty = props.options.some((option) => option.value === '')
  if (props.placeholder !== undefined && !hasEmpty) {
    list.push({ value: '', label: props.placeholder })
  }
  list.push(...props.options)

  const current = modelValue.value
  const hasCurrent = list.some((option) => option.value === current)
  if (current !== '' && !hasCurrent) {
    list.push({ value: current, label: current })
  }
  return list
})
</script>

<template>
  <div class="select-field" :class="{ disabled }">
    <select
      :id="id"
      v-model="modelValue"
      class="select"
      :class="{ mono: monospace, muted: modelValue === '' && placeholder !== undefined }"
      :disabled="disabled"
      :aria-label="label"
    >
      <option
        v-for="option in resolvedOptions"
        :key="option.value"
        :value="option.value"
        :disabled="option.disabled"
      >
        {{ option.label }}
      </option>
    </select>
    <Chevron :size="12" class="chevron" />
  </div>
</template>

<style scoped>
.select-field {
  position: relative;
  display: flex;
  align-items: center;
  width: 100%;
}

.select-field.disabled {
  opacity: var(--disabled-opacity);
}

.select {
  appearance: none;
  width: 100%;
  height: var(--field-height);
  padding: 0 32px 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  cursor: pointer;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.select.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.select.muted {
  color: var(--text-muted);
}

.select:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.select:disabled {
  cursor: default;
}

.chevron {
  position: absolute;
  right: 11px;
  color: var(--text-muted);
  pointer-events: none;
  rotate: 90deg;
}
</style>
