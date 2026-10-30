<script setup lang="ts">
// One toast (Toast.qml). The icon springs in with the toast, and again when the same message repeats.
import { nextTick, onMounted, useTemplateRef, watch } from 'vue'
import type { Notice } from '../stores/notices.ts'
import Flourish from './Flourish.vue'
import Icon from './Icon.vue'

const props = defineProps<{ notice: Notice }>()
defineEmits<{ dismiss: []; pause: []; resume: [] }>()

const ICONS = { info: 'info-16', warn: 'warning-16', error: 'error-16' } as const

const flourish = useTemplateRef<InstanceType<typeof Flourish>>('flourish')
const icon = useTemplateRef<HTMLElement>('icon')

onMounted(() => icon.value && flourish.value?.enter(icon.value))
watch(
  () => props.notice.repeats,
  async () => {
    await nextTick()
    if (icon.value) flourish.value?.play(icon.value)
  },
)
</script>

<template>
  <div class="slot">
    <button
      type="button"
      class="toast"
      :class="notice.level"
      @click="$emit('dismiss')"
      @pointerenter="$emit('pause')"
      @pointerleave="$emit('resume')"
    >
      <span class="mark">
        <Flourish ref="flourish" />
        <span ref="icon" class="icon"><Icon :icon="ICONS[notice.level]" /></span>
      </span>
      <span class="text">{{ notice.text }}</span>
    </button>
  </div>
</template>

<style scoped>
.slot {
  display: grid;
  grid-template-rows: 1fr;
  margin-bottom: 8px;
  pointer-events: auto;
}

.toast {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  min-height: 0;
  padding: 10px 12px;
  overflow: hidden;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  font-size: calc(var(--small-font-size) + 1px);
  text-align: left;
}

.toast.warn {
  background: var(--warning-surface);
  box-shadow: inset 0 0 0 1px var(--warning-border);
}

.toast.error {
  background: var(--error-surface);
  box-shadow: inset 0 0 0 1px var(--error-border);
}

.mark {
  position: relative;
  display: flex;
  flex: none;
  margin-top: 1px;
}

.icon {
  display: flex;
  color: var(--accent);
}

.warn .icon {
  color: var(--warning);
}

.error .icon {
  color: var(--danger);
}

.text {
  display: -webkit-box;
  min-width: 0;
  overflow: hidden;
  overflow-wrap: anywhere;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
}
</style>
