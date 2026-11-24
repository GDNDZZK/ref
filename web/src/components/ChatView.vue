<script setup lang="ts">
// The conversation, centered in a reading column (ChatView.qml).
//
// Virtualization: when `session.slots` is populated, every message in `0..total_messages`
// contributes either its measured height (when resident) or a synchronous local height estimate
// (`estimateMessageHeight`), keeping the scrollbar true with zero network requests on resize.
// Offscreen windows collapse to top/bottom spacers; placeholders render as SkeletonBlock and
// load in debounced ranges with scroll anchoring.
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  provide,
  ref,
  shallowRef,
  useTemplateRef,
  watch,
} from 'vue'
import { chatScrollKey } from '../composables/chatScroll.ts'
import { imageCaption, imageLightboxKey } from '../composables/imageLightboxContext.ts'
import { Key } from '../keys.ts'
import type { Block, ImageRef } from '../model/blocks.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { estimateMessageHeight, layoutMetaFromMessage } from '../model/layoutEstimator.ts'
import { findLeaving, prependedCount, withLeaving } from '../model/rowDiff.ts'
import type { LeavingRow } from '../model/rowDiff.ts'
import type { TranscriptSlot } from '../model/transcript.ts'
import { scrollArrived, scrollStep } from '../motion/chase.ts'
import { useFrame } from '../motion/useFrame.ts'
import { metrics } from '../theme/generated/theme.ts'
import BlockView from './BlockView.vue'
import ImageLightbox from './ImageLightbox.vue'
import type { LightboxItem } from './ImageLightbox.vue'
import OlderMessages from './OlderMessages.vue'
import SkeletonBlock from './SkeletonBlock.vue'
import { OLDER_TRIGGER_DISTANCE } from './toolLimits.ts'

const props = defineProps<{ session: ChatSession }>()
const blocks = computed(() => props.session.blocks)
const slots = computed<readonly TranscriptSlot[] | undefined>(() => props.session.slots)
const generation = computed(() => props.session.generation) // changes when the history is loaded
const following = defineModel<boolean>('following', { default: true })
const emit = defineEmits<{ errorOutOfSight: [] }>()

const GESTURE_IDLE_MS = 150 // without scrolling, a user gesture has ended
const SETTLE_EVICT_MS = 160
const AT_END_TOLERANCE = 1 // px
const OVERSCAN_PX = 600
const OVERSCAN_SLOTS = 6
const DEFAULT_TAIL_RENDER = 30
const FADE_DURATION_MS = 140
const hoveredMessageId = ref('')
const scroller = useTemplateRef<HTMLElement>('scroller')
const rows = useTemplateRef<HTMLElement>('rows')

// --- Virtualization geometry (active when `session.slots` has entries).
const virtualized = computed(() => Array.isArray(slots.value) && slots.value.length > 0)
const columnWidth = ref<number>(metrics.contentWidth)
const viewportTop = ref(0)
const viewportHeight = ref(0)
const measuredHeights = shallowRef(new Map<string, number>())
const materializedFadeIds = shallowRef<ReadonlySet<string>>(new Set())
let fadeClearTimer: ReturnType<typeof setTimeout> | undefined

function slotMeta(slot: TranscriptSlot) {
  return slot.kind === 'placeholder' ? slot.meta : layoutMetaFromMessage(slot.seq, slot.message)
}

function estimateSlot(slot: TranscriptSlot): number {
  return estimateMessageHeight(slotMeta(slot), {
    columnWidth: columnWidth.value,
    thinkingExpanded: Boolean(props.session.thinkingExpanded),
    detailsExpanded: Boolean(props.session.detailsExpanded),
    isFirst: slot.seq === 0 && !props.session.moreBefore,
  })
}

const slotLayout = computed(() => {
  const list = slots.value
  if (!list || list.length === 0) {
    return { heights: [] as number[], offsets: [] as number[], totalHeight: 0 }
  }
  const heights = new Array<number>(list.length)
  const offsets = new Array<number>(list.length)
  const measured = measuredHeights.value
  let cursor = 0
  for (let i = 0; i < list.length; i++) {
    const slot = list[i]!
    const height = slot.kind === 'materialized' ? (measured.get(slot.id) ?? estimateSlot(slot)) : estimateSlot(slot)
    heights[i] = height
    offsets[i] = cursor
    cursor += height + (i < list.length - 1 ? metrics.blockSpacing : 0)
  }
  return { heights, offsets, totalHeight: cursor }
})

