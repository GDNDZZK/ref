<script setup lang="ts">
// The /peers commands in one dialog: other sessions, this session's name, inbox and outbox.
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PeersTab } from '../../stores/dialogs.ts'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { describeError } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import ModalDialog from '../ModalDialog.vue'
import Segmented from '../Segmented.vue'
import TextField from '../TextField.vue'

const props = defineProps<{ sessionId: string; tab: PeersTab; name?: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const dialogs = useDialogsStore()
const workspace = useWorkspaceStore()

const tab = ref<PeersTab>(props.tab)
const text = ref<string>()
const failed = ref(false)
const newName = ref(workspace.infoOf(props.sessionId)?.name ?? '')
const nameError = ref('')

const tabs = computed(() => [
  { value: 'peers' as const, label: t('dialog.peers.peers') },
  { value: 'name' as const, label: t('dialog.peers.name') },
  { value: 'inbox' as const, label: t('dialog.peers.inbox') },
  { value: 'outbox' as const, label: t('dialog.peers.outbox') },
])

let latest = 0

// Runs the backend command of a tab; `args` is only used by the name tab.
async function load(args = ''): Promise<void> {
  const mine = ++latest
  text.value = undefined
  failed.value = false
  try {
    const result = await connection
      .getApi()
      .peerCommand(props.sessionId, `peers-${tab.value}`.replace('peers-peers', 'peers'), args)
    if (mine === latest) text.value = result
  } catch (error) {
    if (mine !== latest) return
    text.value = describeError(error)
    failed.value = true
  }
}

watch(tab, () => void load(), { immediate: false })
void load(tab.value === 'name' ? (props.name ?? '') : '')

async function setName(): Promise<void> {
  const next = newName.value.trim()
  if (next === '') return
  nameError.value = ''
  try {
    await connection.getApi().peerCommand(props.sessionId, 'peers-name', next)
    await load()
  } catch (error) {
    nameError.value = describeError(error)
  }
}
</script>

<template>
  <ModalDialog :title="t('dialog.peers.title')" width="wide" @close="dialogs.close()">
    <Segmented v-model="tab" :options="tabs" :label="t('dialog.peers.title')" class="tabs" />
    <Transition name="swap" mode="out-in">
      <pre :key="`${tab}:${text}`" class="output" :class="{ failed }">{{
        text ?? t('dialog.peers.loading')
      }}</pre>
    </Transition>
    <div v-if="tab === 'name'" class="name">
      <TextField v-model="newName" :label="t('dialog.peers.nameLabel')" :error="nameError" @enter="setName" />
      <DialogButton
        kind="primary"
        :label="t('dialog.peers.setName')"
        :disabled="newName.trim() === ''"
        @click="setName"
      />
    </div>
  </ModalDialog>
</template>

<style scoped>
.tabs {
  margin-bottom: 12px;
}

.output {
  box-sizing: border-box;
  min-height: 200px;
  margin: 0;
  padding: 14px;
  overflow: auto;
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
  font: var(--code-font-size) / 1.45 var(--font-mono);
  white-space: pre-wrap;
  user-select: text;
}

.output.failed {
  color: var(--danger);
}

.name {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  margin-top: 14px;
}

.name > :first-child {
  align-self: stretch;
}

.swap-enter-active,
.swap-leave-active {
  transition: opacity var(--motion-quick) var(--motion-ease);
}

.swap-enter-from,
.swap-leave-to {
  opacity: 0;
}
</style>
