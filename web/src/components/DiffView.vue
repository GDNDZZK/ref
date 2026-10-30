<script setup lang="ts">
// A unified diff: colored rows across the full width, a sign column, no wrapping.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { parseDiff } from '../model/toolContent.ts'
import CopyArea from './CopyArea.vue'
import { PREVIEW_LINES } from './toolLimits.ts'

const props = defineProps<{ diff: string }>()

const { t } = useI18n()
const showAll = ref(false)
const rows = computed(() => parseDiff(props.diff))
const shown = computed(() => (showAll.value ? rows.value : rows.value.slice(0, PREVIEW_LINES)))
</script>

<template>
  <CopyArea :text="diff">
    <div class="scroll">
      <div class="rows">
        <div v-for="(row, index) in shown" :key="index" class="row" :class="row.kind">
          <span class="sign">{{ row.sign }}</span>
          <span class="text">{{ row.text }}</span>
        </div>
      </div>
    </div>
    <button v-if="shown.length < rows.length" type="button" class="more" @click="showAll = true">
      {{ t('tool.showMore', { count: rows.length - shown.length }) }}
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
  color: var(--info);
}

.file {
  font-weight: bold;
}

.more {
  margin-top: 4px;
  color: var(--accent);
  font-size: var(--small-font-size);
}
</style>
