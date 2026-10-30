<script setup lang="ts">
// A text field of a dialog (TextField.qml), with an optional error line under it.
import { useTemplateRef } from 'vue'

defineProps<{ label: string; placeholder?: string; error?: string }>()
const text = defineModel<string>({ required: true })
const emit = defineEmits<{ enter: []; keydown: [event: KeyboardEvent] }>()

const input = useTemplateRef<HTMLInputElement>('input')

defineExpose({
  focus: () => input.value?.focus(),
  select: () => {
    input.value?.focus()
    input.value?.select()
  },
})
</script>

<template>
  <div class="field" :class="{ invalid: error }">
    <input
      ref="input"
      v-model="text"
      type="text"
      class="input"
      spellcheck="false"
      autocomplete="off"
      :aria-label="label"
      :aria-invalid="error ? 'true' : 'false'"
      :placeholder="placeholder"
      @keydown="emit('keydown', $event)"
      @keydown.enter.prevent="!$event.isComposing && emit('enter')"
    />
    <p class="error" :class="{ shown: error }" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
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
  font: inherit;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.input:focus {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.invalid .input {
  box-shadow: inset 0 0 0 1px var(--danger);
}

.error {
  margin: 0;
  height: 0;
  overflow: hidden;
  color: var(--danger);
  font-size: var(--small-font-size);
  opacity: 0;
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    height var(--motion-standard) var(--motion-ease);
}

.error.shown {
  height: 22px;
  padding-top: 4px;
  opacity: 1;
}
</style>
