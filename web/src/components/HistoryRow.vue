<script setup lang="ts">
// A session from history in the sidebar (HistoryRow.qml): title, directory and when it was last used.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../proto/generated'
import { relativeTimeText } from '../shell/relativeTimeText.ts'
import { dirName, historyLabel, relativeTime } from '../shell/sessionRow.ts'
import ContextMenu from './ContextMenu.vue'
import type { MenuItem } from './ContextMenu.vue'

const props = defineProps<{ info: SessionInfo }>()
const emit = defineEmits<{ open: []; remove: [] }>()

const { t, locale } = useI18n()

const subtitle = computed(() => {
  const when = relativeTimeText(relativeTime(props.info.updated_at, Date.now()), t, locale.value)
  return `${dirName(props.info.cwd)} · ${when}`
})

const menuCoords = ref({ x: 0, y: 0 })
const menuOpen = ref(false)
const menuMounted = ref(false)
const menuItems = computed<MenuItem[]>(() => [
  { label: t('sidebar.delete'), danger: true, run: () => emit('remove') },
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
    role="button"
    tabindex="0"
    @click="emit('open')"
    @keydown.enter.self="emit('open')"
    @contextmenu.prevent="openMenu"
  >
    <span class="title">{{ historyLabel(info) }}</span>
    <span class="subtitle">{{ subtitle }}</span>
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
  display: flex;
  flex-direction: column;
  justify-content: center;
  height: var(--sidebar-history-row-height);
  padding: 0 12px;
  border-radius: var(--radius);
  cursor: pointer;
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.history-row:hover {
  background: var(--surface-hover);
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
</style>
