<script setup lang="ts">
// Monospace text of a tool body: input, output, listings. Long text is cut behind a button.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { limitLines } from '../model/toolContent.ts'
import CopyArea from './CopyArea.vue'
import { PREVIEW_LINES } from './toolLimits.ts'

const props = defineProps<{ text: string; tone?: 'info' | 'muted' | 'danger' }>()

const { t } = useI18n()
const showAll = ref(false)
const limited = computed(() => limitLines(props.text, showAll.value ? undefined : PREVIEW_LINES))
</script>

<template>
  <CopyArea :text="text">
    <pre class="text" :class="tone">{{ limited.text }}</pre>
    <button v-if="limited.hidden > 0" type="button" class="more" @click="showAll = true">
      {{ t('tool.showMore', { count: limited.hidden }) }}
    </button>
  </CopyArea>
</template>

<style scoped>
.text {
  margin: 0;
  color: var(--text);
  font: var(--small-font-size) / 18px var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.info {
  color: var(--info);
}

.muted {
  color: var(--text-muted);
}

.danger {
  color: var(--danger);
}

.more {
  margin-top: 4px;
  color: var(--accent);
  font-size: var(--small-font-size);
}
</style>
