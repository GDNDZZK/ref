import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref, shallowRef } from 'vue'
import { info } from '../model/testMessages.ts'
import { useMcpStore } from './mcp.ts'
import { useWorkspaceStore } from './workspace.ts'

const mcpStatus = vi.fn()
const connectionState = ref<'connecting' | 'open' | 'closed'>('connecting')
const reconnected = ref(0)
const config = shallowRef<unknown>()

vi.mock('./connection.ts', () => ({
  useConnectionStore: () => ({
    get state() {
      return connectionState.value
    },
    get reconnected() {
      return reconnected.value
    },
    get config() {
      return config.value
    },
    getApi: () => ({ mcpStatus, listSessions: vi.fn(), listHistory: vi.fn() }),
  }),
}))

describe('useMcpStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    connectionState.value = 'connecting'
    reconnected.value = 0
    config.value = undefined
    mcpStatus.mockReset().mockResolvedValue([])
  })

  it('refreshes on connect, cwd change, reconnect, and config reload', async () => {
    const workspace = useWorkspaceStore()
    workspace.cwd = '/workspace/root'
    useMcpStore()
    expect(mcpStatus).not.toHaveBeenCalled()

    connectionState.value = 'open'
    await nextTick()
    expect(mcpStatus).toHaveBeenLastCalledWith('/workspace/root')

    workspace.onFrame({ kind: 'sessions', sessions: [info({ id: 's1', cwd: '/session/dir' })] })
    workspace.currentId = 's1'
    await nextTick()
    expect(mcpStatus).toHaveBeenLastCalledWith('/session/dir')

    reconnected.value++
    await nextTick()
    expect(mcpStatus).toHaveBeenCalledTimes(3)

    config.value = { agents: [], models: [], commands: [] }
    await nextTick()
    expect(mcpStatus).toHaveBeenCalledTimes(4)
  })
})
