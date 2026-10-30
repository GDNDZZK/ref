<script setup lang="ts">
// The message that summarizes the earlier conversation: a divider with a pill in the middle.
// A click unfolds the summary. Messages above stay visible but are no longer sent to the model.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { renderMarkdown } from '../markdown/markdown.ts'
import type { CompactionBlock } from '../model/blocks.ts'
import { formatClock } from '../model/format.ts'
import Chevron from './Chevron.vue'
import CopyArea from './CopyArea.vue'
import Icon from './Icon.vue'

const props = defineProps<{ block: CompactionBlock }>()

const { t } = useI18n()
const expanded = ref(false)
const opened = ref(false) // the summary is rendered on first unfold
const html = computed(() => (opened.value ? renderMarkdown(props.block.text) : ''))

function toggle(): void {
  expanded.value = !expanded.value
  opened.value ||= expanded.value
}
</script>

<template>
  <div class="compaction">
    <div class="line">
      <span class="rule" />
      <button type="button" class="pill" :aria-expanded="expanded" @click="toggle">
        <Icon icon="history-16" />
        <span class="title">{{ t('compaction.title') }}</span>
        <span class="time">{{ formatClock(block.time) }}</span>
        <Chevron class="chevron" :class="{ open: expanded }" />
      </button>
      <span class="rule" />
    </div>
    <div class="summary" :class="{ open: expanded }">
      <div class="fold">
        <CopyArea :text="block.text">
          <!-- eslint-disable-next-line vue/no-v-html -->
          <div class="prose text" v-html="html" />
        </CopyArea>
      </div>
    </div>
    <div v-if="block.last" class="note">{{ t('compaction.note') }}</div>
  </div>
</template>

<style scoped>
.compaction {
  padding: 4px 0;
}

.line {
  display: flex;
  align-items: center;
  gap: 12px;
}

.rule {
  flex: 1;
  height: 1px;
  background: var(--border);
}

.pill {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--accent);
}

.title {
  font-size: 13px;
  font-weight: bold;
}

.time {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.chevron {
  color: var(--text-muted);
  transition: rotate var(--motion-standard) var(--motion-ease);
}

.chevron.open {
  rotate: 90deg;
}

.summary {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.summary.open {
  grid-template-rows: 1fr;
}

.fold {
  min-height: 0;
  overflow: hidden;
}

.text {
  margin-top: 10px;
  padding-left: 12px;
  border-left: 2px solid var(--accent);
  color: var(--text-muted);
  font-size: 13px;
}

.note {
  margin-top: 8px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-style: italic;
  opacity: 0.7;
  text-align: center;
}
</style>
