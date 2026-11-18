<script setup lang="ts">
// The list above the input box while typing `/command` or `@file`.
import { computed, nextTick, ref, useTemplateRef, watch } from 'vue'
import type { Completion } from '../composer/completion.ts'
import { createPointerGuard } from '../composables/pointerGuard.ts'
import { components } from '../theme/generated/components.ts'
import Icon from './Icon.vue'

const {
  popupMaxRows: POPUP_MAX_ROWS,
  popupPadding: POPUP_PADDING,
  popupRowHeight: POPUP_ROW_HEIGHT,
} = components.completionPopup

const props = defineProps<{ completion: Completion | undefined }>()
const emit = defineEmits<{ hover: [index: number]; pick: [index: number] }>()

const height = computed(() => {
  const rows = Math.min(props.completion?.items.length ?? 0, POPUP_MAX_ROWS)
  return rows * POPUP_ROW_HEIGHT + 2 * POPUP_PADDING
})

const list = useTemplateRef<HTMLElement>('list')
const pointerMoved = createPointerGuard()
const highlightReady = ref(false)
let wasOpen = false

function onRowHover(index: number, event: MouseEvent): void {
  if (pointerMoved(event)) emit('hover', index)
}

watch(
  [() => props.completion?.selected, () => props.completion !== undefined],
  async ([selected, open]) => {
    if (!open || selected === undefined) {
      wasOpen = false
      highlightReady.value = false
      return
    }
    const isInitial = !wasOpen
    wasOpen = true
    await nextTick()
    const view = list.value
    if (!view) return
    const top = selected * POPUP_ROW_HEIGHT
    const bottom = top + POPUP_ROW_HEIGHT
    if (isInitial) {
      if (view.clientHeight > 0 && (top < view.scrollTop || bottom > view.scrollTop + view.clientHeight)) {
        view.scrollTop = Math.max(0, Math.round(top - (view.clientHeight - POPUP_ROW_HEIGHT) / 2))
      }
      requestAnimationFrame(() => {
        highlightReady.value = true
      })
      return
    }
    highlightReady.value = true
    if (top < view.scrollTop) view.scrollTop = top
    else if (bottom > view.scrollTop + view.clientHeight) view.scrollTop = bottom - view.clientHeight
  },
  { immediate: true },
)

// "src/app/main.ts" or "src\app\main.ts" -> ["src/app/", "main.ts"]; directories keep their trailing slash.
function splitPath(path: string): [string, string] {
  const body = path.endsWith('/') || path.endsWith('\\') ? path.slice(0, -1) : path
  const cut = Math.max(body.lastIndexOf('/'), body.lastIndexOf('\\')) + 1
  return [path.slice(0, cut), path.slice(cut)]
}
</script>

<template>
  <Transition name="popup">
    <div v-if="completion" class="popup" role="listbox" :style="{ height: `${height}px` }">
      <div ref="list" class="list">
        <div
          class="highlight"
          :class="{ ready: highlightReady }"
          :style="{ translate: `0 ${completion.selected * POPUP_ROW_HEIGHT}px` }"
        />
        <div
          v-for="(item, index) in completion.items"
          :key="`${index}:${item.value}`"
          class="row"
          role="option"
          :aria-selected="index === completion.selected"
          @mousemove="onRowHover(index, $event)"
          @mousedown.prevent="emit('pick', index)"
        >
          <template v-if="completion.kind === 'file' || completion.kind === 'path'">
            <Icon
              :icon="item.label.endsWith('/') || item.label.endsWith('\\') ? 'folder-16' : 'file-16'"
              class="muted"
            />
            <span class="path">
              <span v-if="splitPath(item.label)[0]" class="path-dir muted"
                ><bdi>{{ splitPath(item.label)[0] }}</bdi></span
              ><span class="path-name">{{ splitPath(item.label)[1] }}</span>
            </span>
            <span v-if="item.description" class="description muted">{{ item.description }}</span>
          </template>
          <template v-else>
            <Icon :icon="item.icon ?? 'slash-16'" class="muted" />
            <span class="label">{{ item.label }}</span>
            <span class="description muted">{{ item.description }}</span>
          </template>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.popup {
  --popup-gap: var(--completion-popup-popup-gap);
  position: absolute;
  left: 0;
  right: 0;
  bottom: calc(100% + var(--popup-gap));
  z-index: 5;
  padding: var(--completion-popup-popup-padding);
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: height var(--motion-standard) var(--motion-ease);
}

.list {
  position: relative;
  height: 100%;
  overflow-y: auto;
}

.highlight {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: var(--completion-popup-popup-row-height);
  border-radius: var(--radius-small);
  background: var(--surface-hover);
}

.highlight.ready {
  transition: translate var(--motion-quick) var(--motion-ease);
}

.row {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  height: var(--completion-popup-popup-row-height);
  padding: 0 10px;
  cursor: pointer;
}

.muted {
  color: var(--text-muted);
}

.label {
  flex: none;
}

.description {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: var(--small-font-size);
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path {
  display: flex;
  align-items: baseline;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.path-dir {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  direction: rtl;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.path-name {
  flex: 0 0 auto;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.popup-enter-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    translate var(--motion-standard) var(--motion-ease),
    height var(--motion-standard) var(--motion-ease);
}

.popup-leave-active {
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.popup-enter-from {
  opacity: 0;
  translate: 0 var(--popup-gap);
}

.popup-leave-to {
  opacity: 0;
}
</style>
