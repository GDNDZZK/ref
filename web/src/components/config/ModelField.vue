<script setup lang="ts">
// Model reference input (`"provider/model"`): combines a configured-model dropdown (when models are
// available) with a direct monospace text input so custom or not-yet-saved model refs work seamlessly.
import { useI18n } from 'vue-i18n'
import SelectField, { type SelectOption } from './SelectField.vue'

withDefaults(
  defineProps<{
    options?: readonly SelectOption[]
    placeholder?: string
    label?: string
    disabled?: boolean
    id?: string
  }>(),
  {
    options: () => [],
    placeholder: undefined,
    label: undefined,
    disabled: false,
    id: undefined,
  },
)

const modelValue = defineModel<string>({ required: true })
const { t } = useI18n()

function onPick(chosen: string): void {
  modelValue.value = chosen
}
</script>

<template>
  <div class="model-field" :class="{ disabled }">
    <SelectField
      v-if="options.length > 0"
      :model-value="modelValue"
      :options="options"
      :placeholder="t('config.editor.pickModelPlaceholder')"
      :label="label"
      :disabled="disabled"
      monospace
      @update:model-value="onPick"
    />
    <input
      :id="id"
      v-model="modelValue"
      type="text"
      class="input"
      spellcheck="false"
      autocomplete="off"
      :disabled="disabled"
      :placeholder="placeholder ?? t('config.editor.customModelPlaceholder')"
      :aria-label="label ?? t('config.editor.customModelPlaceholder')"
    />
  </div>
</template>

<style scoped>
.model-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
}

.model-field.disabled {
  opacity: var(--disabled-opacity);
}

.input {
  width: 100%;
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

.input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}
</style>
