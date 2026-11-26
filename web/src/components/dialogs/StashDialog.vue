<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionInfo, StashGroup, StashMember } from '../../proto/generated'
import { dirName, historyLabel, liveLabel } from '../../shell/sessionRow.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import { assignRootColors, paletteColorFor } from '../../theme/agentColor.ts'
import ContextMenu from '../ContextMenu.vue'
import type { MenuItem } from '../ContextMenu.vue'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import ModalDialog from '../ModalDialog.vue'

type DragPayload =
  | { kind: 'running'; sessionId: string }
  | { kind: 'member'; groupId: string; sessionId: string }
  | { kind: 'group'; groupId: string }

const { t } = useI18n()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()

const dragState = ref<DragPayload>()
const dropGroupId = ref<string>()
const dropNewGroup = ref(false)
const dropRestore = ref(false)
const dropOutside = ref(false)
const dropMemberKey = ref<string>() // `${groupId}:${sessionId}`

const editingGroupId = ref<string>()
const editingTitle = ref('')
const menuOpen = ref(false)
const menuMounted = ref(false)
const menuCoords = ref({ x: 0, y: 0 })
const menuItems = ref<MenuItem[]>([])

const sortedGroups = computed(() =>
  workspace.groups.map((group) => ({
    ...group,
    members: [...group.members].sort((a, b) => a.position - b.position),
  })),
)

function report(promise: Promise<unknown>): void {
  promise.catch(notices.pushError)
}

const creatorColors = computed(() => {
  const keys: string[] = []
  for (const session of workspace.live) {
    keys.push(session.parent ?? session.id)
  }
  for (const group of workspace.groups) {
    for (const member of group.members) {
      keys.push(member.info.parent ?? member.info.id)
    }
  }
  return assignRootColors(keys)
})

function creatorTint(info: SessionInfo): string {
  const key = info.parent ?? info.id
  return creatorColors.value.get(key) ?? paletteColorFor(key)
}

function memberTitle(member: StashMember): string {
  return member.info.live ? liveLabel(member.info) : historyLabel(member.info)
}

function clearDragState(): void {
  dragState.value = undefined
  dropGroupId.value = undefined
  dropNewGroup.value = false
  dropRestore.value = false
  dropOutside.value = false
  dropMemberKey.value = undefined
}

