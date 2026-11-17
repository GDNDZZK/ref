<script setup lang="ts">
// Switch to a live session or resume one from history (the sessions dialog).
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../../proto/generated'
import { relativeTimeText } from '../../shell/relativeTimeText.ts'
import {
  creatorKeywords,
  creatorLabel,
  dirName,
  historyLabel,
  isWorkingSession,
  liveLabel,
  relativeTime,
  rowState,
} from '../../shell/sessionRow.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useLayoutStore } from '../../stores/layout.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import Icon from '../Icon.vue'
import SessionDot from '../SessionDot.vue'
import PickDialog from './PickDialog.vue'

const { t, locale } = useI18n()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const notices = useNoticesStore()

interface SessionItem {
  key: string
  group: string
  info: SessionInfo
}

type CreatorFilter = 'all' | 'user' | 'creator'
const filter = ref<CreatorFilter>('all')
const targetCreatorId = ref<string>()

const sessionById = computed(() => {
  const map = new Map<string, SessionInfo>()
  for (const info of workspace.history) map.set(info.id, info)
  for (const info of workspace.live) map.set(info.id, info)
  return map
})

const allItems = computed<SessionItem[]>(() => {
  const liveIds = new Set(workspace.live.map((session) => session.id))
  const running = workspace.live.map((info) => ({ key: info.id, group: t('sidebar.running'), info }))
  const past = workspace.history
    .filter((info) => !liveIds.has(info.id))
    .map((info) => ({ key: info.id, group: t('sidebar.history'), info }))
  return [...running, ...past]
})

const items = computed<SessionItem[]>(() => {
  if (filter.value === 'user') return allItems.value.filter((item) => !item.info.parent)
  if (filter.value === 'creator' && targetCreatorId.value) {
    return allItems.value.filter((item) => item.info.parent === targetCreatorId.value)
  }
  return allItems.value
})

const isLive = (item: SessionItem) => item.info.live || workspace.live.some((s) => s.id === item.key)

const labelOf = (item: SessionItem) => (isLive(item) ? liveLabel(item.info) : historyLabel(item.info))

function searchText(item: SessionItem): string {
  const extra = creatorKeywords(sessionById.value, item.info)
  return `${labelOf(item)} ${item.info.cwd} ${item.info.id}${extra ? ` ${extra}` : ''}`
}
function selectFilter(next: CreatorFilter, selectedKey: string | undefined): void {
  if (next === 'creator') {
    const picked =
      allItems.value.find((item) => item.key === selectedKey) ??
      items.value.find((item) => item.key === selectedKey)
    if (!picked) return
    targetCreatorId.value = picked.info.parent ?? picked.info.id
  }
  filter.value = next
}

const creatorChipLabel = computed(() => {
  if (filter.value !== 'creator' || !targetCreatorId.value) return t('dialog.sessions.filterByCreator')
  return t('dialog.sessions.filterByName', { name: creatorLabel(sessionById.value, targetCreatorId.value) })
})

function subtitle(item: SessionItem): string {
  const base = isLive(item)
    ? dirName(item.info.cwd)
    : `${dirName(item.info.cwd)} · ${relativeTimeText(relativeTime(item.info.updated_at, Date.now()), t, locale.value)}`
  if (!item.info.parent) return base
  return `${base} · ${t('dialog.sessions.createdBy', { name: creatorLabel(sessionById.value, item.info.parent) })}`
}
function choose(item: SessionItem): void {
  dialogs.close()
  layout.closeDrawer()
  workspace.openSession(item.key).catch(notices.pushError)
}

function close(item: SessionItem): void {
  if (isWorkingSession(workspace.infoOf(item.key) ?? item.info)) {
    dialogs.open({ kind: 'closeSession', sessionId: item.key })
    return
  }
  workspace.closeSession(item.key).catch(notices.pushError)
}
</script>

<template>
  <PickDialog
    :title="t('dialog.sessions.title')"
    :search-label="t('dialog.sessions.search')"
    :items="items"
    :text="searchText"
    :checked="workspace.focusedId"
    :empty="t('sidebar.noMatch')"
    @pick="choose"
    @close="dialogs.close()"
  >
    <template #toolbar="{ selected }">
      <div class="filters" role="group" :aria-label="t('dialog.sessions.filterLabel')">
        <button
          type="button"
          class="filter-chip"
          :class="{ active: filter === 'all' }"
          :aria-pressed="filter === 'all'"
          @click="selectFilter('all', selected)"
        >
          {{ t('dialog.sessions.filterAll') }}
        </button>
        <button
          type="button"
          class="filter-chip"
          :class="{ active: filter === 'user' }"
          :aria-pressed="filter === 'user'"
          @click="selectFilter('user', selected)"
        >
          {{ t('dialog.sessions.filterUser') }}
        </button>
        <button
          type="button"
          class="filter-chip"
          :class="{ active: filter === 'creator' }"
          :aria-pressed="filter === 'creator'"
          :disabled="filter !== 'creator' && !selected"
          @click="selectFilter('creator', selected)"
        >
          {{ creatorChipLabel }}
        </button>
      </div>
    </template>
    <template #row="{ item }">
      <SessionDot v-if="isLive(item)" :state="rowState(item.info)" class="state" />
      <div class="text">
        <span class="name">{{ labelOf(item) }}</span>
        <span class="detail">{{ subtitle(item) }}</span>
      </div>
      <button
        v-if="isLive(item)"
        type="button"
        class="close"
        :aria-label="t('sidebar.close')"
        @click.stop="close(item)"
      >
        <Icon icon="close-16" />
      </button>
    </template>
  </PickDialog>
</template>

<style scoped src="./pickRow.css"></style>
<style scoped>
.filters {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 4px 0;
}

.filter-chip {
  height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.filter-chip:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text);
}

.filter-chip.active {
  background: var(--accent-surface);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 50%, transparent);
  color: var(--accent-text);
}

.filter-chip:disabled {
  opacity: 0.45;
  cursor: default;
}

.state {
  flex: none;
}
.close {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  margin-left: auto;
  border-radius: 50%;
  color: var(--text-muted);
  opacity: 0;
  transition: opacity var(--motion-quick) var(--motion-ease);
}

[role='option']:hover .close,
.close:focus-visible {
  opacity: 1;
}

.close:hover {
  background: var(--surface-hover);
  color: var(--text);
}
</style>
