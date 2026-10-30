<script setup lang="ts">
// The "Output" part of a tool card. While a command runs, the tail of its live output.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolBlock } from '../model/blocks.ts'
import { isFinalToolState } from '../model/blocks.ts'
import { metaString, tailLines } from '../model/toolContent.ts'
import ToolSection from './ToolSection.vue'
import ToolText from './ToolText.vue'
import { LIVE_TAIL_LINES } from './toolLimits.ts'

const props = defineProps<{ block: ToolBlock }>()

const { t } = useI18n()

const live = computed(() =>
  props.block.state === 'running'
    ? tailLines(metaString(props.block.meta, 'live_output') ?? '', LIVE_TAIL_LINES)
    : undefined,
)
const text = computed(() => props.block.output?.trimEnd() ?? '')
const finished = computed(() => isFinalToolState(props.block.state))
const visible = computed(() => (live.value ? live.value.text !== '' : text.value !== '' || finished.value))
</script>

<template>
  <ToolSection v-if="visible" :label="t('tool.output')">
    <template #chips><slot name="chips" /></template>
    <template v-if="live">
      <div v-if="live.hidden > 0" class="earlier">{{ t('tool.earlierLines', { count: live.hidden }) }}</div>
      <ToolText :text="live.text" />
    </template>
    <ToolText v-else-if="text !== ''" :text="text" :tone="block.state === 'error' ? 'danger' : undefined" />
    <div v-else class="empty">{{ t('tool.noOutput') }}</div>
  </ToolSection>
</template>

<style scoped>
.earlier,
.empty {
  color: var(--text-muted);
  font: var(--small-font-size) / 18px var(--font-mono);
}
</style>
