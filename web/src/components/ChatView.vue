<script setup lang="ts">
// The conversation, one row per block, centered in a reading column (ChatView.qml).
//
// Following: while `following`, the scroll position chases the end with an exponential ease
// instead of jumping. Only user gestures change `following`: scrolling (wheel, touch) or
// dragging the scroll bar releases it, and ending such a gesture at the end re-engages it.
//
// Holding: while `turnInProgress` and following, the end never moves up. Streaming content can
// briefly take more room than it ends up needing (a line that wraps, then fits); the footer
// grows by what the content gave back, so the view does not bounce. When the turn ends, the
// held room is released with an animation.
//
// Older messages: a pill is the first row while more exist, and scrolling near the top loads
// the next page. The rows above the ones in view are added without moving what the user reads.
//
// Rows that go away (undo, a reset) stay as leaving rows until their exit animation is done.
//
// Every row stays in the DOM, so the scroll height is exact.
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
import type { Block } from '../model/blocks.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { findLeaving, prependedCount, withLeaving } from '../model/rowDiff.ts'
import type { LeavingRow } from '../model/rowDiff.ts'
import { scrollArrived, scrollStep } from '../motion/chase.ts'
import { tween } from '../motion/tween.ts'
import { useFrame } from '../motion/useFrame.ts'
import { motion, motionEasing } from '../theme/generated/motion.ts'
import BlockView from './BlockView.vue'
import OlderMessages from './OlderMessages.vue'
import { OLDER_TRIGGER_DISTANCE } from './toolLimits.ts'

const props = defineProps<{ session: ChatSession }>()
const blocks = computed(() => props.session.blocks)
const turnInProgress = computed(() => props.session.busy)
const generation = computed(() => props.session.generation) // changes when the history is loaded
const following = defineModel<boolean>('following', { default: true })
const emit = defineEmits<{ errorOutOfSight: [] }>()

const GESTURE_IDLE_MS = 150 // without scrolling, a user gesture has ended
const AT_END_TOLERANCE = 1 // px

const hoveredMessageId = ref('')
const scroller = useTemplateRef<HTMLElement>('scroller')
const rows = useTemplateRef<HTMLElement>('rows')
const footer = useTemplateRef<HTMLElement>('footer')

// --- Rows that appeared after the history was loaded play their entrance; rows that went away
// play their exit and are kept until it is over.
const freshKeys = shallowRef<ReadonlySet<string>>(new Set())
const leaving = shallowRef<readonly LeavingRow[]>([])
let knownBlocks: readonly Block[] = props.session.blocks
let loadedGeneration = generation.value
let anchor: { element: Element; top: number } | undefined

// What is drawn: the blocks, with the leaving rows where they were.
const rowList = computed(() => withLeaving(blocks.value, leaving.value))

function onGone(key: string): void {
  leaving.value = leaving.value.filter((row) => row.block.key !== key)
}

watch(
  [blocks, generation],
  ([current]) => {
    const isLoad = generation.value !== loadedGeneration
    loadedGeneration = generation.value
    const currentKeys = new Set(current.map((block) => block.key))
    const previousKeys = new Set(knownBlocks.map((block) => block.key))
    const prepended = isLoad ? -1 : prependedCount(knownBlocks, current)
    if (prepended > 0) anchor = readAnchor()
    const appeared = isLoad
      ? []
      : current.filter((block, i) => !previousKeys.has(block.key) && i >= prepended)
    const staying = leaving.value.filter((row) => !currentKeys.has(row.block.key))
    leaving.value = isLoad ? [] : [...staying, ...findLeaving(knownBlocks, currentKeys)]
    knownBlocks = current
    freshKeys.value = new Set(appeared.map((block) => block.key))
    if (isLoad) void nextTick(jumpToEnd)
    if (prepended > 0) void nextTick(keepAnchor)
    if (!following.value && appeared.some((block) => block.kind === 'error')) emit('errorOutOfSight')
  },
  { flush: 'pre' },
)

