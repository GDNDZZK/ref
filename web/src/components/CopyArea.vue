<script setup lang="ts">
// Wraps text-like content and adds a copy button in its top right corner, shown on hover.
// The icon turns into a check for a moment after copying.
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNoticesStore } from '../stores/notices.ts'
import { motion } from '../theme/generated/motion.ts'
import Icon from './Icon.vue'

const props = defineProps<{ text: string }>()

const { t } = useI18n()
const notices = useNoticesStore()
const copied = ref(false)
let resetTimer: ReturnType<typeof setTimeout> | undefined

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(props.text)
  } catch (error) {
    notices.pushError(error)
    return
  }
  copied.value = true
  clearTimeout(resetTimer)
  resetTimer = setTimeout(() => (copied.value = false), motion.slow)
}

onBeforeUnmount(() => clearTimeout(resetTimer))
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
