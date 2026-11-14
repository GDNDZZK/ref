<script setup lang="ts">
// A unified diff: colored rows across the full width, a sign column, no wrapping.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { parseDiff } from '../model/toolContent.ts'
import CopyArea from './CopyArea.vue'
import { PREVIEW_LINES } from './toolLimits.ts'

const props = defineProps<{ diff: string; maxRows?: number }>()

const { t } = useI18n()
const showAll = ref(false)
const rows = computed(() => parseDiff(props.diff))
const limit = computed(() => props.maxRows ?? PREVIEW_LINES)
const shown = computed(() =>
  showAll.value && props.maxRows === undefined ? rows.value : rows.value.slice(0, limit.value),
)
const hidden = computed(() => rows.value.length - shown.value.length)
</script>

<template>
  <div v-if="maxRows !== undefined">
    <div class="scroll">
      <div class="rows">
        <div v-for="(row, index) in shown" :key="index" class="row" :class="row.kind">
          <span class="sign">{{ row.sign }}</span>
          <span class="text">{{ row.text }}</span>
        </div>
      </div>
    </div>
    <div v-if="hidden > 0" class="more-note">{{ t('tool.moreDiffRows', { count: hidden }) }}</div>
  </div>
  <CopyArea v-else :text="diff">
    <div class="scroll">
      <div class="rows">
        <div v-for="(row, index) in shown" :key="index" class="row" :class="row.kind">
          <span class="sign">{{ row.sign }}</span>
          <span class="text">{{ row.text }}</span>
        </div>
      </div>
    </div>
    <button v-if="hidden > 0" type="button" class="more" @click="showAll = true">
      {{ t('tool.showMore', { count: hidden }) }}
    </button>
  </CopyArea>
</template>

<style scoped>
.scroll {
  overflow-x: auto;
}

.rows {
  min-width: max-content;
  font: var(--small-font-size) / 18px var(--font-mono);
}

.row {
  display: flex;
  color: var(--text-muted);
  white-space: pre;
}

.sign {
  flex: none;
  width: 16px;
  text-align: center;
}

.added {
  background: var(--diff-added-background);
  color: var(--diff-added);
}

.removed {
  background: var(--diff-removed-background);
  color: var(--diff-removed);
}

.hunk {
  background: var(--surface);
  color: var(--info-text);
}

.file {
  font-weight: bold;
}

.more {
  margin-top: 4px;
  color: var(--accent-text);
  font-size: var(--small-font-size);
}

.more-note {
  margin-top: 4px;
  color: var(--text-muted);
  font: var(--small-font-size) var(--font-mono);
}
</style>
