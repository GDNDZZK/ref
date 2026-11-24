// The way to a signed-in connection through ref-web's channel, with a fake ref-web in place of
// the WebSocket: no key, a wrong key (4401, no retrying), the right key, and a reload.

import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  KIND_BINARY,
  KIND_TEXT,
  Opener,
  Sealer,
  authMac,
  channelKeys,
  fromHex,
  utf8,
} from '../net/channelCrypto.ts'
import { BACKEND_STORAGE_KEY } from '../net/backend.ts'
import { STORAGE_KEY, keys } from '../net/keystore.ts'
import { PROTOCOL_VERSION } from '../proto/generated'
import fixture from '../../../crates/web/fixtures/channel.json'
import { useConnectionStore } from './connection.ts'

const hex = (text: string, length: number): Uint8Array => fromHex(text, length)!
const ROOT = hex(fixture.root, 32)
const SN = hex(fixture.sn, 16)
const CWD = '/srv/work'
const DERIVE_TIMEOUT_MS = 15_000

// ref-web as the page sees it, keyed with the fixture's key and salt.
class FakeRefWeb {
  static opened: FakeRefWeb[] = []
  binaryType = 'blob'
  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: ((event: { code: number; reason: string }) => void) | null = null
  private sealer: Sealer | undefined
  private opener: Opener | undefined

  constructor() {
    FakeRefWeb.opened.push(this)
    setTimeout(() => {
      this.onopen?.()
      const hello = { t: 'hello', v: 1, salt: fixture.salt, sn: fixture.sn }
      this.onmessage?.({ data: JSON.stringify(hello) })
    })
  }

  send(data: string | Uint8Array): void {
    if (typeof data === 'string') return this.auth(data)
    const opened = this.opener!.open(data)
    if (opened.kind === KIND_BINARY) {
      this.sendSealed({ op: 'upstream_ready', cwd: CWD }, KIND_BINARY)
      return
    }
    const request = JSON.parse(new TextDecoder().decode(opened.plaintext)) as { id: number }
    const result = { server_version: '0.2.1', protocol: PROTOCOL_VERSION, cwd: CWD }
    this.sendSealed({ kind: 'response', id: request.id, ok: true, result })
  }

  close(): void {}

  private auth(text: string): void {
    const { cn, mac } = JSON.parse(text) as { cn: string; mac: string }
    const cnBytes = hex(cn, 16)
    const expected = Array.from(authMac(ROOT, SN, cnBytes), (b) => b.toString(16).padStart(2, '0')).join('')
    if (mac !== expected) {
      setTimeout(() => this.onclose?.({ code: 4401, reason: 'key rejected' }))
      return
    }
    const { toServer, toPage } = channelKeys(ROOT, SN, cnBytes)
    this.sealer = new Sealer(toPage)
    this.opener = new Opener(toServer)
    this.sendSealed({ version: '0.2.1', cwd: CWD, mock: false, channel_control: true })
  }

  private sendSealed(value: unknown, kind: typeof KIND_TEXT | typeof KIND_BINARY = KIND_TEXT): void {
    const wire = this.sealer!.seal(kind, utf8(JSON.stringify(value)))
    setTimeout(() =>
      this.onmessage?.({ data: wire.buffer.slice(wire.byteOffset, wire.byteOffset + wire.length) }),
    )
  }
}

async function until(condition: () => boolean): Promise<void> {
  await vi.waitFor(() => expect(condition()).toBe(true), { timeout: DERIVE_TIMEOUT_MS })
}

beforeEach(() => {
  setActivePinia(createPinia())
  FakeRefWeb.opened = []
  vi.stubGlobal('WebSocket', FakeRefWeb)
  localStorage.clear()
  keys.rejected()
  keys.required = false
})

afterEach(() => vi.unstubAllGlobals())

