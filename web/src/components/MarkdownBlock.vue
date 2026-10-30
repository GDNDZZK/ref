<script setup lang="ts">
// Assistant prose, rendered as Markdown while it streams (MarkdownBlock.qml).
import { computed, useTemplateRef } from 'vue'
import { renderMarkdown } from '../markdown/markdown.ts'
import type { MarkdownBlock } from '../model/blocks.ts'
import StreamCaret from './StreamCaret.vue'
import type { CaretAnchor } from './StreamCaret.vue'

const props = defineProps<{ block: MarkdownBlock }>()

const root = useTemplateRef<HTMLElement>('root')
const html = computed(() => renderMarkdown(props.block.text))

// Where the last character of the text ends, relative to the block.
function textEnd(): CaretAnchor | undefined {
  const host = root.value
  if (!host) return undefined
  const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT)
  let last: Text | undefined
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.textContent?.trim()) last = node as Text
  }
  if (!last) return undefined
  const range = document.createRange()
  range.setStart(last, last.length - 1)
  range.setEnd(last, last.length)
  const rect = range.getBoundingClientRect()
  const origin = host.getBoundingClientRect()
  return { x: rect.right - origin.left, y: rect.top - origin.top, height: rect.height }
}
</script>

<template>
  <div ref="root" class="markdown">
    <!-- eslint-disable-next-line vue/no-v-html -->
    <div class="prose" v-html="html" />
    <StreamCaret v-if="block.streaming" :anchor="textEnd" />
  </div>
</template>

<style scoped>
.markdown {
  position: relative;
}
</style>
