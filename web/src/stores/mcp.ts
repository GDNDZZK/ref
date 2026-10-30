// Status of the MCP servers configured for the open session's directory (sidebar footer, MCP dialog).

import { defineStore } from 'pinia'
import { shallowRef, watch } from 'vue'
import type { McpServerStatus } from '../proto/generated'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'
import { useWorkspaceStore } from './workspace.ts'

const CONNECTING = 'connecting'
const RECHECK_MS = 2000

export const useMcpStore = defineStore('mcp', () => {
  const connection = useConnectionStore()
  const workspace = useWorkspaceStore()
  const notices = useNoticesStore()

  const servers = shallowRef<readonly McpServerStatus[]>([])
  let recheck: ReturnType<typeof setTimeout> | undefined

  async function refresh(): Promise<void> {
    clearTimeout(recheck)
    servers.value = await connection.getApi().mcpStatus(workspace.current?.info?.cwd)
    // Servers start in the background: look again until none is still connecting.
    if (servers.value.some((server) => server.status === CONNECTING)) {
      recheck = setTimeout(() => void refresh().catch(notices.pushError), RECHECK_MS)
    }
  }

  // The configuration is per directory.
  watch(
    () => workspace.current?.info?.cwd,
    (cwd) => {
      if (cwd !== undefined) void refresh().catch(notices.pushError)
    },
  )

  return { servers, refresh }
})
