<script setup lang="ts">
// Multiline text / prompt / raw JSON editor styled to match TextField.vue.
import { useTemplateRef } from 'vue'

const DEFAULT_ROWS = 4

withDefaults(
  defineProps<{
    rows?: number
    monospace?: boolean
    placeholder?: string
    label?: string
    error?: string
    disabled?: boolean
    id?: string
  }>(),
  {
    rows: DEFAULT_ROWS,
    monospace: false,
    placeholder: undefined,
    label: undefined,
    error: undefined,
    disabled: false,
    id: undefined,
  },
)

const text = defineModel<string>({ required: true })
const emit = defineEmits<{ blur: [event: FocusEvent] }>()
const textarea = useTemplateRef<HTMLTextAreaElement>('textarea')

defineExpose({
  focus: () => textarea.value?.focus(),
})
</script>

<template>
  <div class="textarea-field" :class="{ invalid: error, disabled }">
    <textarea
      :id="id"
      ref="textarea"
      v-model="text"
      class="textarea"
      :class="{ mono: monospace }"
      :rows="rows"
      :spellcheck="!monospace"
      :placeholder="placeholder"
      :aria-label="label"
      :aria-invalid="error ? 'true' : 'false'"
      :disabled="disabled"
      @blur="emit('blur', $event)"
    />
    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.textarea-field {
  display: flex;
  flex-direction: column;
  width: 100%;
}

.textarea-field.disabled {
  opacity: var(--disabled-opacity);
}

.textarea {
  width: 100%;
  padding: 9px 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  line-height: 1.45;
  resize: vertical;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.textarea:focus,
.textarea:focus-visible {
  outline: 0;
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.invalid .textarea {
  box-shadow: inset 0 0 0 1px var(--danger);
}

.textarea.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.error {
  margin: 4px 0 0;
  color: var(--danger-text);
  font-size: var(--small-font-size);
}
</style>