describe('connection store through ref-web', () => {
  it('without a key it asks for one and does not connect', () => {
    const connection = useConnectionStore()
    connection.start('ws://ref-web/ws')
    expect(connection.keyRequired).toBe(true)
    expect(FakeRefWeb.opened).toHaveLength(0)
  })

  it(
    'a wrong key is refused once, without retrying; the right one opens and is stored',
    async () => {
      const connection = useConnectionStore()
      connection.start('ws://ref-web/ws')

      expect(await connection.submitKey('a wrong key of some length')).toBe(false)
      expect(connection.keyRequired).toBe(true)
      expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
      const attempts = FakeRefWeb.opened.length
      await new Promise((resolve) => setTimeout(resolve, 1500)) // past the first retry delay
      expect(FakeRefWeb.opened.length).toBe(attempts)

      expect(await connection.submitKey(fixture.key)).toBe(true)
      expect(connection.keyRequired).toBe(false)
      await until(() => connection.state === 'open')
      expect(connection.hello?.cwd).toBe(CWD)
      expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual({
        salt: fixture.salt,
        root: fixture.root,
      })
    },
    DERIVE_TIMEOUT_MS * 2,
  )

  it('signs in from the stored root alone', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ salt: fixture.salt, root: fixture.root }))
    const connection = useConnectionStore()
    connection.start('ws://ref-web/ws')
    expect(connection.keyRequired).toBe(false)
    await until(() => connection.state === 'open')
  })

  it('hot-switches backend temporarily, saves default, and restores local default', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ salt: fixture.salt, root: fixture.root }))
    const connection = useConnectionStore()
    const beforeSwitch = vi.fn()
    connection.onBeforeSwitch(beforeSwitch)
    connection.start('ws://ref-web/ws')
    await until(() => connection.state === 'open')

    connection.switchBackend({ mode: 'temporary', url: 'ws://temp-host:9000/ws', token: 'secret-99' })
    expect(beforeSwitch).toHaveBeenCalledTimes(1)
    expect(connection.backendSource).toBe('temporary')
    expect(connection.maskedBackendToken).toBe('••••t-99')
    expect(localStorage.getItem(BACKEND_STORAGE_KEY)).toBeNull()
    await until(() => connection.state === 'open')

    connection.switchBackend({ mode: 'saveDefault', url: 'wss://saved-host:9443/ws', token: 'tok-saved' })
    expect(connection.backendSource).toBe('saved')
    expect(JSON.parse(localStorage.getItem(BACKEND_STORAGE_KEY)!)).toEqual({
      url: 'wss://saved-host:9443/ws',
      token: 'tok-saved',
    })
    await until(() => connection.state === 'open')

    connection.switchBackend({ mode: 'restoreDefault' })
    expect(connection.backendSource).toBe('auto')
    expect(localStorage.getItem(BACKEND_STORAGE_KEY)).toBeNull()
    await until(() => connection.state === 'open')
  })

  it('keeps activeBackendUrl reactive and enforces direct > temporary > saved > auto precedence', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ salt: fixture.salt, root: fixture.root }))
    localStorage.setItem(BACKEND_STORAGE_KEY, JSON.stringify({ url: 'ws://saved/ws', token: 'tok' }))
    const connection = useConnectionStore()
    expect(connection.backendSource).toBe('saved')
    expect(connection.activeBackendUrl).toBe('ws://saved/ws')

    connection.start('ws://ref-web/ws')
    expect(connection.backendSource).toBe('saved')
    expect(connection.activeBackendUrl).toBe('ws://saved/ws')
    await until(() => connection.state === 'open')

    connection.start('ws://direct-cli/ws', '/work', { direct: true, token: 'direct-tok' })
    expect(connection.backendSource).toBe('direct')
    expect(connection.activeBackendUrl).toBe('ws://direct-cli/ws')
  })

  it('syncs cross-tab storage updates when next is defined', async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ salt: fixture.salt, root: fixture.root }))
    const connection = useConnectionStore()
    const beforeSwitch = vi.fn()
    connection.onBeforeSwitch(beforeSwitch)
    connection.start('ws://ref-web/ws')
    await until(() => connection.state === 'open')
    expect(connection.backendSource).toBe('auto')

    const nextPayload = JSON.stringify({ url: 'ws://other-tab:9000/ws', token: 'tab-tok' })
    localStorage.setItem(BACKEND_STORAGE_KEY, nextPayload)
    window.dispatchEvent(new StorageEvent('storage', { key: BACKEND_STORAGE_KEY, newValue: nextPayload }))
    expect(beforeSwitch).toHaveBeenCalledTimes(1)
    expect(connection.backendSource).toBe('saved')
    expect(connection.activeBackendUrl).toBe('ws://other-tab:9000/ws')
    await until(() => connection.state === 'open')
  })
})
