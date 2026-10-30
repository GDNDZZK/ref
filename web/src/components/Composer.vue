<script setup lang="ts">
// Message input (Composer.qml). Enter sends, Shift+Enter inserts a line break.
// While a reply is running, the send button turns into stop.
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { components } from '../theme/generated/components.ts'
import SendButton from './SendButton.vue'

defineProps<{ busy: boolean }>()
const emit = defineEmits<{ submitted: [text: string]; stopRequested: [] }>()

const { padding: PADDING, maxInputHeight: MAX_INPUT_HEIGHT } = components.composer
const TEXT_LEFT_EXTRA = 4
const SEND_BUTTON_SIZE = 32

const { t } = useI18n()
const text = ref('')
const inputHeight = ref(SEND_BUTTON_SIZE)
const input = useTemplateRef<HTMLTextAreaElement>('input')
const root = useTemplateRef<HTMLElement>('root')
const canSend = computed(() => text.value.trim() !== '')

function measure(): void {
  const area = input.value
  if (!area) return
  area.style.height = '0'
  const wanted = Math.max(SEND_BUTTON_SIZE, area.scrollHeight)
  area.style.height = ''
  inputHeight.value = Math.min(wanted, MAX_INPUT_HEIGHT)
}

function submit(): void {
  if (!canSend.value) return
  emit('submitted', text.value)
  text.value = ''
  requestAnimationFrame(measure)
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
  event.preventDefault()
  submit()
}

// Wrapping depends on the width, so a resize can change the height wanted.
const resizeObserver = new ResizeObserver(measure)
onMounted(() => root.value && resizeObserver.observe(root.value))
onBeforeUnmount(() => resizeObserver.disconnect())

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div
    ref="root"
    class="composer"
    :style="{ height: `${inputHeight + 2 * PADDING}px`, padding: `${PADDING}px` }"
  >
    <textarea
      ref="input"
      v-model="text"
      class="input"
      rows="1"
      :placeholder="t('composer.placeholder')"
      :style="{ marginLeft: `${TEXT_LEFT_EXTRA}px` }"
      @input="measure"
      @keydown="onKeydown"
    />
    <SendButton :busy="busy" :can-send="canSend" @click="busy ? $emit('stopRequested') : submit()" />
  </div>
</template>

<style scoped>
.composer {
  display: flex;
  align-items: flex-end;
  gap: 8px;
  border-radius: var(--radius-large);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition:
    height var(--motion-standard) var(--motion-ease),
    box-shadow var(--motion-standard) var(--motion-ease);
}

.composer:focus-within {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.input {
  flex: 1;
  align-self: stretch;
  min-width: 0;
  resize: none;
  border: 0;
  outline: 0;
  padding: 5px 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  line-height: 22px;
}

.input::placeholder {
  color: var(--text-muted);
}
</style>
