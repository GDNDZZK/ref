<script setup lang="ts">
// The MCP servers of the open session's directory: status, errors and tools.
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { McpServerStatus } from '../../proto/generated'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useMcpStore } from '../../stores/mcp.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import Chevron from '../Chevron.vue'
import DialogButton from '../DialogButton.vue'
import ModalDialog from '../ModalDialog.vue'

const { t } = useI18n()
const dialogs = useDialogsStore()
const mcp = useMcpStore()
const notices = useNoticesStore()

const open = ref(new Set<string>())

onMounted(refresh)

function refresh(): void {
  mcp.refresh().catch(notices.pushError)
}

const expandable = (server: McpServerStatus) => server.error !== null || server.tools.length > 0

function toggle(server: McpServerStatus): void {
  if (!expandable(server)) return
  const next = new Set(open.value)
  if (!next.delete(server.name)) next.add(server.name)
  open.value = next
}

function statusText(status: string): string {
  const known = ['connected', 'connecting', 'failed', 'disabled']
  return known.includes(status) ? t(`dialog.mcp.${status}`) : status
}
</script>

<template>
  <ModalDialog :title="t('dialog.mcp.title')" @close="dialogs.close()">
    <p v-if="mcp.servers.length === 0" class="empty">{{ t('dialog.mcp.empty') }}</p>
    <div v-for="server in mcp.servers" :key="server.name" class="server">
      <button type="button" class="head" :disabled="!expandable(server)" @click="toggle(server)">
        <span class="dot" :class="server.status" />
        <span class="name">{{ server.name }}</span>
        <span class="status" :class="{ failed: server.status === 'failed' }">{{
          statusText(server.status)
        }}</span>
        <Chevron v-if="expandable(server)" class="chevron" :class="{ open: open.has(server.name) }" />
      </button>
      <div class="details" :class="{ open: open.has(server.name) }">
        <div class="inner">
          <p v-if="server.error" class="error">{{ server.error }}</p>
          <ul
            v-if="server.tools.length > 0"
            class="tools"
            :aria-label="t('dialog.mcp.tools', { count: server.tools.length })"
          >
            <li v-for="tool in server.tools" :key="tool" class="tool">{{ tool }}</li>
          </ul>
        </div>
      </div>
    </div>
    <template #footer>
      <DialogButton :label="t('dialog.mcp.refresh')" @click="refresh" />
    </template>
  </ModalDialog>
</template>

<style scoped>
.empty {
  margin: 0;
  padding: 16px 0;
  color: var(--text-muted);
  text-align: center;
}

.head {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 44px;
  text-align: left;
}

.head:disabled {
  cursor: default;
}

.dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-muted);
  opacity: 0.5;
}

.dot.connected {
  background: var(--success);
  opacity: 1;
}

.dot.connecting {
  background: var(--accent);
  opacity: 1;
}

.dot.failed {
  background: var(--danger);
  opacity: 1;
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.status {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.status.failed {
  color: var(--danger);
}

.chevron {
  color: var(--text-muted);
  transition: rotate var(--motion-standard) var(--motion-ease);
}

.chevron.open {
  rotate: 90deg;
}

.details {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--motion-standard) var(--motion-ease);
}

.details.open {
  grid-template-rows: 1fr;
}

.inner {
  min-height: 0;
  overflow: hidden;
}

.error {
  margin: 0 0 8px 18px;
  color: var(--danger);
  font: var(--code-font-size) / 1.45 var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.tools {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 0 0 10px 18px;
  padding: 0;
  list-style: none;
}

.tool {
  padding: 2px 6px;
  border-radius: var(--radius-small);
  background: var(--code-background);
  font: var(--small-font-size) var(--font-mono);
}
</style>
