<script setup lang="ts">
// One conversation row: the frame, and the component for the block's kind.
import type { Block } from '../model/blocks.ts'
import BlockFrame from './BlockFrame.vue'
import CodeBlock from './CodeBlock.vue'
import ErrorBlock from './ErrorBlock.vue'
import MarkdownBlock from './MarkdownBlock.vue'
import ReasoningBlock from './ReasoningBlock.vue'
import ToolBlock from './ToolBlock.vue'
import UserBlock from './UserBlock.vue'

defineProps<{ block: Block; fresh: boolean }>()
</script>

<template>
  <BlockFrame :streaming="block.streaming" :fresh="fresh">
    <UserBlock v-if="block.kind === 'user'" :block="block" :fresh="fresh" />
    <ReasoningBlock v-else-if="block.kind === 'reasoning'" :block="block" />
    <MarkdownBlock v-else-if="block.kind === 'markdown'" :block="block" />
    <CodeBlock v-else-if="block.kind === 'code'" :block="block" />
    <ToolBlock v-else-if="block.kind === 'tool'" :block="block" />
    <ErrorBlock v-else-if="block.kind === 'error'" :block="block" :fresh="fresh" />
  </BlockFrame>
</template>
