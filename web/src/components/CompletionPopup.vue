<script setup lang="ts">
// The list above the input box while typing `/command` or `@file`.
import { computed, nextTick, useTemplateRef, watch } from 'vue'
import type { Completion } from '../composer/completion.ts'
import { POPUP_MAX_ROWS, POPUP_PADDING, POPUP_ROW_HEIGHT } from '../composer/metrics.ts'
import Icon from './Icon.vue'

const props = defineProps<{ completion: Completion | undefined }>()
const emit = defineEmits<{ hover: [index: number]; pick: [index: number] }>()

const height = computed(() => {
  const rows = Math.min(props.completion?.items.length ?? 0, POPUP_MAX_ROWS)
  return rows * POPUP_ROW_HEIGHT + 2 * POPUP_PADDING
})

const list = useTemplateRef<HTMLElement>('list')
watch(
  () => props.completion?.selected,
  async (selected) => {
    if (selected === undefined || !list.value) return
    await nextTick()
    const top = selected * POPUP_ROW_HEIGHT
    const view = list.value
    if (top < view.scrollTop) view.scrollTop = top
    const bottom = top + POPUP_ROW_HEIGHT
    if (bottom > view.scrollTop + view.clientHeight) view.scrollTop = bottom - view.clientHeight
  },
)

// "src/app/main.ts" -> ["src/app/", "main.ts"]; a directory keeps its trailing slash in the name part.
function splitPath(path: string): [string, string] {
  const body = path.endsWith('/') ? path.slice(0, -1) : path
  const cut = body.lastIndexOf('/') + 1
  return [path.slice(0, cut), path.slice(cut)]
}
</script>

<template>
  <Transition name="popup">
    <div v-if="completion" class="popup" role="listbox" :style="{ height: `${height}px` }">
      <div ref="list" class="list">
        <div class="highlight" :style="{ translate: `0 ${completion.selected * POPUP_ROW_HEIGHT}px` }" />
        <div
          v-for="(item, index) in completion.items"
          :key="item.value"
          class="row"
          role="option"
          :aria-selected="index === completion.selected"
          @mousemove="emit('hover', index)"
          @mousedown.prevent="emit('pick', index)"
        >
          <template v-if="completion.kind === 'slash'">
            <Icon icon="slash-16" class="muted" />
            <span class="label">{{ item.label }}</span>
            <span class="description muted">{{ item.description }}</span>
          </template>
          <template v-else>
            <Icon :icon="item.label.endsWith('/') ? 'folder-16' : 'file-16'" class="muted" />
            <span class="path">
              <span class="muted">{{ splitPath(item.label)[0] }}</span
              >{{ splitPath(item.label)[1] }}
            </span>
          </template>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.popup {
  --popup-gap: 8px;
  position: absolute;
  left: 0;
  right: 0;
  bottom: calc(100% + var(--popup-gap));
  z-index: 5;
  padding: 4px;
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
  height: 36px;
  border-radius: var(--radius-small);
  background: var(--surface-hover);
  transition: translate var(--motion-quick) var(--motion-ease);
}

.row {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
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
  min-width: 0;
  overflow: hidden;
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
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