function findSlotAtOffset(offsets: readonly number[], heights: readonly number[], y: number): number {
  let lo = 0
  let hi = offsets.length - 1
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    const end = offsets[mid]! + heights[mid]!
    if (end < y) lo = mid + 1
    else hi = mid
  }
  return lo
}

const visibleRange = computed(() => {
  const list = slots.value
  if (!list || list.length === 0) return { start: 0, end: -1 }
  if (viewportHeight.value <= 0) {
    const start = Math.max(0, list.length - DEFAULT_TAIL_RENDER)
    return { start, end: list.length - 1 }
  }
  const { offsets, heights } = slotLayout.value
  const top = Math.max(0, viewportTop.value)
  const bottom = top + viewportHeight.value
  const start = findSlotAtOffset(offsets, heights, top)
  const end = findSlotAtOffset(offsets, heights, bottom)
  return { start, end }
})

const renderRange = computed(() => {
  const list = slots.value
  if (!list || list.length === 0) return { start: 0, end: -1 }
  if (viewportHeight.value <= 0) return visibleRange.value
  const { offsets, heights } = slotLayout.value
  const top = Math.max(0, viewportTop.value - OVERSCAN_PX)
  const bottom = viewportTop.value + viewportHeight.value + OVERSCAN_PX
  const start = Math.max(0, findSlotAtOffset(offsets, heights, top) - OVERSCAN_SLOTS)
  const end = Math.min(list.length - 1, findSlotAtOffset(offsets, heights, bottom) + OVERSCAN_SLOTS)
  return { start, end }
})

const renderedSlots = computed(() => {
  const list = slots.value
  if (!list || renderRange.value.end < renderRange.value.start) return [] as TranscriptSlot[]
  return list.slice(renderRange.value.start, renderRange.value.end + 1)
})

const topSpacerHeight = computed(() => {
  if (!virtualized.value) return 0
  const { start, end } = renderRange.value
  if (end < start) return 0
  return slotLayout.value.offsets[start] ?? 0
})

const bottomSpacerHeight = computed(() => {
  if (!virtualized.value) return 0
  const { start, end } = renderRange.value
  if (end < start) return 0
  const { offsets, heights, totalHeight } = slotLayout.value
  const renderedBottom = (offsets[end] ?? 0) + (heights[end] ?? 0)
  return Math.max(0, totalHeight - renderedBottom)
})

const tailErrorBlock = computed(() => {
  const last = blocks.value.at(-1)
  return last && last.messageId === '' ? last : undefined
})

const showTailError = computed(() => {
  const list = slots.value
  if (!tailErrorBlock.value || !list) return false
  return renderRange.value.end >= list.length - 1
})

function slotHeight(seq: number): number {
  return slotLayout.value.heights[seq] ?? 48
}

function measureRenderedMessages(): void {
  const container = rows.value
  if (!container || !virtualized.value) return
  const current = new Map(measuredHeights.value)
  let changed = false
  const byMessage = new Map<string, { top: number; bottom: number }>()
  for (const el of container.querySelectorAll<HTMLElement>('[data-message]:not(.skeleton-frame)')) {
    const id = el.dataset.message
    if (!id) continue
    const top = el.offsetTop
    const bottom = top + el.offsetHeight
    const existing = byMessage.get(id)
    if (!existing) byMessage.set(id, { top, bottom })
    else {
      existing.top = Math.min(existing.top, top)
      existing.bottom = Math.max(existing.bottom, bottom)
    }
  }
  for (const [id, span] of byMessage) {
    const height = Math.max(1, span.bottom - span.top)
    if (current.get(id) !== height) {
      current.set(id, height)
      changed = true
    }
  }
  if (changed) measuredHeights.value = current
}

// Recompute estimates synchronously on column width or fold changes with zero network requests.
watch(
  [columnWidth, () => props.session.thinkingExpanded, () => props.session.detailsExpanded],
  () => {
    if (!virtualized.value) return
    measuredHeights.value = new Map()
    void nextTick(measureRenderedMessages)
  },
)