function startRunningDrag(event: DragEvent, sessionId: string): void {
  dragState.value = { kind: 'running', sessionId }
  event.dataTransfer?.setData('text/plain', sessionId)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

function startMemberDrag(event: DragEvent, group: StashGroup, sessionId: string): void {
  if (group.locked) {
    event.preventDefault()
    return
  }
  dragState.value = { kind: 'member', groupId: group.id, sessionId }
  event.dataTransfer?.setData('text/plain', sessionId)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

function startGroupDrag(event: DragEvent, group: StashGroup): void {
  if (group.locked) {
    event.preventDefault()
    return
  }
  dragState.value = { kind: 'group', groupId: group.id }
  event.dataTransfer?.setData('text/plain', group.id)
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

function onDragOverNewGroup(event: DragEvent): void {
  if (!dragState.value || dragState.value.kind === 'group') return
  event.preventDefault()
  event.stopPropagation()
  dropNewGroup.value = true
  dropGroupId.value = undefined
  dropMemberKey.value = undefined
  dropRestore.value = false
  dropOutside.value = false
}

function onDropNewGroup(event: DragEvent): void {
  event.preventDefault()
  event.stopPropagation()
  const payload = dragState.value
  clearDragState()
  if (!payload || payload.kind === 'group') return
  if (payload.kind === 'running') {
    report(workspace.createGroup([payload.sessionId], { stashLive: true }))
    return
  }
  report(
    workspace
      .createGroup([payload.sessionId], { stashLive: false })
      .then(() => workspace.removeFromGroup(payload.groupId, payload.sessionId)),
  )
}

function onDragOverGroup(event: DragEvent, group: StashGroup): void {
  const payload = dragState.value
  if (!payload) return
  if (group.locked) return
  if (payload.kind === 'group' && payload.groupId === group.id) return
  event.preventDefault()
  event.stopPropagation()
  dropGroupId.value = group.id
  dropNewGroup.value = false
  dropRestore.value = false
  dropOutside.value = false
  dropMemberKey.value = undefined
}

function onDragLeaveGroup(groupId: string): void {
  if (dropGroupId.value === groupId) dropGroupId.value = undefined
}

function onDropGroup(event: DragEvent, targetGroup: StashGroup): void {
  event.preventDefault()
  event.stopPropagation()
  const payload = dragState.value
  clearDragState()
  if (!payload || targetGroup.locked) return
  if (payload.kind === 'running') {
    report(workspace.addToGroup(targetGroup.id, payload.sessionId, { stashLive: true }))
    return
  }
  if (payload.kind === 'member') {
    const position = targetGroup.members.length
    report(
      workspace.moveGroupMember(targetGroup.id, payload.sessionId, position, {
        fromGroupId: payload.groupId,
        stashLive: false,
      }),
    )
    return
  }
  if (payload.kind === 'group' && payload.groupId !== targetGroup.id) {
    report(workspace.mergeGroups(payload.groupId, targetGroup.id))
  }
}

function onDragOverMember(event: DragEvent, group: StashGroup, member: StashMember): void {
  const payload = dragState.value
  if (!payload || payload.kind === 'group' || group.locked) return
  event.preventDefault()
  event.stopPropagation()
  dropMemberKey.value = `${group.id}:${member.session_id}`
  dropGroupId.value = group.id
  dropNewGroup.value = false
  dropRestore.value = false
  dropOutside.value = false
}

function onDropMember(event: DragEvent, targetGroup: StashGroup, targetIndex: number): void {
  event.preventDefault()
  event.stopPropagation()
  const payload = dragState.value
  clearDragState()
  if (!payload || payload.kind === 'group' || targetGroup.locked) return
  if (payload.kind === 'running') {
    report(
      workspace.addToGroup(targetGroup.id, payload.sessionId, {
        position: targetIndex,
        stashLive: true,
      }),
    )
    return
  }
  const sourceIndex =
    payload.groupId === targetGroup.id
      ? targetGroup.members.findIndex((member) => member.session_id === payload.sessionId)
      : -1
  const position =
    payload.groupId === targetGroup.id && sourceIndex >= 0 && sourceIndex < targetIndex
      ? targetIndex - 1
      : targetIndex
  report(
    workspace.moveGroupMember(targetGroup.id, payload.sessionId, position, {
      fromGroupId: payload.groupId,
      stashLive: false,
    }),
  )
}

function onDragOverRunning(event: DragEvent): void {
  if (dragState.value?.kind !== 'member') return
  event.preventDefault()
  event.stopPropagation()
  dropRestore.value = true
  dropOutside.value = false
  dropGroupId.value = undefined
  dropMemberKey.value = undefined
  dropNewGroup.value = false
}

function onDragLeaveRunning(event: DragEvent): void {
  const currentTarget = event.currentTarget as HTMLElement | null
  const relatedTarget = event.relatedTarget as Node | null
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return
  dropRestore.value = false
}

function onDropRunning(event: DragEvent): void {
  event.preventDefault()
  event.stopPropagation()
  const payload = dragState.value
  clearDragState()
  if (payload?.kind !== 'member') return
  report(workspace.restoreGroupMember(payload.groupId, payload.sessionId))
}
function onDragOverGroupsColumn(): void {
  dropOutside.value = false
  dropRestore.value = false
}

function onDragOverOutside(event: DragEvent): void {
  if (dragState.value?.kind !== 'member') return
  event.preventDefault()
  dropOutside.value = true
  dropRestore.value = false
  dropGroupId.value = undefined
  dropMemberKey.value = undefined
  dropNewGroup.value = false
}

function onDragLeaveOutside(event: DragEvent): void {
  const currentTarget = event.currentTarget as HTMLElement | null
  const relatedTarget = event.relatedTarget as Node | null
  if (currentTarget && relatedTarget && currentTarget.contains(relatedTarget)) return
  dropOutside.value = false
}

function onDropOutside(event: DragEvent): void {
  event.preventDefault()
  const payload = dragState.value
  clearDragState()
  if (payload?.kind !== 'member') return
  report(workspace.removeFromGroup(payload.groupId, payload.sessionId))
}

function isInsideColumns(target: EventTarget | null): boolean {
  let element: Element | null = null
  if (target instanceof Element) element = target
  else if (target instanceof Node) element = target.parentElement
  return element?.closest('.running-column, .groups-column') != null
}

function onWindowDragOver(event: DragEvent): void {
  if (dragState.value?.kind !== 'member') return
  if (event.defaultPrevented || isInsideColumns(event.target)) return
  onDragOverOutside(event)
}

function onWindowDrop(event: DragEvent): void {
  if (dragState.value?.kind !== 'member') return
  if (event.defaultPrevented || isInsideColumns(event.target)) return
  onDropOutside(event)
}

onMounted(() => {
  window.addEventListener('dragover', onWindowDragOver)
  window.addEventListener('drop', onWindowDrop)
})

onBeforeUnmount(() => {
  window.removeEventListener('dragover', onWindowDragOver)
  window.removeEventListener('drop', onWindowDrop)
})

function moveMemberBy(group: StashGroup, index: number, delta: -1 | 1): void {
  if (group.locked) return
  const targetIndex = index + delta
  if (targetIndex < 0 || targetIndex >= group.members.length) return
  const member = group.members[index]
  if (!member) return
  report(
    workspace.moveGroupMember(group.id, member.session_id, targetIndex, {
      fromGroupId: group.id,
      stashLive: false,
    }),
  )
}

async function beginRename(group: StashGroup): Promise<void> {
  editingGroupId.value = group.id
  editingTitle.value = group.name
  await nextTick()
  const input = document.querySelector<HTMLInputElement>('.group-title-input')
  input?.focus()
  input?.select()
}

function commitRename(group: StashGroup): void {
  if (editingGroupId.value !== group.id) return
  editingGroupId.value = undefined
  const trimmed = editingTitle.value.trim()
  if (trimmed === '' || trimmed === group.name) return
  report(workspace.renameGroup(group.id, trimmed))
}

function cancelRename(): void {
  editingGroupId.value = undefined
}

function openAssignMenu(event: MouseEvent, sessionId: string, sourceGroup?: StashGroup): void {
  const button = event.currentTarget as HTMLElement | null
  const rect = button?.getBoundingClientRect()
  menuCoords.value = { x: rect ? rect.left : event.clientX, y: rect ? rect.bottom + 4 : event.clientY }
  const items: MenuItem[] = []
  for (const group of workspace.groups) {
    if (group.locked || group.id === sourceGroup?.id) continue
    items.push({
      label: sourceGroup
        ? t('dialog.stash.moveToGroup', { name: group.name })
        : t('dialog.stash.addToGroup', { name: group.name }),
      run: () => {
        if (sourceGroup) {
          report(
            workspace.moveGroupMember(group.id, sessionId, group.members.length, {
              fromGroupId: sourceGroup.id,
              stashLive: false,
            }),
          )
        } else {
          report(workspace.addToGroup(group.id, sessionId, { stashLive: true }))
        }
      },
    })
    if (sourceGroup) {
      items.push({
        label: t('dialog.stash.copyToGroup', { name: group.name }),
        run: () => report(workspace.addToGroup(group.id, sessionId, { stashLive: false })),
      })
    }
  }
  items.push({
    label: t('dialog.stash.newGroupWithSession'),
    separatorBefore: items.length > 0,
    run: () => {
      report(workspace.createGroup([sessionId], { stashLive: !sourceGroup }))
    },
  })
  menuItems.value = items
  menuMounted.value = true
  menuOpen.value = true
}

function createEmptyGroup(): void {
  report(workspace.createGroup([], { stashLive: false }))
}
</script>

<template>
  <ModalDialog
    class="stash-modal"
    :class="{ 'drop-remove': dropOutside }"
    :title="t('dialog.stash.title')"
    width="wide"
    @close="dialogs.close()"
    @dragover="onDragOverOutside"
    @dragleave="onDragLeaveOutside"
    @drop="onDropOutside"
  >
    <div class="columns">
      <section
        class="column running-column"
        :class="{ 'drop-restore': dropRestore }"
        @dragover="onDragOverRunning"
        @dragleave="onDragLeaveRunning"
        @drop="onDropRunning"
      >
        <header class="column-head">
          <h3 class="column-title">{{ t('dialog.stash.runningColumn') }}</h3>
          <DialogButton
            v-if="workspace.live.length > 0"
            small
            :label="t('dialog.stash.stashAll')"
            @click="report(workspace.stashAll())"
          />
        </header>
        <div v-if="workspace.live.length === 0" class="column-empty">
          {{ t('dialog.sessions.emptyLive') }}
        </div>
        <TransitionGroup v-else tag="div" name="stash-list" class="list">
          <div
            v-for="session in workspace.live"
            :key="session.id"
            class="running-card"
            draggable="true"
            @dragstart="startRunningDrag($event, session.id)"
            @dragend="clearDragState"
          >
            <span class="tint-bar" :style="{ background: creatorTint(session) }" />
            <span class="tint-dot" :style="{ background: creatorTint(session) }" />
            <div class="card-text">
              <span class="card-title">{{ liveLabel(session) }}</span>
              <span class="card-sub">{{ dirName(session.cwd) }}</span>
            </div>
            <button
              type="button"
              class="card-action"
              :aria-label="t('dialog.stash.assignGroup')"
              :title="t('dialog.stash.assignGroup')"
              @click="openAssignMenu($event, session.id)"
            >
              <Icon icon="plus-16" />
            </button>
          </div>
        </TransitionGroup>
      </section>

      <section
        class="column groups-column"
        @dragover.stop="onDragOverGroupsColumn"
        @drop.stop="clearDragState"
      >
        <header class="column-head">
          <h3 class="column-title">{{ t('dialog.stash.groupsColumn') }}</h3>
          <button
            type="button"
            class="new-group-zone"
            :class="{ 'drop-active': dropNewGroup }"
            @click="createEmptyGroup"
            @dragover="onDragOverNewGroup"
            @dragleave="dropNewGroup = false"
            @drop="onDropNewGroup"
          >
            <Icon icon="plus-16" />
            <span>{{ t('dialog.stash.newGroup') }}</span>
          </button>
        </header>

        <div v-if="sortedGroups.length === 0" class="column-empty">
          {{ t('dialog.stash.emptyGroups') }}
        </div>
        <TransitionGroup v-else tag="div" name="stash-list" class="groups-list">
          <article
            v-for="group in sortedGroups"
            :key="group.id"
            class="group-card"
            :class="{ locked: group.locked, 'drop-target': dropGroupId === group.id }"
            @dragover="onDragOverGroup($event, group)"
            @dragleave="onDragLeaveGroup(group.id)"
            @drop="onDropGroup($event, group)"
          >
            <header
              class="group-header"
              :draggable="!group.locked && editingGroupId !== group.id"
              @dragstart="startGroupDrag($event, group)"
              @dragend="clearDragState"
            >
              <input
                v-if="editingGroupId === group.id"
                v-model="editingTitle"
                type="text"
                class="group-title-input"
                @keydown.enter.prevent="commitRename(group)"
                @keydown.esc.prevent.stop="cancelRename"
                @blur="commitRename(group)"
              />
              <button
                v-else
                type="button"
                class="group-title"
                :title="t('dialog.stash.renameGroup')"
                @click="beginRename(group)"
              >
                {{ group.name }}
              </button>
              <div class="group-actions">
                <button
                  type="button"
                  class="icon-btn"
                  :class="{ active: group.locked }"
                  :aria-label="group.locked ? t('dialog.stash.unlock') : t('dialog.stash.lock')"
                  :aria-pressed="group.locked"
                  :title="group.locked ? t('dialog.stash.unlock') : t('dialog.stash.lock')"
                  @click="report(workspace.setGroupLocked(group.id, !group.locked))"
                >
                  <Icon icon="lock-16" />
                </button>
                <DialogButton
                  small
                  :label="t('dialog.stash.restoreAll')"
                  @click="report(workspace.restoreGroup(group.id))"
                />
                <button
                  type="button"
                  class="icon-btn danger"
                  :disabled="group.locked"
                  :aria-label="t('dialog.stash.deleteGroup')"
                  :title="t('dialog.stash.deleteGroup')"
                  @click="report(workspace.deleteGroup(group.id))"
                >
                  <Icon icon="trash-16" />
                </button>
              </div>
            </header>

            <TransitionGroup tag="div" name="stash-list" class="members">
              <div
                v-for="(member, index) in group.members"
                :key="member.session_id"
                class="member-row"
                :class="{
                  live: member.info.live,
                  'drop-before': dropMemberKey === `${group.id}:${member.session_id}`,
                }"
                :draggable="!group.locked"
                @dragstart="startMemberDrag($event, group, member.session_id)"
                @dragend="clearDragState"
                @dragover="onDragOverMember($event, group, member)"
                @drop="onDropMember($event, group, index)"
                @keydown.alt.up.prevent="moveMemberBy(group, index, -1)"
                @keydown.alt.down.prevent="moveMemberBy(group, index, 1)"
              >
                <span class="tint-bar" :style="{ background: creatorTint(member.info) }" />
                <span class="tint-dot" :style="{ background: creatorTint(member.info) }" />
                <div class="card-text">
                  <span class="card-title">{{ memberTitle(member) }}</span>
                  <span class="card-sub">{{ dirName(member.info.cwd) }}</span>
                </div>
                <div class="member-actions">
                  <button
                    type="button"
                    class="icon-btn"
                    :disabled="group.locked || index === 0"
                    :aria-label="t('config.moveUp')"
                    :title="t('config.moveUp')"
                    @click="moveMemberBy(group, index, -1)"
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    class="icon-btn"
                    :disabled="group.locked || index === group.members.length - 1"
                    :aria-label="t('config.moveDown')"
                    :title="t('config.moveDown')"
                    @click="moveMemberBy(group, index, 1)"
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    class="icon-btn"
                    :aria-label="t('dialog.stash.assignGroup')"
                    :title="t('dialog.stash.assignGroup')"
                    @click="openAssignMenu($event, member.session_id, group)"
                  >
                    <Icon icon="plus-16" />
                  </button>
                  <button
                    type="button"
                    class="icon-btn"
                    :aria-label="t('dialog.stash.restoreMember')"
                    :title="t('dialog.stash.restoreMember')"
                    @click="report(workspace.restoreGroupMember(group.id, member.session_id))"
                  >
                    <Icon icon="undo-16" />
                  </button>
                  <button
                    type="button"
                    class="icon-btn danger"
                    :disabled="group.locked"
                    :aria-label="t('dialog.stash.removeMember')"
                    :title="t('dialog.stash.removeMember')"
                    @click="report(workspace.removeFromGroup(group.id, member.session_id))"
                  >
                    <Icon icon="close-16" />
                  </button>
                </div>
              </div>
            </TransitionGroup>
          </article>
        </TransitionGroup>
      </section>
    </div>

    <ContextMenu
      v-if="menuMounted"
      :open="menuOpen"
      :x="menuCoords.x"
      :y="menuCoords.y"
      :items="menuItems"
      @close="menuOpen = false"
    />
  </ModalDialog>
</template>

<style scoped>
.stash-modal :deep(.dialog.wide) {
  width: min(840px, calc(100vw - 24px));
}

.columns {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.35fr);
  gap: 16px;
  min-height: 300px;
}

@media (max-width: 720px) {
  .columns {
    grid-template-columns: 1fr;
  }
}

.column {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
  padding: 10px;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: box-shadow var(--motion-quick) var(--motion-ease);
}

.running-column.drop-restore {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.stash-modal {
  transition: box-shadow var(--motion-quick) var(--motion-ease);
}

.stash-modal.drop-remove {
  box-shadow:
    inset 0 0 0 1px var(--danger),
    0 12px 40px #00000040;
}

.column-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 28px;
}

.column-title {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.new-group-zone {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 10px;
  border-radius: 14px;
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text-muted);
  font-size: var(--small-font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    box-shadow var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.new-group-zone:hover,
.new-group-zone.drop-active {
  background: var(--surface-hover);
  box-shadow: inset 0 0 0 1px var(--accent-text);
  color: var(--text);
}

.column-empty {
  padding: 24px 12px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  text-align: center;
}

.list,
.groups-list,
.members {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.running-card,
.member-row {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px 8px 12px;
  border-radius: var(--radius-small);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  cursor: grab;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    box-shadow var(--motion-quick) var(--motion-ease),
    transform var(--motion-standard) var(--motion-ease);
}

.member-row.live {
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
}

.member-row.drop-before {
  box-shadow:
    inset 0 2px 0 var(--accent-text),
    inset 0 0 0 1px var(--accent-text);
}

.tint-bar {
  position: absolute;
  left: 0;
  top: 6px;
  bottom: 6px;
  width: 3px;
  border-radius: 2px;
}

.tint-dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
}

.card-text {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
}

.card-title {
  overflow: hidden;
  font-size: var(--small-font-size);
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card-sub {
  overflow: hidden;
  color: var(--text-muted);
  font-size: var(--micro-font-size);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card-action,
.icon-btn {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.card-action:hover,
.icon-btn:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--text);
}

.icon-btn.active {
  color: var(--accent-text);
}

.icon-btn.danger:not(:disabled):hover {
  color: var(--danger-text);
}

.icon-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.group-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border-radius: var(--radius);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
  transition:
    box-shadow var(--motion-quick) var(--motion-ease),
    transform var(--motion-standard) var(--motion-ease);
}

.group-card.drop-target {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.group-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-align: left;
  font-size: calc(var(--font-size) - 1px);
  font-weight: 700;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.group-title-input {
  flex: 1;
  min-width: 0;
  height: 26px;
  padding: 0 8px;
  border: 0;
  outline: 0;
  border-radius: var(--radius-small);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--accent-text);
  color: var(--text);
  font: inherit;
}

.group-actions,
.member-actions {
  display: flex;
  flex: none;
  align-items: center;
  gap: 4px;
}

.stash-list-move,
.stash-list-enter-active,
.stash-list-leave-active {
  transition:
    opacity var(--motion-standard) var(--motion-ease),
    transform var(--motion-standard) var(--motion-ease);
}

.stash-list-enter-from,
.stash-list-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
