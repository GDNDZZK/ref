<script setup lang="ts">
// Base of every conversation row (Block.qml): centers its content in the reading column.
//
// A row that appears after the session was opened fades in and drops into place.
//
// While the block streams, its height chases the content height with a per-frame exponential
// ease, so new lines slide in instead of popping. A chase, not a CSS transition: a transition
// restarts whenever its target changes, and content that is itself animating (a reasoning body
// easing open) changes it every frame, which would keep restarting it and stall the height.
import { onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { arrived, chase } from '../motion/chase.ts'
import { cssEasing } from '../motion/cssEasing.ts'
import { useFrame } from '../motion/useFrame.ts'
import { motion } from '../theme/generated/motion.ts'

const props = defineProps<{ streaming: boolean; fresh: boolean }>()

const frame = useTemplateRef<HTMLElement>('frame')
const column = useTemplateRef<HTMLElement>('column')

let naturalHeight = 0
let measured = false
let easedHeight = 0 // starts at the natural height; the observer reports it after the first layout
const easing = ref(false) // from the start of streaming until caught up after it ends

useFrame(easing, (frameSeconds) => {
  if (!frame.value || !measured) return
  easedHeight = chase(easedHeight, naturalHeight, frameSeconds)
  easing.value = props.streaming || !arrived(easedHeight, naturalHeight)
  frame.value.style.height = easing.value ? `${easedHeight}px` : ''
})

watch(
  () => props.streaming,
  (isStreaming) => {
    if (!isStreaming || easing.value || !frame.value) return
    easedHeight = frame.value.offsetHeight
    easing.value = true
  },
)

const sizeObserver = new ResizeObserver((entries) => {
  const size = entries[0]?.borderBoxSize[0]
  if (!size) return
  if (!measured) easedHeight = size.blockSize
  measured = true
  naturalHeight = size.blockSize
})

onMounted(() => {
  if (!frame.value || !column.value) return
  easing.value = props.streaming
  sizeObserver.observe(column.value)
  if (props.fresh) enter(frame.value)
})
onBeforeUnmount(() => sizeObserver.disconnect())

function enter(element: HTMLElement): void {
  element.animate(
    [
      { opacity: 0, transform: `translateY(${-motion.enterDrop}px)` },
      { opacity: 1, transform: 'translateY(0)' },
    ],
    { duration: motion.standard, easing: cssEasing('--motion-ease') },
  )
}
</script>

<template>
  <div ref="frame" class="frame">
    <div ref="column" class="column"><slot /></div>
  </div>
</template>

<style scoped>
.frame {
  overflow: clip;
}

.frame + .frame {
  margin-top: var(--block-spacing);
}

.column {
  width: 100%;
  max-width: var(--content-width);
  margin: 0 auto;
  display: flow-root;
}
</style>