// --- Rows that appeared after the history was loaded play their entrance; rows that went away
// play their exit and are kept until it is over.
const freshKeys = shallowRef<ReadonlySet<string>>(new Set())
const leaving = shallowRef<readonly LeavingRow[]>([])
let knownBlocks: readonly Block[] = props.session.blocks
let knownSlots: readonly TranscriptSlot[] = props.session.slots ?? []
let loadedGeneration = generation.value
let anchor: { messageId: string; element: Element; top: number } | undefined

// What is drawn when not virtualized: the blocks, with the leaving rows where they were.
const rowList = computed(() => withLeaving(blocks.value, leaving.value))

function onGone(key: string): void {
  leaving.value = leaving.value.filter((row) => row.block.key !== key)
}

function markMaterializedFade(previousSlots: readonly TranscriptSlot[], currentSlots: readonly TranscriptSlot[]): void {
  const prevPlaceholders = new Set(
    previousSlots.filter((slot) => slot.kind === 'placeholder').map((slot) => slot.id),
  )
  const newlyMaterialized = currentSlots
    .filter((slot) => slot.kind === 'materialized' && prevPlaceholders.has(slot.id))
    .map((slot) => slot.id)
  if (newlyMaterialized.length === 0) return
  materializedFadeIds.value = new Set(newlyMaterialized)
  clearTimeout(fadeClearTimer)
  fadeClearTimer = setTimeout(() => {
    materializedFadeIds.value = new Set()
  }, FADE_DURATION_MS)
}

watch(
  [blocks, slots, generation],
  ([current, currentSlots]) => {
    const isLoad = generation.value !== loadedGeneration
    loadedGeneration = generation.value
    const currentKeys = new Set(current.map((block) => block.key))
    const previousKeys = new Set(knownBlocks.map((block) => block.key))
    const prepended = isLoad ? -1 : prependedCount(knownBlocks, current)
    const slotListNow = currentSlots ?? []
    const materializedFromPlaceholder =
      !isLoad &&
      knownSlots.some(
        (prev, index) => prev.kind === 'placeholder' && slotListNow[index]?.kind === 'materialized',
      )

    if (prepended > 0 || materializedFromPlaceholder) anchor = readAnchor()
    if (!isLoad && currentSlots) markMaterializedFade(knownSlots, currentSlots)

    const appeared = isLoad
      ? []
      : current.filter((block, i) => !previousKeys.has(block.key) && i >= Math.max(0, prepended))
    const staying = leaving.value.filter((row) => !currentKeys.has(row.block.key))
    const addedLeaving = isLoad ? [] : findLeaving(knownBlocks, currentKeys)
    leaving.value = isLoad ? [] : [...staying, ...addedLeaving]
    freshKeys.value = new Set(appeared.map((block) => block.key))
    if (isLoad) {
      measuredHeights.value = new Map()
      void nextTick(() => {
        syncViewportMetrics()
        jumpToEnd()
        measureRenderedMessages()
      })
    } else if (prepended > 0 || materializedFromPlaceholder) {
      void nextTick(() => {
        keepAnchor()
        measureRenderedMessages()
      })
    } else if (virtualized.value) {
      void nextTick(measureRenderedMessages)
    }
    if (!following.value && appeared.some((block) => block.kind === 'error')) emit('errorOutOfSight')
    knownBlocks = current
    knownSlots = slotListNow
  },
  { flush: 'pre' },
)

// The first row in view before rows are added/materialized above it, and where it was.
function readAnchor(): { messageId: string; element: Element; top: number } | undefined {
  const view = scroller.value
  const container = rows.value
  if (!view || !container) return undefined
  const scrollerTop = view.getBoundingClientRect().top
  const candidates = container.querySelectorAll<HTMLElement>('[data-message]')
  for (const el of candidates) {
    const rect = el.getBoundingClientRect()
    if (rect.bottom <= scrollerTop) continue
    const messageId = el.dataset.message ?? ''
    return { messageId, element: el, top: rect.top }
  }
  return undefined
}

function keepAnchor(): void {
  const view = scroller.value
  const container = rows.value
  if (!view || !anchor) return
  let target: Element | null = anchor.element.isConnected ? anchor.element : null
  if (!target && container && anchor.messageId) {
    target = container.querySelector(`[data-message="${CSS.escape(anchor.messageId)}"]`)
  }
  if (target) {
    view.scrollTop += target.getBoundingClientRect().top - anchor.top
  }
  anchor = undefined
}

