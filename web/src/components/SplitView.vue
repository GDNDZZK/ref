<script setup lang="ts">
// All background sessions at once (SplitView.qml): tiles on a grid that fills the content area.
// Zooming a tile stretches its frame over the area and fades the others; it is the same pane
// throughout, so scroll position and streaming text survive.
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { motion } from '../theme/generated/motion.ts'
import { gridHeight, tileRects, TILE_SPEC } from '../shell/tileLayout.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import SplitTile from './SplitTile.vue'
import TextButton from './TextButton.vue'

const { t } = useI18n()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const notices = useNoticesStore()

const area = useTemplateRef<HTMLElement>('area')
const size = ref({ width: 0, height: 0 })
let observer: ResizeObserver | undefined

onMounted(() => {
  const element = area.value
  if (!element) return
  observer = new ResizeObserver(([entry]) => {
    if (entry) size.value = { width: entry.contentRect.width, height: entry.contentRect.height }
  })
  observer.observe(element)
})
onBeforeUnmount(() => observer?.disconnect())

const rects = computed(() =>
  tileRects(workspace.tileIds.length, size.value.width, size.value.height, TILE_SPEC),
)
watchEffect(() => workspace.setTileRects(rects.value))

const zoomed = computed(() => workspace.zoomedId !== undefined)
// A zoomed tile fills the area; the others keep their slots, faded out.
function frame(index: number, id: string): Record<string, string> {
  const rect = id === workspace.zoomedId ? { x: 0, y: 0, ...size.value } : rects.value[index]
  if (!rect) return {}
  return { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` }
}

// Zooming in or out takes the slow duration; reflowing the grid the standard one.
const zooming = ref(false)
let zoomTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => workspace.zoomedId,
  () => {
    zooming.value = true
    clearTimeout(zoomTimer)
    zoomTimer = setTimeout(() => (zooming.value = false), motion.slow)
  },
)
onBeforeUnmount(() => clearTimeout(zoomTimer))

const contentHeight = computed(() =>
  zoomed.value ? size.value.height : Math.max(size.value.height, gridHeight(rects.value, TILE_SPEC.padding)),
)

function newSession(): void {
  commands.run('new').catch(notices.pushError)
}
</script>

<template>
  <div ref="area" class="split" :class="{ zoomed, zooming }">
    <div v-if="workspace.tileIds.length === 0" class="empty">
      <p>{{ t('split.empty') }}</p>
      <TextButton :label="t('sidebar.newSession')" @click="newSession" />
    </div>
    <TransitionGroup v-else tag="div" name="tile" class="grid" :style="{ height: `${contentHeight}px` }">
      <SplitTile
        v-for="(id, index) in workspace.tileIds"
        :key="id"
        class="slot"
        :style="frame(index, id)"
        :session="workspace.chat(id)"
        :focused="id === workspace.tileFocusId"
        :zoomed="id === workspace.zoomedId"
        :hidden="zoomed && id !== workspace.zoomedId"
        @focus="workspace.focusTile(id)"
        @zoom="workspace.zoomedId === id ? workspace.unzoom() : workspace.zoomTile(id)"
      />
    </TransitionGroup>
  </div>
</template>

<style scoped>
.split {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  --geometry-duration: var(--motion-standard);
}

.split.zooming {
  --geometry-duration: var(--motion-slow);
}

.split.zoomed {
  overflow-y: hidden;
}

.grid {
  position: relative;
}

.slot {
  position: absolute;
}

.slot.tile-enter-from {
  opacity: 0;
  scale: var(--motion-tile-enter-scale);
}

.slot.tile-leave-active {
  z-index: -1;
  transition-timing-function: var(--motion-exit-ease);
}

.slot.tile-leave-to {
  opacity: 0;
  scale: var(--motion-tile-exit-scale);
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-muted);
}
</style>
