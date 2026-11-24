<script setup lang="ts">
import { computed } from 'vue'
import { toolIcon } from '../model/toolContent.ts'
import type { MessageLayoutMeta, PartLayoutMeta } from '../proto/generated'
import Icon from './Icon.vue'

const props = defineProps<{
  meta: MessageLayoutMeta
  height: number
}>()

const MAX_TEXT_BARS = 4
const MAX_TODO_BARS = 4

function textBarWidths(part: PartLayoutMeta): string[] {
  const count = Math.max(1, Math.min(MAX_TEXT_BARS, part.lines || Math.ceil(part.chars / 60)))
  const pattern = ['92%', '84%', '76%', '58%']
  return Array.from({ length: count }, (_, idx) => pattern[idx % pattern.length]!)
}

function todoRows(part: PartLayoutMeta): number {
  return Math.max(1, Math.min(MAX_TODO_BARS, part.tool_lines || 2))
}

function imageBoxStyle(part: PartLayoutMeta): Record<string, string> {
  const w = part.width && part.width > 0 ? Math.min(160, part.width) : 140
  const h = part.height && part.height > 0 ? Math.min(96, part.height) : 84
  return { width: `${w}px`, height: `${h}px` }
}

const isUser = computed(() => props.meta.role === 'user' && props.meta.source_kind !== 'compaction')
const isCompaction = computed(() => props.meta.source_kind === 'compaction')
</script>

<template>
  <div
    class="skeleton-frame"
    :style="{ minHeight: `${height}px` }"
    :data-seq="meta.seq"
    :data-message="meta.id"
    aria-hidden="true"
  >
    <div class="column">
      <div v-if="isCompaction" class="compaction-skeleton">
        <span class="rule" />
        <span class="pill-bar" />
        <span class="rule" />
      </div>

      <div v-else-if="isUser" class="user-skeleton">
        <div class="user-bubble">
          <div
            v-for="(part, idx) in meta.parts"
            :key="idx"
            class="user-part"
          >
            <div v-if="part.kind === 'image'" class="image-box small" />
            <div v-else class="lines">
              <span
                v-for="(width, lineIdx) in textBarWidths(part)"
                :key="lineIdx"
                class="bar"
                :style="{ width }"
              />
            </div>
          </div>
          <div v-if="meta.parts.length === 0" class="lines">
            <span class="bar" style="width: 65%" />
          </div>
        </div>
      </div>

      <div v-else class="assistant-skeleton">
        <template v-for="(part, idx) in meta.parts" :key="idx">
          <div v-if="part.kind === 'reasoning'" class="reasoning-pill">
            <span class="dot" />
            <span class="bar short" />
          </div>

          <div v-else-if="part.kind === 'tool' && part.tool === 'todo'" class="todo-card">
            <div class="tool-pill-head">
              <Icon :icon="toolIcon('todo')" class="tool-icon" />
              <span class="tool-name">todo</span>
            </div>
            <div class="todo-rows">
              <div v-for="row in todoRows(part)" :key="row" class="todo-row">
                <span class="todo-check" />
                <span class="bar" :style="{ width: row % 2 === 0 ? '62%' : '78%' }" />
              </div>
            </div>
          </div>

          <div v-else-if="part.kind === 'tool'" class="tool-pill">
            <Icon :icon="toolIcon(part.tool ?? '')" class="tool-icon" />
            <span v-if="part.tool" class="tool-name">{{ part.tool }}</span>
            <span class="bar tool-purpose" />
          </div>

          <div v-else-if="part.kind === 'image'" class="image-box" :style="imageBoxStyle(part)" />

          <div v-else class="text-part">
            <div class="lines">
              <span
                v-for="(width, lineIdx) in textBarWidths(part)"
                :key="lineIdx"
                class="bar"
                :style="{ width }"
              />
            </div>
            <div v-if="part.code_blocks > 0" class="code-box">
              <div class="code-head">
                <span class="bar tiny" />
              </div>
              <div class="lines">
                <span class="bar" style="width: 72%" />
                <span class="bar" style="width: 54%" />
              </div>
            </div>
          </div>
        </template>
        <div v-if="meta.parts.length === 0" class="lines">
          <span class="bar" style="width: 48%" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.skeleton-frame {
  overflow: hidden;
  box-sizing: border-box;
}

.skeleton-frame + .skeleton-frame,
:deep(.frame) + .skeleton-frame,
.skeleton-frame + :deep(.frame) {
  margin-top: var(--block-spacing);
}

.column {
  width: 100%;
  max-width: var(--content-width);
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: var(--block-spacing);
}

.bar {
  display: block;
  height: 12px;
  border-radius: 6px;
  background: var(--surface);
  animation: skeleton-pulse 1.6s ease-in-out infinite;
}

.bar.short {
  width: 76px;
}

.bar.tiny {
  width: 48px;
  height: 10px;
}

@keyframes skeleton-pulse {
  0%,
  100% {
    opacity: 0.55;
  }

  50% {
    opacity: 0.9;
  }
}

@media (prefers-reduced-motion: reduce) {
  .bar {
    animation: none;
    opacity: 0.7;
  }
}

.compaction-skeleton {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 0;
}

.rule {
  flex: 1;
  height: 1px;
  background: var(--border);
}

.pill-bar {
  width: 120px;
  height: 16px;
  border-radius: 999px;
  background: var(--surface);
}

.user-skeleton {
  display: flex;
}

.user-bubble {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: min(260px, 72%);
  max-width: min(520px, 100%);
  padding: 10px 14px;
  border-radius: var(--radius-large);
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
}

.user-bubble .bar {
  background: color-mix(in srgb, var(--text-muted) 22%, transparent);
}

.lines {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.assistant-skeleton {
  display: flex;
  flex-direction: column;
  gap: var(--block-spacing);
}

.reasoning-pill {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 28px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--surface);
}

.tool-pill,
.tool-card {
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.tool-pill {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 38px;
  padding: 0 14px;
}

.tool-pill-head {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 38px;
  padding: 0 14px;
  border-bottom: 1px solid var(--border);
}

.tool-icon {
  flex: none;
  color: var(--text-muted);
}

.tool-name {
  flex: none;
  color: var(--text-muted);
  font: bold var(--code-font-size) var(--font-mono);
}

.tool-purpose {
  flex: 0 1 180px;
}

.todo-rows {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 14px;
}

.todo-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.todo-check {
  width: 12px;
  height: 12px;
  flex: none;
  border-radius: 3px;
  box-shadow: inset 0 0 0 1px var(--border);
}

.image-box {
  border-radius: var(--radius-small);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.image-box.small {
  width: 72px;
  height: 72px;
}

.text-part {
  display: flex;
  flex-direction: column;
  gap: var(--block-spacing);
}

.code-box {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px 14px 12px;
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
}
</style>
