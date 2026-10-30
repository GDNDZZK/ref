<script setup lang="ts">
// One tag of the input box (agent, model, variant, shell): icon and label in a pill.
// The pill's width follows its label, and a new label slides in as the old one slides out
// (Composer.qml's chips). `shown` false shrinks the pill to nothing, which is how the variant
// and shell pills come and go.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef } from 'vue'
import type { IconKey } from '../icons/generated/icons.ts'
import Icon from './Icon.vue'

const CHIP_GAP = 4 // between pills; part of the width that collapses

const props = defineProps<{
  shown: boolean
  icon: IconKey
  label: string
  tooltip?: string
  iconColor?: string // CSS color; default muted
  accent?: boolean // label and icon in the shell color
  clickable?: boolean
  labelHover?: boolean // the label turns to full text color on hover (agent)
}>()
defineEmits<{ click: [] }>()

const content = useTemplateRef<HTMLElement>('content')
const contentWidth = ref(0)
const animated = ref(false) // false until the first layout, so a pill does not grow in when the page opens
const width = computed(() => (props.shown ? contentWidth.value + CHIP_GAP : 0))

const observer = new ResizeObserver(() => {
  if (content.value) contentWidth.value = content.value.offsetWidth
})

onMounted(async () => {
  if (!content.value) return
  contentWidth.value = content.value.offsetWidth
  observer.observe(content.value)
  await nextTick()
  animated.value = true
})
onBeforeUnmount(() => observer.disconnect())
</script>

<template>
  <div
    class="pill-slot"
    :class="{ shown, animated }"
    :style="{ width: `${width}px` }"
    :inert="!shown || undefined"
  >
    <component
      :is="clickable ? 'button' : 'div'"
      ref="content"
      class="pill"
      :class="{ clickable, accent, 'label-hover': labelHover }"
      :style="iconColor ? { '--icon-color': iconColor } : undefined"
      :title="tooltip"
      :type="clickable ? 'button' : undefined"
      @click="clickable && $emit('click')"
    >
      <Icon class="icon" :icon="icon" />
      <span class="label">
        <span class="sizer">{{ label }}</span>
        <Transition name="morph">
          <span :key="label" class="text">{{ label }}</span>
        </Transition>
      </span>
    </component>
  </div>
</template>

<style scoped>
.pill-slot {
  --chip-height: 28px;
  --chip-padding-x: 10px;
  --chip-icon-gap: 6px;
  --label-slide: 6px;
  flex: none;
  height: var(--chip-height);
  overflow: hidden;
  opacity: 0;
  visibility: hidden;
}

.pill-slot.shown {
  opacity: 1;
  visibility: visible;
}

.pill-slot.animated {
  transition:
    width var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    visibility 0s linear var(--motion-standard);
}

.pill-slot.animated.shown {
  transition-delay: 0s;
}

.pill {
  display: flex;
  align-items: center;
  gap: var(--chip-icon-gap);
  width: max-content;
  height: var(--chip-height);
  padding: 0 var(--chip-padding-x);
  border-radius: calc(var(--chip-height) / 2);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  white-space: nowrap;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.pill.clickable {
  cursor: pointer;
}

.pill.clickable:hover {
  background: var(--surface-hover);
}

.pill.label-hover:hover {
  color: var(--text);
}

.pill.accent {
  color: var(--shell-border);
}

.icon {
  color: var(--icon-color, currentColor);
  transition: color var(--motion-standard) var(--motion-ease);
}

.label {
  position: relative;
  display: block;
}

/* Holds the width of the current label; the visible text is laid over it. */
.sizer {
  visibility: hidden;
}

.text {
  position: absolute;
  left: 0;
  top: 0;
}

.morph-enter-active,
.morph-leave-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease);
}

.morph-enter-from {
  opacity: 0;
  translate: 0 var(--label-slide);
}

.morph-leave-to {
  opacity: 0;
  translate: 0 calc(-1 * var(--label-slide));
}
</style>
