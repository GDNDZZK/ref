<script setup lang="ts">
// One session in the split view (SplitView.qml's tile): a header with its state and a zoom button,
// and the session itself below. Zoomed, the header folds away and the frame fills the area.
import { computed, nextTick, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNow } from '../composables/useNow.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { liveLabel, needsAttention, rowState, runningTime, todoProgress } from '../shell/sessionRow.ts'
import AlertBadge from './AlertBadge.vue'
import Icon from './Icon.vue'
import SessionDot from './SessionDot.vue'
import SessionPane from './SessionPane.vue'

const BADGE_SIZE = 14
const PRESS_TARGETS = 'button, a, summary'

const props = defineProps<{ session: ChatSession; focused: boolean; zoomed: boolean; hidden: boolean }>()
const emit = defineEmits<{ focus: []; zoom: [] }>()

const { t } = useI18n()
const now = useNow()

// A press on a button focuses the tile only once the click is through: the composer unfolding
// would move the button from under the pointer, and the click would be lost.
// The press itself moves the keyboard focus off the box, so a tile that took the focus with this
// click puts the caret into its box again once the click is through.
const pane = useTemplateRef<InstanceType<typeof SessionPane>>('pane')
let focusedByPress = false

function onPointerDown(event: PointerEvent): void {
  focusedByPress = !props.focused
  if (event.target instanceof Element && event.target.closest(PRESS_TARGETS)) return
  emit('focus')
}

async function onClick(): Promise<void> {
  emit('focus')
  if (!focusedByPress) return
  focusedByPress = false
  await nextTick()
  pane.value?.focusComposer()
}

const info = computed(() => props.session.info)
const state = computed(() => (info.value ? rowState(info.value) : 'idle'))
const title = computed(() => (info.value ? liveLabel(info.value) : props.session.id))
const progress = computed(() => todoProgress(info.value?.todos))
const elapsed = computed(() => (info.value ? runningTime(info.value, now.value) : undefined))
</script>

<template>
  <div
    class="tile"
    :class="{ focused, zoomed, hidden }"
    :data-tile="session.id"
    @pointerdown.capture="onPointerDown"
    @click="onClick"
  >
    <header
      class="header"
      :class="{ permission: state === 'permission', error: state === 'error' }"
      @dblclick="emit('zoom')"
    >
      <SessionDot :state="state" />
      <span class="title">{{ title }}</span>
      <span v-if="needsAttention(state)" class="mark">
        <Icon v-if="state === 'permission'" icon="lock-16" class="lock" />
        <AlertBadge v-else :size="BADGE_SIZE" />
      </span>
      <span v-else-if="progress" class="progress" :class="{ complete: progress.complete }">
        <Icon v-if="progress.complete" icon="check-16" />{{ progress.done }}/{{ progress.total }}
      </span>
      <span v-if="elapsed" class="elapsed">{{ elapsed }}</span>
      <button
        type="button"
        class="zoom"
        :aria-label="zoomed ? t('split.unzoom') : t('split.zoom')"
        :title="zoomed ? t('split.unzoom') : t('split.zoom')"
        @click="emit('zoom')"
      >
        <Icon icon="maximize-20" />
      </button>
    </header>
    <SessionPane ref="pane" class="body" :session="session" :composer-shown="focused || zoomed" />
  </div>
</template>

<style scoped>
.tile {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  transition:
    left var(--geometry-duration) var(--motion-ease),
    top var(--geometry-duration) var(--motion-ease),
    width var(--geometry-duration) var(--motion-ease),
    height var(--geometry-duration) var(--motion-ease),
    box-shadow var(--motion-standard) var(--motion-ease),
    border-radius var(--geometry-duration) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    scale var(--motion-standard) var(--motion-ease);
}

.tile:not(.zoomed) {
  --page-padding: var(--tile-padding);
}

.tile.focused {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.tile.zoomed {
  border-radius: 0;
  box-shadow: none;
}

.tile.hidden {
  opacity: 0;
  scale: var(--motion-tile-exit-scale);
  pointer-events: none;
}

.header {
  display: flex;
  flex: none;
  align-items: center;
  gap: 10px;
  height: var(--tile-header-height);
  padding: 0 8px 0 12px;
  overflow: hidden;
  background: var(--surface);
  box-shadow: inset 0 -1px 0 var(--border);
  transition:
    height var(--motion-standard) var(--motion-ease),
    opacity var(--motion-standard) var(--motion-ease),
    background-color var(--motion-standard) var(--motion-ease);
}

.zoomed .header {
  height: 0;
  opacity: 0;
}

.header.permission {
  background: var(--warning-surface);
}

.header.error {
  background: var(--error-surface);
}

.title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: calc(var(--font-size) - 2px);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.progress,
.elapsed {
  flex: none;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.progress {
  display: flex;
  align-items: center;
  gap: 2px;
}

.progress.complete {
  color: var(--success);
}

.mark {
  display: flex;
  flex: none;
}

.lock {
  color: var(--warning);
}

.zoom {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.zoom:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.body {
  flex: 1;
  min-height: 0;
}
</style>
