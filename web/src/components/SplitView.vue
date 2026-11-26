<script setup lang="ts">
// All background sessions at once (SplitView.qml): tiles on a grid that fills the content area.
// Zooming a tile stretches its frame over the area and fades the others; it is the same pane
// throughout, so scroll position and streaming text survive.
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  shallowRef,
  useTemplateRef,
  watch,
  watchEffect,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { motion, motionEasing } from '../theme/generated/motion.ts'
import { metrics } from '../theme/generated/theme.ts'
import { tween } from '../motion/tween.ts'
import { cornerClearance, gridHeight, tileRects, TILE_SPEC } from '../shell/tileLayout.ts'
import type { Band } from '../shell/tileLayout.ts'
import { useCommandsStore } from '../stores/commands.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import SplitTile from './SplitTile.vue'
import TextButton from './TextButton.vue'
const { t } = useI18n()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const notices = useNoticesStore()
const layout = useLayoutStore()

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

// Freeze visual tile/focus/zoom state once mode leaves 'split' so the split-fade exit transition
// fades the focused tile border and grid cleanly instead of snapping off on frame 1.
const activeTileIds = shallowRef<readonly string[]>(workspace.tileIds)
const activeFocusId = ref<string | undefined>(workspace.tileFocusId)
const activeZoomedId = ref<string | undefined>(workspace.zoomedId)
watchEffect(() => {
  if (workspace.mode !== 'split') return
  activeTileIds.value = workspace.tileIds
  activeFocusId.value = workspace.tileFocusId
  activeZoomedId.value = workspace.zoomedId
})

// No rects before the observer has reported the area size: tiles mounted on the unmeasured 0×0
// grid would animate left/top/width/height from that degenerate layout into place.
const rects = computed(() =>
  size.value.width === 0
    ? []
    : tileRects(activeTileIds.value.length, size.value.width, size.value.height, TILE_SPEC),
)
watchEffect(() => {
  if (workspace.mode === 'split') workspace.setTileRects(rects.value)
})

// Which tiles have a rect yet: none until the area is measured, so no tile is ever placed on the
// degenerate 0×0 grid.
const laidOutIds = computed(() => (size.value.width > 0 ? activeTileIds.value : []))

const zoomed = computed(() => activeZoomedId.value !== undefined)
const hasTopChrome = computed(() => !zoomed.value && !layout.narrow)
// The floating window buttons sit over the content. The header of a tile under them keeps its
// buttons and marks out of their way, so no tile loses height (the GUI's TileGrid.topInset).
interface Clearance {
  left: number
  right: number
}
const NO_CLEARANCE: Clearance = { left: 0, right: 0 }
const clearances = ref<Clearance[]>([])

function band(element: Element | null | undefined): Band | undefined {
  return element?.getBoundingClientRect()
}

let clearanceFrame: number | undefined

function scheduleMeasureClearances(): void {
  if (clearanceFrame !== undefined) return
  clearanceFrame = requestAnimationFrame(() => {
    clearanceFrame = undefined
    measureClearances()
  })
}

onBeforeUnmount(() => {
  if (clearanceFrame !== undefined) {
    cancelAnimationFrame(clearanceFrame)
    clearanceFrame = undefined
  }
})

function measureClearances(): void {
  if (clearanceFrame !== undefined) {
    cancelAnimationFrame(clearanceFrame)
    clearanceFrame = undefined
  }
  const element = area.value
  const areaBand = band(element)
  const content = element?.closest('.content') ?? element?.parentElement
  if (!element || !areaBand || zoomed.value) {
    clearances.value = []
    return
  }
  const left = band(content?.querySelector('.corner.left'))
  const right = band(content?.querySelector('.corner.right'))
  clearances.value = rects.value.map((rect) => {
    const top = areaBand.top + rect.y - element.scrollTop
    const header = {
      top,
      bottom: top + metrics.tileHeaderHeight,
      left: areaBand.left + rect.x,
      right: areaBand.left + rect.x + rect.width,
    }
    return {
      left: left ? cornerClearance(header, left, 'left', TILE_SPEC.gap) : 0,
      right: right ? cornerClearance(header, right, 'right', TILE_SPEC.gap) : 0,
    }
  })
}

watch([rects, zoomed, () => layout.sidebarVisible], () => void nextTick(measureClearances), { flush: 'post' })

