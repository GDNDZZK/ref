<script setup lang="ts">
// Optional numeric input: clearing the field emits `undefined` so `setAt` removes unset keys from
// ref.json cleanly; valid finite numeric strings emit `number`.
import { ref, watch } from 'vue'

withDefaults(
  defineProps<{
    min?: number
    max?: number
    step?: number
    placeholder?: string
    label?: string
    disabled?: boolean
    id?: string
  }>(),
  {
    min: undefined,
    max: undefined,
    step: undefined,
    placeholder: undefined,
    label: undefined,
    disabled: false,
    id: undefined,
  },
)

const modelValue = defineModel<number | undefined>({ required: true })

const rawText = ref(modelValue.value === undefined ? '' : String(modelValue.value))

watch(modelValue, (next) => {
  if (next === undefined) {
    rawText.value = ''
    return
  }
  const currentTrimmed = rawText.value.trim()
  if (currentTrimmed === '' || Number(currentTrimmed) !== next) {
    rawText.value = String(next)
  }
})

function onInput(event: Event): void {
  const nextRaw = (event.target as HTMLInputElement).value
  rawText.value = nextRaw
  const trimmed = nextRaw.trim()
  if (trimmed === '') {
    modelValue.value = undefined
    return
  }
  const parsed = Number(trimmed)
  if (Number.isFinite(parsed)) {
    modelValue.value = parsed
  }
}

function onBlur(): void {
  rawText.value = modelValue.value === undefined ? '' : String(modelValue.value)
}
</script>

<template>
  <input
    :id="id"
    :value="rawText"
    type="number"
    class="number-field"
    :min="min"
    :max="max"
    :step="step ?? 'any'"
    :placeholder="placeholder"
    :aria-label="label"
    :disabled="disabled"
    @input="onInput"
    @blur="onBlur"
  />
</template>

<style scoped>
.number-field {
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
  font-variant-numeric: tabular-nums;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.number-field:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.number-field:disabled {
  opacity: var(--disabled-opacity);
}
</style>
