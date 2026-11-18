<script setup lang="ts">
// A small menu at the pointer (right click). Closes on a choice, a click elsewhere, Esc or scrolling.
import { nextTick, onBeforeUnmount, useTemplateRef, watch } from 'vue'
import { Key } from '../keys.ts'

export interface MenuItem {
  label: string
  danger?: boolean
  run(): void
}

const props = withDefaults(
  defineProps<{ x: number; y: number; items: readonly MenuItem[]; open?: boolean }>(),
  {
    open: true,
  },
)
const emit = defineEmits<{ close: [] }>()

const EDGE_MARGIN = 8

const menu = useTemplateRef<HTMLElement>('menu')

function choose(item: MenuItem): void {
  emit('close')
  item.run()
}

function onPointerDown(event: PointerEvent): void {
  if (!menu.value?.contains(event.target as Node)) emit('close')
}

function onBlur(): void {
  emit('close')
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== Key.Escape) return
  event.stopPropagation()
  emit('close')
}

function unlisten(): void {
  window.removeEventListener('pointerdown', onPointerDown, true)
  window.removeEventListener('keydown', onKeydown, true)
  window.removeEventListener('blur', onBlur)
}

function place(): void {
  const element = menu.value
  if (!element) return
  const left = Math.min(props.x, window.innerWidth - element.offsetWidth - EDGE_MARGIN)
  const top = Math.min(props.y, window.innerHeight - element.offsetHeight - EDGE_MARGIN)
  element.style.left = `${Math.max(EDGE_MARGIN, left)}px`
  element.style.top = `${Math.max(EDGE_MARGIN, top)}px`
}

// Keeps the menu inside the window and listens while open.
watch(
  [() => props.open, () => props.x, () => props.y],
  async ([isOpen]) => {
    unlisten()
    if (!isOpen) return
    await nextTick()
    place()
    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('keydown', onKeydown, true)
    window.addEventListener('blur', onBlur)
  },
  { immediate: true },
)
onBeforeUnmount(unlisten)
</script>

<template>
  <Teleport to="body">
    <Transition name="context-menu" appear>
      <div v-if="open" ref="menu" class="menu" role="menu" :style="{ left: `${x}px`, top: `${y}px` }">
        <button
          v-for="item in items"
          :key="item.label"
          type="button"
          role="menuitem"
          class="item"
          :class="{ danger: item.danger }"
          @click="choose(item)"
        >
          {{ item.label }}
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.menu {
  position: fixed;
  z-index: 30;
  min-width: 140px;
  padding: 4px;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 6px 20px #0000002e;
  transform-origin: top left;
}

.item {
  display: block;
  width: 100%;
  padding: 6px 10px;
  border-radius: var(--radius-small);
  font-size: calc(var(--font-size) - 2px);
  text-align: left;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.item:hover {
  background: var(--surface-hover);
}

.item.danger {
  color: var(--danger-text);
}

.context-menu-enter-active {
  transition:
    opacity var(--motion-quick) var(--motion-ease),
    scale var(--motion-quick) var(--motion-ease);
}

.context-menu-leave-active {
  pointer-events: none;
  transition:
    opacity var(--motion-quick) var(--motion-exit-ease),
    scale var(--motion-quick) var(--motion-exit-ease);
}

.context-menu-enter-from,
.context-menu-leave-to {
  opacity: 0;
  scale: var(--motion-pop-scale);
}
</style>
