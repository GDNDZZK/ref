<script setup lang="ts">
// One conversation row: the frame, and the component for the block's kind.
import { useTemplateRef, watch } from 'vue'
import type { Block } from '../model/blocks.ts'
import { cssEasing } from '../motion/cssEasing.ts'
import { motion } from '../theme/generated/motion.ts'
import BlockFrame from './BlockFrame.vue'
import CodeBlock from './CodeBlock.vue'
import CompactionBlock from './CompactionBlock.vue'
import ErrorBlock from './ErrorBlock.vue'
import MarkdownBlock from './MarkdownBlock.vue'
import MetaRow from './MetaRow.vue'
import ReasoningBlock from './ReasoningBlock.vue'
import ToolBlock from './ToolBlock.vue'
import UserBlock from './UserBlock.vue'

const props = defineProps<{ block: Block; fresh: boolean; leaving: boolean }>()
const emit = defineEmits<{ gone: [] }>()

const frame = useTemplateRef<InstanceType<typeof BlockFrame>>('frame')

// A removed block fades out, then its height closes so the rows below move up with it.
watch(
  () => props.leaving,
  async (isLeaving) => {
    const element = frame.value?.$el as HTMLElement | undefined
    if (!isLeaving || !element) return
    const marginTop = getComputedStyle(element).marginTop
    const fade = element.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: motion.quick,
      fill: 'forwards',
    })
    await fade.finished
    const close = element.animate(
      [
        { height: `${element.offsetHeight}px`, marginTop },
        { height: '0px', marginTop: '0px' },
      ],
      { duration: motion.standard, easing: cssEasing('--motion-ease'), fill: 'forwards' },
    )
    await close.finished
    emit('gone')
  },
)
</script>

<template>
  <BlockFrame
    ref="frame"
    :streaming="block.streaming"
    :fresh="fresh"
    :stale="block.stale"
    :tight="block.kind === 'meta'"
  >
    <UserBlock v-if="block.kind === 'user'" :block="block" :fresh="fresh" />
    <ReasoningBlock v-else-if="block.kind === 'reasoning'" :block="block" />
    <MarkdownBlock v-else-if="block.kind === 'markdown'" :block="block" />
    <CodeBlock v-else-if="block.kind === 'code'" :block="block" />
    <ToolBlock v-else-if="block.kind === 'tool'" :block="block" />
    <MetaRow v-else-if="block.kind === 'meta'" :block="block" />
    <CompactionBlock v-else-if="block.kind === 'compaction'" :block="block" />
    <ErrorBlock v-else :block="block" :fresh="fresh" />
  </BlockFrame>
</template>
