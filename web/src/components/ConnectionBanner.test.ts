import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import { useConnectionStore } from '../stores/connection.ts'
import ConnectionBanner from './ConnectionBanner.vue'

class FakeSocket {
  static opened: FakeSocket[] = []
  static reason = ''
  binaryType = 'blob'
  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: ((event: { code: number; reason: string }) => void) | null = null

  constructor() {
    FakeSocket.opened.push(this)
    setTimeout(() => this.onopen?.())
  }

  send(data: string): void {
    const frame = JSON.parse(data) as { id: number; op: string }
    if (frame.op !== 'hello') return
    setTimeout(() => {
      this.onmessage?.({
        data: JSON.stringify({
          kind: 'response',
          id: frame.id,
          ok: false,
          result: null,
          error: FakeSocket.reason,
          code: 'protocol_mismatch',
        }),
      })
    })
  }

  close(): void {
    this.onclose?.({ code: 1000, reason: 'closed' })
  }
}

describe('ConnectionBanner', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    FakeSocket.opened = []
    FakeSocket.reason = ''
    vi.stubEnv('VITE_REF_URL', 'ws://localhost:1234/ws')
    vi.stubGlobal('WebSocket', FakeSocket)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('displays the refusal reason without reconnecting when the handshake fails with protocol_mismatch', async () => {
    const reason =
      'ref-server 0.2.1 speaks protocol 4, but this client (ref-web) speaks protocol 3. Update the client to the same version as ref-server.'
    FakeSocket.reason = reason
    const connection = useConnectionStore()
    const wrapper = mount(ConnectionBanner, {
      global: { plugins: [createAppI18n(['en'])] },
    })

    connection.start('ws://localhost:1234/ws', '/work')
    await vi.waitFor(() => expect(connection.state).toBe('closed'))
    await new Promise((resolve) => setTimeout(resolve, 600))

    expect(FakeSocket.opened).toHaveLength(1)
    expect(wrapper.get('[role="alert"]').text()).toContain(reason)
  })
})