// --- Older messages & debounced range loading.
function requestVisibleRange(): void {
  if (!virtualized.value || typeof props.session.requestRange !== 'function') return
  const { start, end } = visibleRange.value
  if (end < start) return
  props.session.requestRange(start, end + 1)
}

function loadOlderIfNear(): void {
  const view = scroller.value
  const { session } = props
  if (virtualized.value) {
    requestVisibleRange()
    // If placeholders exist at index 0, range loading handles it; otherwise fallback to loadOlder.
    if (slots.value?.[0]?.kind === 'placeholder') return
  }
  const cannotLoadOlder = !view || !session.moreBefore || session.loadingOlder || session.olderError
  if (cannotLoadOlder) return
  if (view.scrollTop < OLDER_TRIGGER_DISTANCE) void session.loadOlder()
}

watch(
  () => [props.session.moreBefore, props.session.loadingOlder],
  () => void nextTick(loadOlderIfNear),
)

watch([visibleRange, virtualized], () => requestVisibleRange())

function syncViewportMetrics(): void {
  const view = scroller.value
  if (view) {
    viewportTop.value = view.scrollTop
    viewportHeight.value = view.clientHeight
  }
  const width = rows.value?.clientWidth ?? 0
  if (width > 0) columnWidth.value = Math.min(metrics.contentWidth, width)
}

function jumpToEnd(): void {
  if (!scroller.value) return
  following.value = true
  scroller.value.scrollTop = scroller.value.scrollHeight
  viewportTop.value = scroller.value.scrollTop
  viewportHeight.value = scroller.value.clientHeight
}

function atEnd(): boolean {
  const view = scroller.value
  return view !== null && view.scrollHeight - view.clientHeight - view.scrollTop < AT_END_TOLERANCE
}

// --- Following.
const chasing = ref(false)
let gesturing = false
let settleEvictTimer: ReturnType<typeof setTimeout> | undefined

function runEvictionOnSettle(): void {
  if (!virtualized.value) return
  if (following.value) {
    props.session.evictAtBottom?.()
    return
  }
  const { start, end } = visibleRange.value
  if (end >= start) props.session.evictAround?.(start, end)
}

function scheduleSettleEviction(): void {
  clearTimeout(settleEvictTimer)
  settleEvictTimer = setTimeout(runEvictionOnSettle, SETTLE_EVICT_MS)
}

function chaseEnd(): void {
  if (following.value && !gesturing) chasing.value = true
}

useFrame(chasing, (frameSeconds) => {
  const view = scroller.value
  if (!view || !following.value || gesturing) {
    chasing.value = false
    return
  }
  const target = Math.max(0, view.scrollHeight - view.clientHeight)
  if (scrollArrived(view.scrollTop, target)) {
    chasing.value = false
    runEvictionOnSettle()
    return
  }
  view.scrollTop = scrollStep(view.scrollTop, target, frameSeconds)
  viewportTop.value = view.scrollTop
})

watch(following, (isFollowing) => {
  if (isFollowing) chaseEnd()
})

// A viewport that changes size (the window, or the input box unfolding below) keeps the end in
// view in the same frame; rows that change size (streaming text, folding bodies) are chased.
function pinToEnd(): void {
  const view = scroller.value
  if (!view) return
  view.scrollTop = Math.max(0, view.scrollHeight - view.clientHeight)
  viewportTop.value = view.scrollTop
}

const sizeObserver = new ResizeObserver((entries) => {
  syncViewportMetrics()
  if (virtualized.value) measureRenderedMessages()
  const viewportResized = entries.some((entry) => entry.target === scroller.value)
  if (following.value && !gesturing && viewportResized) pinToEnd()
  else chaseEnd()
})

// --- User gestures decide `following`.
let gestureTimer: ReturnType<typeof setTimeout> | undefined

function beginGesture(): void {
  gesturing = true
  chasing.value = false
  scheduleGestureEnd()
}

function scheduleGestureEnd(): void {
  clearTimeout(gestureTimer)
  gestureTimer = setTimeout(endGesture, GESTURE_IDLE_MS)
}

function endGesture(): void {
  gesturing = false
  following.value = atEnd()
  chaseEnd()
  runEvictionOnSettle()
}

function onWheel(event: WheelEvent): void {
  if (event.deltaY === 0 || event.ctrlKey) return // horizontal, or a zoom
  beginGesture()
  if (event.deltaY < 0) following.value = false
}

