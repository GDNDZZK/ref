<script setup lang="ts" generic="T extends PickItem">
// The list of the command palette and the pick dialogs: rows of one height, optional section labels,
// and one highlight that slides to the selected row.
import { computed, nextTick, ref, useTemplateRef, watch } from 'vue'
import { createPointerGuard } from '../composables/pointerGuard.ts'
import type { PickItem } from '../composables/usePickSelection.ts'
import { metrics } from '../theme/generated/theme.ts'
const SECTION_HEIGHT_PX = metrics.sidebarSectionHeight

const props = defineProps<{
  items: readonly T[]
  selected: string | undefined
  rowHeight: number
  maxRows: number
  grouped: boolean // show the section labels
  empty: string
}>()
const emit = defineEmits<{ hover: [key: string]; pick: [key: string] }>()

// Where each row starts, counting the section labels.
const layout = computed(() => {
  let top = 0
  let group: string | undefined
  return props.items.map((item) => {
    const labelled = props.grouped && item.group !== undefined && item.group !== group
    if (props.grouped) group = item.group
    if (labelled) top += SECTION_HEIGHT_PX
    const placed = { item, top, label: labelled ? item.group : undefined }
    top += props.rowHeight
    return placed
  })
})

const listHeight = computed(() => {
  const last = layout.value.at(-1)
  return last ? last.top + props.rowHeight : 0
})
const highlight = computed(() => layout.value.find((row) => row.item.key === props.selected))

const scroller = useTemplateRef<HTMLElement>('scroller')
const pointerMoved = createPointerGuard()
const highlightReady = ref(false)
let initialScrollDone = false

function onRowHover(item: T, event: MouseEvent): void {
  if (item.disabled || !pointerMoved(event)) return
  emit('hover', item.key)
}

// Keeps the highlighted row visible on initial mount (instant) and after arrow keys move it.
watch(
  [() => props.selected, () => props.items.length],
  async () => {
    await nextTick()
    const row = highlight.value
    const box = scroller.value
    if (!row || !box) return
    const topEdge = row.top - (row.label ? SECTION_HEIGHT_PX : 0)
    const rowBottom = row.top + props.rowHeight
    if (!initialScrollDone) {
      initialScrollDone = true
      if (box.clientHeight > 0 && (topEdge < box.scrollTop || rowBottom > box.scrollTop + box.clientHeight)) {
        box.scrollTop = Math.max(0, Math.round(row.top - (box.clientHeight - props.rowHeight) / 2))
      }
      requestAnimationFrame(() => {
        highlightReady.value = true
      })
      return
    }
    highlightReady.value = true
    if (topEdge < box.scrollTop) box.scrollTop = topEdge
    else if (rowBottom > box.scrollTop + box.clientHeight) box.scrollTop = rowBottom - box.clientHeight
  },
  { immediate: true },
)
</script>

<template>
  <div v-if="items.length === 0" class="empty-wrap">
    <p class="empty">{{ empty }}</p>
    <slot name="empty-action" />
  </div>
  <div
    v-else
    ref="scroller"
    class="scroller"
    :style="{ maxHeight: `${maxRows * rowHeight}px` }"
    role="listbox"
  >
    <div class="rows" :style="{ height: `${listHeight}px` }">
      <span
        v-if="highlight"
        class="highlight"
        :class="{ ready: highlightReady }"
        :style="{ height: `${rowHeight}px`, translate: `0 ${highlight.top}px` }"
      />
      <template v-for="row in layout" :key="row.item.key">
        <h3 v-if="row.label" class="section" :style="{ top: `${row.top - SECTION_HEIGHT_PX}px` }">
          {{ row.label }}
        </h3>
        <div
          class="row"
          role="option"
          :aria-selected="row.item.key === selected"
          :aria-disabled="row.item.disabled"
          :class="{ disabled: row.item.disabled }"
          :style="{ top: `${row.top}px`, height: `${rowHeight}px` }"
          @mousemove="onRowHover(row.item, $event)"
          @click="!row.item.disabled && emit('pick', row.item.key)"
        >
          <slot name="row" :item="row.item" :selected="row.item.key === selected" />
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.scroller {
  overflow-y: auto;
}

.rows {
  position: relative;
}

.highlight {
  position: absolute;
  inset-inline: 0;
  border-radius: calc(var(--radius) - 2px);
  background: var(--surface-hover);
  pointer-events: none;
}

.highlight.ready {
  transition: translate var(--motion-quick) var(--motion-ease);
}

.section {
  position: absolute;
  inset-inline: 0;
  height: 28px;
  margin: 0;
  padding: 0 10px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 400;
  line-height: 36px;
}

.row {
  position: absolute;
  inset-inline: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 12px;
  border-radius: calc(var(--radius) - 2px);
  cursor: pointer;
}

.row.disabled {
  opacity: 0.4;
  cursor: default;
}

.empty-wrap {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px 0;
}

.empty {
  margin: 0;
  color: var(--text-muted);
  text-align: center;
}
</style>
