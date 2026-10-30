import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PROTOCOL_VERSION } from '../proto/generated'
import type { NotifyKind } from '../proto/generated'
import { RefConnection, RefError } from './connection.ts'
import type { ConnectionState } from './connection.ts'
import type { TransportHandlers } from './transport.ts'

// A transport the test drives by hand.
class FakeSocket {
  sent: Record<string, unknown>[] = []
  closedByClient = false

  readonly handlers: TransportHandlers

  constructor(handlers: TransportHandlers) {
    this.handlers = handlers
  }

  send(text: string): void {
    this.sent.push(JSON.parse(text) as Record<string, unknown>)
  }

  close(): void {
    this.closedByClient = true
    this.handlers.closed('closed')
  }

  serverSends(frame: unknown): void {
    this.handlers.message(typeof frame === 'string' ? frame : JSON.stringify(frame))
  }

  respond(id: unknown, result: unknown = {}): void {
    this.serverSends({ kind: 'response', id, ok: true, result, error: null })
  }
}

function setup(retryDelaysMs = [100, 200], notifyKinds?: () => NotifyKind[]) {
  const sockets: FakeSocket[] = []
  const connection = new RefConnection({
    connect: (handlers) => {
      const socket = new FakeSocket(handlers)
      sockets.push(socket)
      return socket
    },
    client: 'ref-web-test',
    cwd: '/work',
    notifyKinds,
    retryDelaysMs,
  })
  const states: ConnectionState[] = []
  const errors: Error[] = []
  connection.onState((state) => states.push(state))
  connection.onError((error) => errors.push(error))
  return { connection, sockets, states, errors }
}

// Opens and completes the handshake on the latest socket.
async function openFully(env: ReturnType<typeof setup>): Promise<FakeSocket> {
  env.connection.open()
  const socket = env.sockets.at(-1)!
  socket.handlers.opened()
  socket.respond(socket.sent[0]!['id'], { server_version: '1', protocol: PROTOCOL_VERSION, cwd: '/work' })
  await vi.waitFor(() => expect(env.connection.state).toBe('open'))
  return socket
}

describe('RefConnection', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('says hello first and opens once the server answers', async () => {
    const env = setup()
    const socket = await openFully(env)

    expect(socket.sent[0]).toMatchObject({
      op: 'hello',
      client: 'ref-web-test',
      version: PROTOCOL_VERSION,
      cwd: '/work',
      notify: [],
    })
    expect(env.states).toEqual(['connecting', 'open'])
    expect(env.connection.hello?.cwd).toBe('/work')
  })

  it('asks for the notification kinds at every handshake', async () => {
    let kinds: NotifyKind[] = ['turn_done']
    const env = setup([100], () => kinds)
    const socket = await openFully(env)
    expect(socket.sent[0]).toMatchObject({ op: 'hello', notify: ['turn_done'] })

    kinds = ['permission', 'turn_error']
    socket.handlers.closed('gone')
    vi.advanceTimersByTime(100)
    const second = env.sockets.at(-1)!
    second.handlers.opened()
    expect(second.sent[0]).toMatchObject({ op: 'hello', notify: ['permission', 'turn_error'] })
  })

  it('matches responses to requests by id, in any order', async () => {
    const env = setup()
    const socket = await openFully(env)

    const first = env.connection.request({ op: 'ping' })
    const second = env.connection.request({ op: 'list_sessions' })
    const [, firstFrame, secondFrame] = socket.sent
    socket.respond(secondFrame!['id'], { sessions: [] })
    socket.respond(firstFrame!['id'], { pong: true })

    await expect(second).resolves.toEqual({ sessions: [] })
    await expect(first).resolves.toEqual({ pong: true })
  })

  it('rejects a request the server answers with ok: false', async () => {
    const env = setup()
    const socket = await openFully(env)

    const request = env.connection.request({ op: 'ping' })
    socket.serverSends({
      kind: 'response',
      id: socket.sent[1]!['id'],
      ok: false,
      result: null,
      error: 'nope',
    })

    await expect(request).rejects.toThrow(new RefError('nope'))
  })

  it('rejects requests while not connected', async () => {
    const env = setup()
    await expect(env.connection.request({ op: 'ping' })).rejects.toThrow('not connected')
  })

  it('hands events, sessions, toasts and notifications to frame listeners', async () => {
    const env = setup()
    const socket = await openFully(env)
    const seen: string[] = []
    env.connection.onFrame((frame) => seen.push(frame.kind))

    socket.serverSends({ kind: 'sessions', sessions: [] })
    socket.serverSends({ kind: 'toast', level: 'info', text: 'hi' })
    socket.serverSends({ kind: 'event', session_id: 's', event: { type: 'closed' } })

    expect(seen).toEqual(['sessions', 'toast', 'event'])
  })

  it('reports malformed frames instead of dropping them silently', async () => {
    const env = setup()
    const socket = await openFully(env)

    socket.serverSends('not json')
    socket.serverSends({ kind: 'mystery' })
    socket.respond(999)

    expect(env.errors.map((e) => e.message)).toEqual([
      expect.stringContaining('invalid JSON'),
      'unknown frame kind "mystery"',
      'response for unknown request 999',
    ])
  })

  it('fails pending requests when the connection drops, then reconnects and says hello again', async () => {
    const env = setup([100])
    const socket = await openFully(env)
    const request = env.connection.request({ op: 'ping' })

    socket.handlers.closed('gone')
    await expect(request).rejects.toThrow('the connection was lost: gone')
    expect(env.connection.state).toBe('connecting')

    vi.advanceTimersByTime(100)
    const second = env.sockets.at(-1)!
    expect(second).not.toBe(socket)
    second.handlers.opened()
    second.respond(second.sent[0]!['id'], { server_version: '1', protocol: PROTOCOL_VERSION, cwd: '/work' })
    await vi.waitFor(() => expect(env.connection.state).toBe('open'))
    expect(second.sent[0]).toMatchObject({ op: 'hello' })
  })

  it('backs off between attempts, repeating the last delay', () => {
    const env = setup([100, 300])
    env.connection.open()

    env.sockets.at(-1)!.handlers.closed('refused')
    vi.advanceTimersByTime(99)
    expect(env.sockets).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(env.sockets).toHaveLength(2)

    env.sockets.at(-1)!.handlers.closed('refused')
    vi.advanceTimersByTime(299)
    expect(env.sockets).toHaveLength(2)
    vi.advanceTimersByTime(1)
    expect(env.sockets).toHaveLength(3)

    env.sockets.at(-1)!.handlers.closed('refused')
    vi.advanceTimersByTime(300)
    expect(env.sockets).toHaveLength(4)
  })

  it('stops for good when the server refuses the handshake', async () => {
    const env = setup()
    env.connection.open()
    const socket = env.sockets[0]!
    socket.handlers.opened()
    socket.serverSends({
      kind: 'response',
      id: socket.sent[0]!['id'],
      ok: false,
      result: null,
      error: 'bad token',
    })

    await vi.waitFor(() => expect(env.connection.state).toBe('closed'))
    vi.advanceTimersByTime(60_000)
    expect(env.sockets).toHaveLength(1)
  })

  it('does not reconnect after close()', async () => {
    const env = setup()
    await openFully(env)

    env.connection.close()
    vi.advanceTimersByTime(60_000)

    expect(env.connection.state).toBe('closed')
    expect(env.sockets).toHaveLength(1)
  })
})
