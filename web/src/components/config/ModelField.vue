<script setup lang="ts">
// Model reference input (`"provider/model"`): single editable monospace combobox combining
// configured-model suggestions (when available) with direct free-form text entry.
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
</script>

<template>
  <SelectField
    :id="id"
    v-model="modelValue"
    editable
    monospace
    :options="options"
    :placeholder="placeholder ?? t('config.editor.customModelPlaceholder')"
    :label="label ?? t('config.editor.customModelPlaceholder')"
    :disabled="disabled"
  />
</template>
