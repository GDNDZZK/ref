<script setup lang="ts">
// The model's reasoning: open while it streams, folded away once it ends (ReasoningBlock.qml).
import { computed, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChatScroll } from '../composables/chatScroll.ts'
import { useStuck } from '../composables/useStuck.ts'
import { renderMarkdown } from '../markdown/markdown.ts'
import type { ReasoningBlock } from '../model/blocks.ts'
import { usePulse } from '../motion/usePulse.ts'
import { motion } from '../theme/generated/motion.ts'
import Chevron from './Chevron.vue'

const props = defineProps<{ block: ReasoningBlock }>()

const PULSE_DIMMED = 0.5

const { t } = useI18n()
const chatScroll = useChatScroll()
const root = useTemplateRef<HTMLElement>('root')
const header = useTemplateRef<HTMLElement>('header')
const title = useTemplateRef<HTMLElement>('title')
const sentinel = useTemplateRef<HTMLElement>('sentinel')
const stuck = useStuck(sentinel)

const chosen = ref<boolean>() // set once the user toggles; until then it follows streaming
const expanded = computed(() => chosen.value ?? props.block.streaming)
const streaming = computed(() => props.block.streaming)
const html = computed(() => renderMarkdown(props.block.text))

usePulse(title, streaming, { dimmed: PULSE_DIMMED, period: motion.calmPulse })

function toggle(): void {
  if (expanded.value && stuck.value && root.value && header.value) {
    chatScroll.holdHeader(root.value, header.value.offsetHeight)
  }
  chosen.value = !expanded.value
}

function onFolded(event: TransitionEvent): void {
  if (event.propertyName === 'grid-template-rows') chatScroll.releaseHeader()
}
</script>

<template>
  <div ref="root" class="reasoning">
    <span ref="sentinel" class="sentinel" />
    <button
      ref="header"
      type="button"
      class="header"
      :class="{ stuck }"
      :aria-expanded="expanded"
      @click="toggle"
    >
      <Chevron class="chevron" :class="{ open: expanded }" />
      <span ref="title" class="title">{{ block.streaming ? t('chat.thinking') : t('chat.thought') }}</span>
    </button>
    <div
      class="body"
      :class="{ open: expanded }"
      @transitionend.self="onFolded"
      @transitioncancel.self="onFolded"
    >
      <div class="fold">
        <div class="thoughts">
          <span class="rule" />
          <!-- eslint-disable-next-line vue/no-v-html -->
          <div class="prose" v-html="html" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.reasoning {
  position: relative;
}

.sentinel {
  position: absolute;
  top: 0;
  left: 0;
  width: 1px;
  height: 1px;
  pointer-events: none;
}

/* Pinned to the top of the view while the thoughts scroll past under it. */
.header {
  position: sticky;
  top: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  height: 28px;
  background: var(--background);
  box-shadow: 0 1px 0 transparent;
  color: var(--text-muted);
  font-size: calc(var(--small-font-size) + 1px);
  text-align: left;
}

.header.stuck {
  box-shadow: 0 1px 0 var(--border);
}

.chevron {
  transition: rotate var(--motion-standard) var(--motion-ease);
}

.chevron.open {
  rotate: 90deg;
}

.body {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.body.open {
  grid-template-rows: 1fr;
}

.fold {
  min-height: 0;
  overflow: hidden;
}

.thoughts {
  position: relative;
  padding: 0 0 6px 18px;
  color: var(--text-muted);
  font-size: calc(var(--font-size) - 1px);
}

.rule {
  position: absolute;
  top: 0;
  bottom: 6px;
  left: 5px;
  width: 2px;
  border-radius: 1px;
  background: var(--border);
}
</style>
