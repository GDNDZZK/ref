<script setup lang="ts">
// Wraps text-like content and adds a copy button in its top right corner, shown on hover.
// The icon turns into a check for a moment after copying.
import { useI18n } from 'vue-i18n'
import { useCopyFeedback } from '../composables/useCopyFeedback.ts'
import { motion } from '../theme/generated/motion.ts'
import Icon from './Icon.vue'

const props = defineProps<{ text: string }>()

const { t } = useI18n()
const { copied, copy } = useCopyFeedback(() => navigator.clipboard.writeText(props.text), motion.slow)
</script>

<template>
  <div class="area">
    <slot />
    <button type="button" class="copy" :aria-label="t('code.copy')" :title="t('code.copy')" @click="copy">
      <Icon :icon="copied ? 'check-16' : 'copy-16'" />
    </button>
  </div>
</template>

<style scoped>
.area {
  position: relative;
}

.copy {
  position: absolute;
  top: 4px;
  right: 4px;
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-small);
  background: var(--surface);
  color: var(--text-muted);
  opacity: 0;
  transition:
    opacity var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.area:hover .copy,
.copy:focus-visible {
  opacity: 1;
}

.copy:hover {
  color: var(--text);
}
</style>
