<script setup lang="ts">
// Pulsing dot that trails the end of streaming text (StreamCaret.qml). It chases the end rather
// than transitioning to it: the end moves every frame while text streams in.
import { ref, useTemplateRef } from 'vue'
import { arrived, chase } from '../motion/chase.ts'
import { useFrame } from '../motion/useFrame.ts'
import { usePulse } from '../motion/usePulse.ts'

export interface CaretAnchor {
  x: number // end of the last character, relative to the caret's parent
  y: number // top of its line
  height: number
}

const props = defineProps<{ anchor: () => CaretAnchor | undefined }>()

const DOT_SIZE = 8
const END_GAP = 3
const PULSE_DIMMED = 0.5

const dot = useTemplateRef<HTMLElement>('dot')
const placed = ref(false) // hidden until the end of the text is known
const alwaysOn = ref(true) // the caret exists only while text streams
let x = 0
let y = 0

usePulse(dot, alwaysOn, { dimmed: PULSE_DIMMED })

useFrame(alwaysOn, (frameSeconds) => {
  const anchor = props.anchor()
  if (!dot.value || !anchor) return
  const targetX = anchor.x + END_GAP
  const targetY = anchor.y + (anchor.height - DOT_SIZE) / 2
  if (!placed.value) {
    x = targetX
    y = targetY
    placed.value = true
  }
  if (!arrived(x, targetX) || !arrived(y, targetY)) {
    x = chase(x, targetX, frameSeconds)
    y = chase(y, targetY, frameSeconds)
  }
  dot.value.style.translate = `${x}px ${y}px`
})
</script>

<template>
  <span v-show="placed" ref="dot" class="caret" />
</template>

<style scoped>
.caret {
  position: absolute;
  left: 0;
  top: 0;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent);
  pointer-events: none;
}
</style>