const KEYS_UP: readonly string[] = [Key.PageUp, Key.ArrowUp, Key.Home]
const KEYS_SCROLL: readonly string[] = [...KEYS_UP, Key.PageDown, Key.ArrowDown, Key.End, Key.Space]

// Keys scroll the view when nothing that takes keys has the focus.
function onKeydown(event: KeyboardEvent): void {
  if (!KEYS_SCROLL.includes(event.key) || event.ctrlKey || event.metaKey || event.altKey) return
  const { target } = event
  const isInsideScroller = target instanceof Node && scroller.value?.contains(target)
  if (target !== document.body && !isInsideScroller) return
  beginGesture()
  const isScrollUpKey = KEYS_UP.includes(event.key) || (event.key === Key.Space && event.shiftKey)
  if (isScrollUpKey) following.value = false
}

function onScroll(): void {
  if (scroller.value) {
    viewportTop.value = scroller.value.scrollTop
    viewportHeight.value = scroller.value.clientHeight
  }
  loadOlderIfNear()
  scheduleSettleEviction()
  if (!gesturing) return
  following.value = atEnd()
  scheduleGestureEnd()
}

function onPointerDown(event: PointerEvent): void {
  const onScrollBar = event.target === scroller.value && event.offsetX >= (scroller.value?.clientWidth ?? 0)
  if (!onScrollBar) return
  beginGesture()
  window.addEventListener('pointerup', endGesture, { once: true })
}

// --- Holding a pinned header while its body folds (ChatScroll.holdHeader).
const holdObserver = new ResizeObserver(() => correctForHeldHeader())
let held: { block: HTMLElement; headerHeight: number } | undefined

function correctForHeldHeader(): void {
  const view = scroller.value
  if (!held || !view) return
  const blockTop = held.block.getBoundingClientRect().top - view.getBoundingClientRect().top + view.scrollTop
  const pinLimit = blockTop + held.block.offsetHeight - held.headerHeight // furthest scroll that keeps the header pinned
  if (view.scrollTop <= pinLimit) return
  view.scrollTop = pinLimit
  following.value = atEnd()
}

// The message under the pointer lights its footer.
function onRowsOver(event: MouseEvent): void {
  const row = event.target instanceof Element ? event.target.closest<HTMLElement>('[data-message]') : null
  hoveredMessageId.value = row?.dataset.message ?? ''
}

provide(chatScrollKey, {
  scroller,
  hoveredMessageId,
  holdHeader(block, headerHeight) {
    holdObserver.disconnect()
    held = { block, headerHeight }
    holdObserver.observe(block)
  },
  releaseHeader() {
    holdObserver.disconnect()
    held = undefined
  },
})

// --- Session-wide image lightbox navigation.
const sessionImages = computed<ImageRef[]>(() => {
  const list: ImageRef[] = []
  for (const block of blocks.value) {
    if (block.kind !== 'user' && block.kind !== 'tool') continue
    for (const image of block.images) {
      if (image.id !== null) list.push(image)
    }
  }
  return list
})
const resolvedUrls = ref<Record<string, string>>({})
const lightboxOpen = ref(false)
const lightboxIndex = ref(0)
const everOpenedLightbox = ref(false)
const lightboxItems = computed<LightboxItem[]>(() =>
  sessionImages.value.map((image) => ({
    url: (image.id ? resolvedUrls.value[image.id] : undefined) ?? '',
    caption: imageCaption(image),
  })),
)

function ensureImageUrl(imageId: string | null | undefined): void {
  if (!imageId || resolvedUrls.value[imageId] || typeof props.session.imageUrl !== 'function') return
  props.session.imageUrl(imageId).then(
    (url) => {
      resolvedUrls.value = { ...resolvedUrls.value, [imageId]: url }
    },
    () => undefined,
  )
}

watch([lightboxOpen, lightboxIndex, sessionImages], ([open, index, list]) => {
  if (!open || list.length === 0) return
  const clamped = Math.max(0, Math.min(index, list.length - 1))
  ensureImageUrl(list[clamped]?.id)
  ensureImageUrl(list[clamped - 1]?.id)
  ensureImageUrl(list[clamped + 1]?.id)
})