// A zoomed tile fills the area; the others keep their slots, faded out.
function frame(index: number, id: string): Record<string, string> {
  const rect = id === activeZoomedId.value ? { x: 0, y: 0, ...size.value } : rects.value[index]
  if (!rect) return {}
  return { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.width}px`, height: `${rect.height}px` }
}

// Zooming in or out takes the slow duration; reflowing the grid the standard one.
// When zooming from a scrolled grid, tween scrollTop to 0 so frame()'s top: 0 aligns with the viewport.
const zooming = ref(false)
let zoomTimer: ReturnType<typeof setTimeout> | undefined
let stopScrollTween: (() => void) | undefined
watch(activeZoomedId, (nextId) => {
  zooming.value = true
  clearTimeout(zoomTimer)
  zoomTimer = setTimeout(() => (zooming.value = false), motion.slow)
  stopScrollTween?.()
  stopScrollTween = undefined
  const element = area.value
  if (!nextId || !element || element.scrollTop <= 0) return
  stopScrollTween = tween({
    from: element.scrollTop,
    to: 0,
    durationMs: motion.slow,
    easing: motionEasing.ease,
    update: (value) => {
      element.scrollTop = value
    },
    done: () => {
      element.scrollTop = 0
      stopScrollTween = undefined
    },
  })
})
onBeforeUnmount(() => {
  clearTimeout(zoomTimer)
  stopScrollTween?.()
})
const contentHeight = computed(() =>
  zoomed.value ? size.value.height : Math.max(size.value.height, gridHeight(rects.value, TILE_SPEC.padding)),
)

function newSession(): void {
  commands.run('new').catch(notices.pushError)
}

function onBackgroundClick(event: MouseEvent): void {
  if (zoomed.value) return
  const target = event.target instanceof Element ? event.target : null
  if (!target?.closest('[data-tile], button')) workspace.focusTile(undefined)
}
</script>

<template>
  <div
    ref="area"
    class="split"
    :class="{ zoomed, zooming, 'with-top-chrome': hasTopChrome }"
    @scroll.passive="scheduleMeasureClearances"
    @click="onBackgroundClick"
  >
    <TransitionGroup tag="div" name="tile" class="grid" :style="{ height: `${contentHeight}px` }">
      <SplitTile
        v-for="(id, index) in laidOutIds"
        :key="id"
        class="slot"
        :style="frame(index, id)"
        :session="workspace.chat(id)"
        :focused="id === activeFocusId"
        :zoomed="id === activeZoomedId"
        :hidden="zoomed && id !== activeZoomedId"
        :clearance="clearances[index] ?? NO_CLEARANCE"
        @focus="workspace.focusTile(id)"
        @unfocus="workspace.focusTile(undefined)"
        @zoom="activeZoomedId === id ? workspace.unzoom() : workspace.zoomTile(id)"
      />
    </TransitionGroup>
    <Transition name="empty-fade">
      <div v-if="activeTileIds.length === 0" class="empty">
        <p>{{ workspace.splitFilter !== 'all' ? t('split.emptyFiltered') : t('split.empty') }}</p>
        <div class="empty-actions">
          <TextButton
            v-if="workspace.splitFilter !== 'all'"
            :label="t('split.showAll')"
            @click="workspace.setSplitFilter('all')"
          />
          <TextButton :label="t('sidebar.newSession')" @click="newSession" />
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.split {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  background: var(--background);
  --geometry-duration: var(--motion-standard);
}

.split.zooming {
  --geometry-duration: var(--motion-slow);
}

.split.zoomed {
  overflow-y: hidden;
}

.split.with-top-chrome {
  margin-top: var(--sidebar-header-height);
}

.grid {
  position: relative;
  transition: height var(--geometry-duration) var(--motion-ease);
}

.slot {
  position: absolute;
  z-index: 1;
}

.slot.tile-enter-from {
  opacity: 0;
  scale: var(--motion-tile-enter-scale);
}

.slot.tile-leave-active {
  z-index: 0;
  pointer-events: none;
  transition-timing-function: var(--motion-exit-ease);
}

.slot.tile-leave-to {
  opacity: 0;
  scale: var(--motion-tile-exit-scale);
}

.empty {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
}
.empty-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.empty-fade-enter-active,
.empty-fade-leave-active {
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.empty-fade-enter-from,
.empty-fade-leave-to {
  opacity: 0;
}
</style>
