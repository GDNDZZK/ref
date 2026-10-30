<script setup lang="ts" generic="T extends string">
// A row of exclusive choices with a sliding indicator (Segmented.qml).
import { nextTick, onMounted, ref, useTemplateRef, watch } from 'vue'

const props = defineProps<{ options: readonly { value: T; label: string }[]; label: string }>()
const selected = defineModel<T>({ required: true })

const segments = useTemplateRef<HTMLElement>('segments')
const indicator = ref({ x: 0, width: 0 })
const placed = ref(false) // the first placement does not slide

function place(): void {
  const index = props.options.findIndex((option) => option.value === selected.value)
  const button = segments.value?.children[index + 1] as HTMLElement | undefined // child 0 is the indicator
  if (button) indicator.value = { x: button.offsetLeft, width: button.offsetWidth }
}

onMounted(() => {
  place()
  void nextTick(() => (placed.value = true))
})
watch([selected, () => props.options], () => place(), { flush: 'post' })
</script>

<template>
  <div ref="segments" class="segmented" role="radiogroup" :aria-label="label">
    <span
      class="indicator"
      :class="{ slides: placed }"
      :style="{ translate: `${indicator.x}px 0`, width: `${indicator.width}px` }"
    />
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      role="radio"
      class="segment"
      :class="{ chosen: option.value === selected }"
      :aria-checked="option.value === selected"
      @click="selected = option.value"
    >
      {{ option.label }}
    </button>
  </div>
</template>

<style scoped>
.segmented {
  position: relative;
  display: flex;
  height: 30px;
  padding: 2px;
  border-radius: 15px;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.indicator {
  position: absolute;
  left: 0;
  top: 2px;
  bottom: 2px;
  border-radius: 13px;
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
}

.indicator.slides {
  transition:
    translate var(--motion-standard) var(--motion-ease),
    width var(--motion-standard) var(--motion-ease);
}

.segment {
  position: relative;
  flex: 1;
  padding: 0 12px;
  color: var(--text-muted);
  font-size: calc(var(--small-font-size) + 1px);
  white-space: nowrap;
  transition: color var(--motion-standard) var(--motion-ease);
}

.segment.chosen {
  color: var(--text);
}
</style>
