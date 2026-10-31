<script setup lang="ts">
// Switch to a live session or resume one from history (the sessions dialog).
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../../proto/generated'
import { relativeTimeText } from '../../shell/relativeTimeText.ts'
import { dirName, historyLabel, liveLabel, relativeTime, rowState } from '../../shell/sessionRow.ts'
import { useCommandsStore } from '../../stores/commands.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useLayoutStore } from '../../stores/layout.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import Icon from '../Icon.vue'
import SessionDot from '../SessionDot.vue'
import PickDialog from './PickDialog.vue'

const { t, locale } = useI18n()
const workspace = useWorkspaceStore()
const commands = useCommandsStore()
const dialogs = useDialogsStore()
const layout = useLayoutStore()
const notices = useNoticesStore()

interface SessionItem {
  key: string
  group: string
  info: SessionInfo
}

const items = computed<SessionItem[]>(() => {
  const liveIds = new Set(workspace.live.map((session) => session.id))
  const running = workspace.live.map((info) => ({ key: info.id, group: t('sidebar.running'), info }))
  const past = workspace.history
    .filter((info) => !liveIds.has(info.id))
    .map((info) => ({ key: info.id, group: t('sidebar.history'), info }))
  return [...running, ...past]
})

const isLive = (item: SessionItem) => item.info.live || workspace.live.some((s) => s.id === item.key)

const labelOf = (item: SessionItem) => (isLive(item) ? liveLabel(item.info) : historyLabel(item.info))

function subtitle(item: SessionItem): string {
  if (isLive(item)) return dirName(item.info.cwd)
  const when = relativeTimeText(relativeTime(item.info.updated_at, Date.now()), t, locale.value)
  return `${dirName(item.info.cwd)} · ${when}`
}

function choose(item: SessionItem): void {
  dialogs.close()
  layout.closeDrawer()
  workspace.openSession(item.key).catch(notices.pushError)
}

function close(item: SessionItem): void {
  commands.run('close', '', item.key).catch(notices.pushError)
}
</script>

<template>
  <PickDialog
    :title="t('dialog.sessions.title')"
    :search-label="t('dialog.sessions.search')"
    :items="items"
    :text="(item) => `${labelOf(item)} ${item.info.cwd}`"
    :checked="workspace.focusedId"
    :empty="t('sidebar.noMatch')"
    @pick="choose"
    @close="dialogs.close()"
  >
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
