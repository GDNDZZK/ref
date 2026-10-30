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
// Every row stays in the DOM, so the scroll height is exact.
import { nextTick, onBeforeUnmount, onMounted, provide, ref, shallowRef, useTemplateRef, watch } from 'vue'
import { chatScrollKey } from '../composables/chatScroll.ts'
import type { Block } from '../model/blocks.ts'
import { scrollArrived, scrollStep } from '../motion/chase.ts'
import { tween } from '../motion/tween.ts'
import { useFrame } from '../motion/useFrame.ts'
import { motion, motionEasing } from '../theme/generated/motion.ts'
import BlockView from './BlockView.vue'

const props = defineProps<{
  blocks: readonly Block[]
  turnInProgress: boolean
  generation: number // changes when the history is loaded, not for rows added after
}>()
const following = defineModel<boolean>('following', { default: true })
const emit = defineEmits<{ errorOutOfSight: [] }>()

const GESTURE_IDLE_MS = 150 // without scrolling, a user gesture has ended
const AT_END_TOLERANCE = 1 // px

const scroller = useTemplateRef<HTMLElement>('scroller')
const rows = useTemplateRef<HTMLElement>('rows')
const footer = useTemplateRef<HTMLElement>('footer')

// --- Rows that appeared after the history was loaded play their entrance.
const freshKeys = shallowRef<ReadonlySet<string>>(new Set())
let knownKeys = new Set<string>()
let loadedGeneration = props.generation

watch(
  [() => props.blocks, () => props.generation],
  ([blocks]) => {
    const isLoad = props.generation !== loadedGeneration
    loadedGeneration = props.generation
    const appeared = isLoad ? [] : blocks.filter((block) => !knownKeys.has(block.key))
    knownKeys = new Set(blocks.map((block) => block.key))
    freshKeys.value = new Set(appeared.map((block) => block.key))
    if (isLoad) void nextTick(jumpToEnd)
    if (!following.value && appeared.some((block) => block.kind === 'error')) emit('errorOutOfSight')
  },
  { flush: 'pre' },
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
  () => props.turnInProgress,
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
  if (props.turnInProgress && following.value) holdEnd()
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

provide(chatScrollKey, {
  scroller,
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
    <div ref="rows" class="rows">
      <BlockView v-for="block in blocks" :key="block.key" :block="block" :fresh="freshKeys.has(block.key)" />
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
  padding: var(--page-padding) var(--page-padding) 0;
}

.footer {
  height: var(--page-padding);
}
</style>
