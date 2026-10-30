<script setup lang="ts">
// A fenced code block: language label, copy action and source (CodeBlock.qml).
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { CodeBlock } from '../model/blocks.ts'
import { useHighlight } from '../highlight/useHighlight.ts'
import { useNoticesStore } from '../stores/notices.ts'
import TextButton from './TextButton.vue'

const props = defineProps<{ block: CodeBlock }>()

const COPIED_SHOWN_MS = 1500

const { t } = useI18n()
const notices = useNoticesStore()
const copied = ref(false)
const tokens = useHighlight(() => props.block)
let resetTimer: ReturnType<typeof setTimeout> | undefined

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(props.block.text)
  } catch (error) {
    notices.pushError(error)
    return
  }
  copied.value = true
  clearTimeout(resetTimer)
  resetTimer = setTimeout(() => (copied.value = false), COPIED_SHOWN_MS)
}

onBeforeUnmount(() => clearTimeout(resetTimer))
</script>

<template>
  <div class="code">
    <div class="header">
      <span class="language">{{ block.language || t('code.plainText') }}</span>
      <TextButton :label="copied ? t('code.copied') : t('code.copy')" @click="copy" />
    </div>
    <pre
      class="source"
    ><span v-for="(token, index) in tokens" :key="index" class="token" :style="token.style">{{ token.content }}</span></pre>
  </div>
</template>

<style scoped>
.code {
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
}

.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 34px;
  padding: 0 8px 0 14px;
}

.language {
  color: var(--text-muted);
  font: var(--small-font-size) var(--font-mono);
}

.source {
  margin: 0;
  padding: 0 14px 12px;
  font: var(--code-font-size) / 1.45 var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.token {
  color: var(--shiki-light);
  font-weight: var(--shiki-light-font-weight);
  font-style: var(--shiki-light-font-style);
  text-decoration: var(--shiki-light-text-decoration);
}

:root[data-theme='dark'] .token {
  color: var(--shiki-dark);
  font-weight: var(--shiki-dark-font-weight);
  font-style: var(--shiki-dark-font-style);
  text-decoration: var(--shiki-dark-text-decoration);
}
</style>
