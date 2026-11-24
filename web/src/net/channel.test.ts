// @vitest-environment node
/// <reference types="node" />
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'
import { channelTransport } from './channel.ts'
import type { KeySource } from './channel.ts'
import { KIND_BINARY, KIND_TEXT, Opener, Sealer, channelKeys, fromHex, toHex, utf8 } from './channelCrypto.ts'

// The vectors shared with crates/web: the page's nonce is fixed, so its auth message is too.
const FIXTURE = fileURLToPath(new URL('../../../crates/web/fixtures/channel.json', import.meta.url))
const fixture = JSON.parse(readFileSync(FIXTURE, 'utf8')) as {
  salt: string
  root: string
  sn: string
  cn: string
  mac: string
  c2s: { plain: string; wire: string }[]
  s2c: { plain: string; wire: string }[]
}

const hex = (text: string, length: number): Uint8Array => fromHex(text, length)!
const CLOSE_KEY_REJECTED = 4401

function helloText(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({ t: 'hello', v: 1, salt: fixture.salt, sn: fixture.sn, ...overrides })
}

const frame = (index: number): Uint8Array =>
  hex(fixture.s2c[index]!.wire, fixture.s2c[index]!.wire.length / 2)

const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

function setup(
  rootFor: KeySource['rootFor'] = (salt) =>
    Promise.resolve(salt === fixture.salt ? hex(fixture.root, 32) : undefined),
) {
  const keys = { rootFor: vi.fn(rootFor), rejected: vi.fn() }
  const sent: (string | Uint8Array)[] = []
  let socketHandlers!: SocketHandlers
  const socketClose = vi.fn()
  const handlers = { opened: vi.fn(), message: vi.fn(), closed: vi.fn() } satisfies TransportHandlers
  const factory = channelTransport(
    (given) => {
      socketHandlers = given
      return { send: (data) => void sent.push(data), close: socketClose }
    },
    keys,
    () => hex(fixture.cn, 16),
  )
  const transport = factory(handlers)
  const server = {
    open: () => socketHandlers.opened(),
    send: async (data: string | Uint8Array) => {
      socketHandlers.message(data)
      await flush()
    },
    close: (code: number, reason = '') => socketHandlers.closed(code, reason),
  }
  return { keys, sent, socketClose, handlers, transport, server }
}

// Through the handshake, up to an open transport.
async function opened(...args: Parameters<typeof setup>) {
  const t = setup(...args)
  t.server.open()
  await t.server.send(helloText())
  await t.server.send(frame(0))
  return t
}

