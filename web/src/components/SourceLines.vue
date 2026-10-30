<script setup lang="ts">
// File lines with a line number gutter (read, grep). Not wrapped; wide lines scroll sideways.
// In grep results, matches sit on a highlight and their context is dimmed.
import { computed } from 'vue'
import type { SourceFile } from '../model/toolContent.ts'
import CopyArea from './CopyArea.vue'

const props = defineProps<{ file: SourceFile; grep: boolean }>()

const GAP_MARK = '⋯'

const gutterDigits = computed(() => {
  const numbers = props.file.rows.flatMap((row) => (row === 'gap' ? [] : [row.number]))
  return String(Math.max(0, ...numbers)).length
})
const plainText = computed(() =>
  props.file.rows.map((row) => (row === 'gap' ? GAP_MARK : row.text)).join('\n'),
)
</script>

<template>
  <div class="file">
    <div class="path">
      <span class="name" :class="{ strong: grep }">{{ file.path }}</span>
      <span v-if="!grep" class="tag">#{{ file.tag }}</span>
    </div>
    <CopyArea :text="plainText">
      <div class="scroll">
        <div class="rows" :style="{ '--digits': gutterDigits }">
          <template v-for="(row, index) in file.rows" :key="index">
            <div v-if="row === 'gap'" class="row gap">
              <span class="gutter" />
              <span class="text">{{ GAP_MARK }}</span>
            </div>
            <div v-else class="row" :class="{ match: grep && row.match, context: grep && !row.match }">
              <span class="gutter">{{ row.number }}</span>
              <span class="text">{{ row.text }}</span>
            </div>
          </template>
        </div>
      </div>
    </CopyArea>
  </div>
</template>

<style scoped>
.file + .file {
  margin-top: 10px;
}

.path {
  margin-bottom: 2px;
  font: var(--small-font-size) / 18px var(--font-mono);
}

.strong {
  font-weight: bold;
}

.tag {
  margin-left: 6px;
  color: var(--text-muted);
  opacity: 0.7;
}

.scroll {
  overflow-x: auto;
}

.rows {
  min-width: max-content;
  font: var(--small-font-size) / 18px var(--font-mono);
}

.row {
  display: flex;
  gap: 12px;
  white-space: pre;
}

.gutter {
  flex: none;
  min-width: calc(var(--digits) * 1ch);
  color: color-mix(in srgb, var(--text-muted) 70%, transparent);
  font-size: 11px;
  text-align: right;
}

.match {
  background: var(--match-background);
}

.context,
.gap {
  color: var(--text-muted);
}
</style>
