<script setup lang="ts">
// Switch the agent of a session (the agents dialog).
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { AgentInfo } from '../../proto/generated'
import { agentColor } from '../../theme/agentColor.ts'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import Icon from '../Icon.vue'
import PickDialog from './PickDialog.vue'

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()

interface AgentItem {
  key: string
  agent: AgentInfo
  color: string | undefined
}

// The color comes from the agent's position in the whole list, like in the chat.
const items = computed<AgentItem[]>(() =>
  (connection.config?.agents ?? []).flatMap((agent, index) =>
    agent.creatable
      ? [{ key: agent.name, agent, color: agent.color ? agentColor(agent.color, index) : undefined }]
      : [],
  ),
)

function choose({ agent }: AgentItem): void {
  dialogs.close()
  workspace.setAgent(props.sessionId, agent.name).catch(notices.pushError)
}
</script>

<template>
  <PickDialog
    :title="t('dialog.agents.title')"
    :search-label="t('dialog.agents.search')"
    :items="items"
    :text="(item) => `${item.agent.name} ${item.agent.description}`"
    :checked="workspace.infoOf(sessionId)?.agent"
    @pick="choose"
    @close="dialogs.close()"
  >
    <template #row="{ item }">
      <Icon icon="agent-16" class="icon" :style="{ color: item.color }" />
      <div class="text">
        <span class="name">{{ item.agent.name }}</span>
        <span class="detail">{{ item.agent.description }}</span>
      </div>
    </template>
  </PickDialog>
</template>

<style scoped src="./pickRow.css"></style>
<style scoped>
.icon {
  flex: none;
  color: var(--text-muted);
}
</style>