provide(imageLightboxKey, {
  open(target: ImageRef) {
    const list = sessionImages.value
    let index = list.indexOf(target)
    if (index < 0 && target.id !== null) {
      index = list.findIndex((candidate) => candidate.id === target.id)
    }
    if (index < 0) return
    lightboxIndex.value = index
    everOpenedLightbox.value = true
    lightboxOpen.value = true
    ensureImageUrl(list[index]?.id)
  },
})

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  if (rows.value) sizeObserver.observe(rows.value)
  if (scroller.value) sizeObserver.observe(scroller.value)
  syncViewportMetrics()
  if (virtualized.value) {
    jumpToEnd()
    void nextTick(measureRenderedMessages)
  }
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  sizeObserver.disconnect()
  holdObserver.disconnect()
  clearTimeout(gestureTimer)
  clearTimeout(settleEvictTimer)
  clearTimeout(fadeClearTimer)
})
</script>

<template>
  <div
    ref="scroller"
    class="scroller"
    @wheel.passive="onWheel"
    @touchstart.passive="beginGesture"
    @touchend.passive="scheduleGestureEnd"
    @scroll.passive="onScroll"
    @pointerdown="onPointerDown"
  >
    <div ref="rows" class="rows" @mouseover="onRowsOver" @mouseleave="hoveredMessageId = ''">
      <OlderMessages
        v-if="session.moreBefore && (!virtualized || slots?.[0]?.kind !== 'placeholder')"
        :loading="session.loadingOlder"
        :error="session.olderError"
        @load="session.loadOlder()"
      />
      <template v-if="virtualized">
        <div
          v-if="topSpacerHeight > 0"
          class="virtual-spacer top"
          :style="{ height: `${topSpacerHeight}px` }"
        />
        <template v-for="slot in renderedSlots" :key="slot.id">
          <SkeletonBlock
            v-if="slot.kind === 'placeholder'"
            :meta="slot.meta"
            :height="slotHeight(slot.seq)"
          />
          <template v-else>
            <BlockView
              v-for="block in slot.blocks"
              :key="block.key"
              :class="{ 'materialized-fade': materializedFadeIds.has(slot.id) }"
              :block="block"
              :fresh="freshKeys.has(block.key)"
              :leaving="false"
              :data-message="block.messageId"
            />
          </template>
        </template>
        <BlockView
          v-if="showTailError && tailErrorBlock"
          :key="tailErrorBlock.key"
          :block="tailErrorBlock"
          :fresh="freshKeys.has(tailErrorBlock.key)"
          :leaving="false"
          :data-message="tailErrorBlock.messageId"
        />
        <BlockView
          v-for="row in leaving"
          :key="row.block.key"
          :block="row.block"
          :fresh="false"
          :leaving="row.leaving"
          :data-message="row.block.messageId"
          @gone="onGone(row.block.key)"
        />
        <div
          v-if="bottomSpacerHeight > 0"
          class="virtual-spacer bottom"
          :style="{ height: `${bottomSpacerHeight}px` }"
        />
      </template>
      <template v-else>
        <BlockView
          v-for="row in rowList"
          :key="row.block.key"
          :block="row.block"
          :fresh="freshKeys.has(row.block.key)"
          :leaving="row.leaving"
          :data-message="row.block.messageId"
          @gone="onGone(row.block.key)"
        />
      </template>
    </div>
    <div class="footer" />
    <ImageLightbox
      v-if="everOpenedLightbox"
      v-model:index="lightboxIndex"
      :open="lightboxOpen"
      :images="lightboxItems"
      @close="lightboxOpen = false"
    />
  </div>
</template>

<style scoped>
.scroller {
  height: 100%;
  overflow-x: hidden;
  overflow-y: auto;
  overflow-anchor: none; /* the view decides where to stay */
  padding: 0 var(--page-padding); /* top padding is on the rows: sticky headers pin to the padding edge */
}

.rows {
  padding-top: var(--page-padding);
}

.virtual-spacer {
  flex: none;
  pointer-events: none;
}

:deep(.skeleton-frame) + :deep(.skeleton-frame),
:deep(.frame) + :deep(.skeleton-frame),
:deep(.skeleton-frame) + :deep(.frame) {
  margin-top: var(--block-spacing);
}

.materialized-fade {
  animation: materialize-in var(--motion-quick) var(--motion-ease);
}

@keyframes materialize-in {
  from {
    opacity: 0;
  }

  to {
    opacity: 1;
  }
}

.footer {
  height: var(--page-padding);
}
</style>