describe('channelTransport', () => {
  it('answers the hello with the shared auth message, then opens on frame 0', async () => {
    const t = setup()
    t.server.open()
    expect(t.handlers.opened).not.toHaveBeenCalled()
    await t.server.send(helloText())
    expect(t.keys.rootFor).toHaveBeenCalledWith(fixture.salt)
    expect(t.sent).toEqual([JSON.stringify({ t: 'auth', cn: fixture.cn, mac: fixture.mac })])
    expect(t.handlers.opened).not.toHaveBeenCalled()

    await t.server.send(frame(0))
    expect(t.handlers.opened).toHaveBeenCalledOnce()
    expect(t.transport.info).toEqual({ version: '0.2.1', cwd: '/work', mock: false })
  })

  it('hands sealed text frames up and seals what it sends, counting from 0', async () => {
    const t = await opened()
    await t.server.send(frame(1))
    expect(t.handlers.message).toHaveBeenCalledWith(fixture.s2c[1]!.plain)

    t.transport.send(fixture.c2s[0]!.plain)
    expect(toHex(t.sent[1] as Uint8Array)).toBe(fixture.c2s[0]!.wire)
  })

  it('stops on a binary protocol frame', async () => {
    const t = await opened()
    await t.server.send(frame(1))
    await t.server.send(frame(2)) // sealed as binary
    expect(t.handlers.closed).toHaveBeenCalledWith('the server sent a binary frame')
    expect(t.socketClose).toHaveBeenCalled()
  })

  it('does not send before it is open', async () => {
    const t = setup()
    expect(() => t.transport.send('x')).toThrow('not open')
    t.server.open()
    await t.server.send(helloText())
    expect(() => t.transport.send('x')).toThrow('not open')
  })

  it('asks for a key when it has none, and sends nothing', async () => {
    const t = setup(() => Promise.resolve(undefined))
    t.server.open()
    await t.server.send(helloText())
    expect(t.sent).toEqual([])
    expect(t.handlers.closed).toHaveBeenCalledWith('a key is needed')
    expect(t.socketClose).toHaveBeenCalled()
  })

  it('forgets the key when ref-web closes with 4401', async () => {
    const t = setup()
    t.server.open()
    await t.server.send(helloText())
    t.server.close(CLOSE_KEY_REJECTED, 'key rejected')
    expect(t.keys.rejected).toHaveBeenCalledOnce()
    expect(t.handlers.closed).toHaveBeenCalledWith('key rejected')
  })

  it('reports any other close with its reason or code', async () => {
    const t = await opened()
    t.server.close(1001, 'ref-web is shutting down')
    expect(t.handlers.closed).toHaveBeenCalledWith('ref-web is shutting down')
    expect(t.keys.rejected).not.toHaveBeenCalled()
    const bare = await opened()
    bare.server.close(1006)
    expect(bare.handlers.closed).toHaveBeenCalledWith('closed (1006)')
  })

  it('reports an end once', async () => {
    const t = await opened()
    await t.server.send('plain text')
    t.server.close(1006)
    expect(t.handlers.closed).toHaveBeenCalledOnce()
  })

  it('closes without forgetting the stored key on a malformed or unknown hello', async () => {
    for (const hello of [
      helloText({ v: 2 }),
      helloText({ salt: 'zz' }),
      helloText({ sn: '00' }),
      'nonsense',
    ]) {
      const t = setup()
      t.server.open()
      await t.server.send(hello)
      expect(t.keys.rejected, hello).not.toHaveBeenCalled()
      expect(t.sent, hello).toEqual([])
      expect(t.handlers.closed, hello).toHaveBeenCalledWith('this ref-web speaks another channel version')
    }

    // With a real KeyStore, a stored root survives a bad hello and works on the next connection.
    const { KeyStore, STORAGE_KEY } = await import('./keystore.ts')
    let stored = JSON.stringify({ salt: fixture.salt, root: fixture.root })
    const store = new KeyStore({
      getItem: (k) => (k === STORAGE_KEY ? stored : null),
      setItem: (k, v) => {
        if (k === STORAGE_KEY) stored = v
      },
      removeItem: (k) => {
        if (k === STORAGE_KEY) stored = ''
      },
    })
    let socketHandlers!: SocketHandlers
    const sent: (string | Uint8Array)[] = []
    const factory = channelTransport(
      (given) => {
        socketHandlers = given
        return { send: (data) => void sent.push(data), close: vi.fn() }
      },
      store,
      () => hex(fixture.cn, 16),
    )
    factory({ opened: vi.fn(), message: vi.fn(), closed: vi.fn() })
    socketHandlers.opened()
    socketHandlers.message(helloText({ v: 2 }))
    await flush()
    expect(store.required).toBe(false)
    expect(store.hasKey()).toBe(true)
    expect(sent).toEqual([])
  })

  it('stops on frames that do not open or come out of order', async () => {
    const changed = frame(0).slice()
    changed[3]! ^= 1
    const cases: [string, Uint8Array[]][] = [
      ['changed frame 0', [changed]],
      ['wrong order', [frame(1)]],
      ['truncated', [frame(0).slice(0, 10)]],
    ]
    for (const [name, frames] of cases) {
      const t = setup()
      t.server.open()
      await t.server.send(helloText())
      for (const f of frames) await t.server.send(f)
      expect(t.handlers.opened, name).not.toHaveBeenCalled()
      expect(t.handlers.closed, name).toHaveBeenCalledWith('a frame from ref-web did not open')
    }
    const repeated = await opened()
    await repeated.server.send(frame(0))
    expect(repeated.handlers.closed).toHaveBeenCalledWith('a frame from ref-web did not open')
    const dropped = await opened()
    await dropped.server.send(frame(2)) // frame 1 never came
    expect(dropped.handlers.closed).toHaveBeenCalledWith('a frame from ref-web did not open')
  })

  it('stops on messages of the wrong shape', async () => {
    const plainAfterHello = await opened()
    await plainAfterHello.server.send('{"kind":"event"}')
    expect(plainAfterHello.handlers.closed).toHaveBeenCalledWith('ref-web sent an unsealed message')

    const binaryFirst = setup()
    binaryFirst.server.open()
    await binaryFirst.server.send(new Uint8Array(40))
    expect(binaryFirst.handlers.closed).toHaveBeenCalledWith('ref-web did not start with a hello')
    expect(binaryFirst.keys.rootFor).not.toHaveBeenCalled()
  })

  it('stops when frame 0 is not the start information', async () => {
    const keys = channelKeys(hex(fixture.root, 32), hex(fixture.sn, 16), hex(fixture.cn, 16))
    for (const body of ['not json', '{"version":"0.2.1"}', '{"version":1,"cwd":"/","mock":false}']) {
      const t = setup()
      t.server.open()
      await t.server.send(helloText())
      await t.server.send(new Sealer(keys.toPage).seal(KIND_TEXT, utf8(body)))
      expect(t.handlers.opened, body).not.toHaveBeenCalled()
      expect(t.handlers.closed, body).toHaveBeenCalledWith('ref-web sent no start information')
    }
  })

  it('closing it ends the socket and reports nothing', async () => {
    const t = await opened()
    t.transport.close()
    expect(t.socketClose).toHaveBeenCalled()
    t.server.close(1000)
    expect(t.handlers.closed).not.toHaveBeenCalled()
  })

  it('sends sealed connect_upstream and waits for upstream_ready when channel_control is true', async () => {
    const keys = { rootFor: vi.fn().mockResolvedValue(hex(fixture.root, 32)), rejected: vi.fn() }
    const sent: (string | Uint8Array)[] = []
    let socketHandlers!: SocketHandlers
    const handlers = { opened: vi.fn(), message: vi.fn(), closed: vi.fn() } satisfies TransportHandlers
    const factory = channelTransport(
      (given) => {
        socketHandlers = given
        return { send: (data) => void sent.push(data), close: vi.fn() }
      },
      keys,
      () => hex(fixture.cn, 16),
      { mode: 'explicit', url: 'ws://custom:9000/ws', token: 'tok' },
    )
    factory(handlers)
    socketHandlers.opened()
    socketHandlers.message(helloText())
    await flush()

    const derived = channelKeys(hex(fixture.root, 32), hex(fixture.sn, 16), hex(fixture.cn, 16))
    const serverSealer = new Sealer(derived.toPage)
    const serverOpener = new Opener(derived.toServer)
    const frame0 = serverSealer.seal(
      KIND_TEXT,
      utf8(JSON.stringify({ version: '0.2.4', cwd: '/work', mock: false, channel_control: true })),
    )
    socketHandlers.message(frame0)
    await flush()

    expect(handlers.opened).not.toHaveBeenCalled()
    expect(sent).toHaveLength(2)
    const ctrlWire = sent[1] as Uint8Array
    const ctrlOpened = serverOpener.open(ctrlWire)
    expect(ctrlOpened.kind).toBe(KIND_BINARY)
    expect(JSON.parse(new TextDecoder().decode(ctrlOpened.plaintext))).toEqual({
      op: 'connect_upstream',
      mode: 'explicit',
      url: 'ws://custom:9000/ws',
      token: 'tok',
    })

    const ready = serverSealer.seal(KIND_BINARY, utf8(JSON.stringify({ op: 'upstream_ready' })))
    socketHandlers.message(ready)
    await flush()
    expect(handlers.opened).toHaveBeenCalledOnce()
  })
})
