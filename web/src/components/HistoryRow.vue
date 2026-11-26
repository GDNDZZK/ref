<script setup lang="ts">
// A session from history in the sidebar (HistoryRow.qml): title, directory and when it was last used.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../proto/generated'
import { relativeTimeText } from '../shell/relativeTimeText.ts'
import { dirName, historyLabel, relativeTime } from '../shell/sessionRow.ts'
import ContextMenu from './ContextMenu.vue'
import type { MenuItem } from './ContextMenu.vue'
import Icon from './Icon.vue'

const props = defineProps<{ info: SessionInfo; creatorName?: string }>()
const emit = defineEmits<{
  open: []
  remove: []
  filterByCreator: [creatorId: string]
  pin: [pinned: boolean]
}>()

const { t, locale } = useI18n()
const pinned = computed(() => props.info.pinned_at != null)

const subtitle = computed(() => {
  const when = relativeTimeText(relativeTime(props.info.updated_at, Date.now()), t, locale.value)
  const base = `${dirName(props.info.cwd)} · ${when}`
  if (!props.info.parent) return base
  return `${base} · ⟲ ${props.creatorName ?? props.info.parent}`
})

const menuCoords = ref({ x: 0, y: 0 })
const menuOpen = ref(false)
const menuMounted = ref(false)
const menuItems = computed<MenuItem[]>(() => [
  { label: pinned.value ? t('sidebar.unpin') : t('sidebar.pin'), run: () => emit('pin', !pinned.value) },
  ...(props.info.parent
    ? [{ label: t('sidebar.filterByCreator'), run: () => emit('filterByCreator', props.info.parent!) }]
    : []),
  { label: t('sidebar.filterSpawnedHistory'), run: () => emit('filterByCreator', props.info.id) },
  { label: t('sidebar.delete'), danger: true, separatorBefore: true, run: () => emit('remove') },
])

function openMenu(event: MouseEvent): void {
  menuCoords.value = { x: event.clientX, y: event.clientY }
  menuMounted.value = true
  menuOpen.value = true
}
</script>

<template>
  <div
    class="history-row"
    :class="{ pinned }"
    role="button"
    tabindex="0"
    @click="emit('open')"
    @keydown.enter.self="emit('open')"
    @contextmenu.prevent="openMenu"
  >
    <div class="main">
      <span class="title">{{ historyLabel(info) }}</span>
      <span class="subtitle">{{ subtitle }}</span>
    </div>
    <button
      type="button"
      class="pin"
      :class="{ pinned }"
      :title="pinned ? t('sidebar.unpin') : t('sidebar.pin')"
      :aria-label="pinned ? t('sidebar.unpin') : t('sidebar.pin')"
      @click.stop="emit('pin', !pinned)"
    >
      <Icon icon="pin-16" />
    </button>
    <ContextMenu
      v-if="menuMounted"
      :open="menuOpen"
      :x="menuCoords.x"
      :y="menuCoords.y"
      :items="menuItems"
      @close="menuOpen = false"
    />
  </div>
</template>

<style scoped>
.history-row {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  height: var(--sidebar-history-row-height);
  padding: 0 12px;
  border-radius: var(--radius);
  cursor: pointer;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.history-row:hover {
  background: var(--surface-hover);
}

.main {
  display: flex;
  flex: 1;
  flex-direction: column;
  justify-content: center;
  min-width: 0;
}

.title,
.subtitle {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.title {
  font-size: calc(var(--font-size) - 2px);
}

.subtitle {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.pin {
  position: absolute;
  right: 12px;
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  opacity: 0;
  pointer-events: none;
  transition:
    opacity var(--motion-quick) var(--motion-ease),
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.pin:hover {
  background: var(--surface);
  color: var(--text);
}

.pin.pinned {
  position: static;
  color: var(--accent-text);
  opacity: 1;
  pointer-events: auto;
}

.pin.pinned:hover {
  color: var(--accent-text);
}

.history-row:hover .pin,
.history-row:focus-within .pin {
  opacity: 1;
  pointer-events: auto;
}
</style>