// The first row in view before rows are added above it, and where it was.
function readAnchor(): { element: Element; top: number } | undefined {
  const view = scroller.value
  const first = rows.value?.querySelector('[data-message]')
  return view && first ? { element: first, top: first.getBoundingClientRect().top } : undefined
}

function keepAnchor(): void {
  const view = scroller.value
  if (!view || !anchor) return
  view.scrollTop += anchor.element.getBoundingClientRect().top - anchor.top
  anchor = undefined
}

// --- Older messages.
function loadOlderIfNear(): void {
  const view = scroller.value
  const { session } = props
  if (!view || !session.moreBefore || session.loadingOlder || session.olderError) return
  if (view.scrollTop < OLDER_TRIGGER_DISTANCE) void session.loadOlder()
}

watch(
  () => [props.session.moreBefore, props.session.loadingOlder],
  () => void nextTick(loadOlderIfNear),
)

function jumpToEnd(): void {
  if (!scroller.value) return
  following.value = true
  setHeld(0)
  scroller.value.scrollTop = scroller.value.scrollHeight
}

// --- Geometry. The footer sits after the last row and carries the held room.
let heldSpace = 0
let highWater = 0 // furthest natural end of the current turn

function heldEnd(): number {
  return scroller.value?.scrollHeight ?? 0
}

function naturalEnd(): number {
  return heldEnd() - heldSpace
}

function atEnd(): boolean {
  const view = scroller.value
  return view !== null && view.scrollHeight - view.clientHeight - view.scrollTop < AT_END_TOLERANCE
}

function setHeld(space: number): void {
  heldSpace = space
  if (footer.value) footer.value.style.height = `calc(var(--page-padding) + ${space}px)`
  chaseEnd()
}

function holdEnd(): void {
  highWater = Math.max(highWater, naturalEnd())
  setHeld(highWater - naturalEnd())
}

// --- Following.
const chasing = ref(false)
let gesturing = false

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
    return
  }
  view.scrollTop = scrollStep(view.scrollTop, target, frameSeconds)
})

watch(following, (isFollowing) => {
  if (!isFollowing) return
  highWater = heldEnd()
  chaseEnd()
})

let cancelRelease: (() => void) | undefined
watch(
  () => turnInProgress.value,
  (inProgress) => {
    cancelRelease?.()
    if (inProgress) {
      highWater = heldEnd()
      holdEnd()
      return
    }
    cancelRelease = tween({
      from: heldSpace,
      to: 0,
      durationMs: motion.slow,
      easing: motionEasing.ease,
      update: setHeld,
    })
  },
)

// Rows resize (streaming text, folding bodies) and so does the window.
const sizeObserver = new ResizeObserver(() => {
  if (turnInProgress.value && following.value) holdEnd()
  chaseEnd()
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
}

function onWheel(event: WheelEvent): void {
  if (event.deltaY === 0 || event.ctrlKey) return // horizontal, or a zoom
  beginGesture()
  if (event.deltaY < 0) following.value = false
}

function onScroll(): void {
  loadOlderIfNear()
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

onMounted(() => {
  if (rows.value) sizeObserver.observe(rows.value)
  if (scroller.value) sizeObserver.observe(scroller.value)
})
onBeforeUnmount(() => {
  sizeObserver.disconnect()
  holdObserver.disconnect()
  clearTimeout(gestureTimer)
  cancelRelease?.()
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
        v-if="session.moreBefore"
        :loading="session.loadingOlder"
        :error="session.olderError"
        @load="session.loadOlder()"
      />
      <BlockView
        v-for="row in rowList"
        :key="row.block.key"
        :block="row.block"
        :fresh="freshKeys.has(row.block.key)"
        :leaving="row.leaving"
        :data-message="row.block.messageId"
        @gone="onGone(row.block.key)"
      />
    </div>
    <div ref="footer" class="footer" />
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

.footer {
  height: var(--page-padding);
}
</style>
