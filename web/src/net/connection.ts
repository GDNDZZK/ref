// One protocol connection: handshake, request/response matching by id, server frames,
// and reconnecting with backoff.
//
//   transport frame --> parse --> response?  --> resolves the request with that id
//                              \-> otherwise --> frame listeners
//
// After a reconnect the handshake runs again and listeners are told the connection is
// `open` once more; whatever the upper layer attached is its own to restore.

import { PROTOCOL_VERSION } from '../proto/generated'
import type { JsonValue, NotifyKind, Request, ServerFrame } from '../proto/generated'
import type { Transport, TransportFactory } from './transport.ts'

export type ConnectionState = 'connecting' | 'open' | 'closed'

export type EventFrame = Exclude<ServerFrame, { kind: 'response' }>

export interface HelloResult {
  server_version: string
  protocol?: number // absent in servers older than protocol 2
  cwd: string
}

export class RefError extends Error {}

export interface ConnectionOptions {
  connect: TransportFactory
  client: string // shown to other clients, e.g. "ref-web"
  cwd: string
  token?: string
  notifyKinds?: () => NotifyKind[] // asked at each handshake, so a reconnect carries the current choice
  retryDelaysMs?: number[] // waits between reconnect attempts; the last one repeats
  // Asked after each lost connection; true stops the retrying for good (e.g. the login expired).
  giveUp?: () => Promise<boolean>
}

const DEFAULT_RETRY_DELAYS_MS = [500, 1000, 2000, 4000, 8000]

interface Pending {
  resolve(result: JsonValue): void
  reject(error: Error): void
}

export class RefConnection {
  private transport: Transport | undefined
  private nextId = 1
  private pending = new Map<number, Pending>()
  private currentState: ConnectionState = 'connecting'
  private attempt = 0
  private retryTimer: ReturnType<typeof setTimeout> | undefined
  private readonly frameListeners = new Set<(frame: EventFrame) => void>()
  private readonly stateListeners = new Set<(state: ConnectionState, detail: string) => void>()
  private readonly errorListeners = new Set<(error: Error) => void>()
  private helloResult: HelloResult | undefined

  private readonly options: ConnectionOptions

  constructor(options: ConnectionOptions) {
    this.options = options
  }

  get state(): ConnectionState {
    return this.currentState
  }

  get hello(): HelloResult | undefined {
    return this.helloResult
  }

  open(): void {
    this.dial()
  }

  close(detail = 'closed by the client'): void {
    clearTimeout(this.retryTimer)
    this.setState('closed', detail)
    this.failPending(new RefError('the connection was closed'))
    this.transport?.close()
    this.transport = undefined
  }

  onFrame(listener: (frame: EventFrame) => void): () => void {
    this.frameListeners.add(listener)
    return () => this.frameListeners.delete(listener)
  }

  onState(listener: (state: ConnectionState, detail: string) => void): () => void {
    this.stateListeners.add(listener)
    return () => this.stateListeners.delete(listener)
  }

  // Frames the client could not make sense of.
  onError(listener: (error: Error) => void): () => void {
    this.errorListeners.add(listener)
    return () => this.errorListeners.delete(listener)
  }

  // Rejects with RefError when the server answers `ok: false` or the connection goes away.
  request(request: Request): Promise<JsonValue> {
    const transport = this.transport
    if (!transport || this.currentState !== 'open') {
      return Promise.reject(new RefError('not connected'))
    }
    return this.send(transport, request)
  }

  private send(transport: Transport, request: Request): Promise<JsonValue> {
    const id = this.nextId++
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject })
      transport.send(JSON.stringify({ id, ...request }))
    })
  }

  private dial(): void {
    this.setState('connecting', this.attempt === 0 ? 'connecting' : `reconnecting (attempt ${this.attempt})`)
    const transport = this.options.connect({
      opened: () => void this.handshake(transport),
      message: (text) => this.receive(text),
      closed: (reason) => this.lost(transport, reason),
    })
    this.transport = transport
  }

  private async handshake(transport: Transport): Promise<void> {
    const { client, cwd, token, notifyKinds } = this.options
    try {
      const result = await this.send(transport, {
        op: 'hello',
        client,
        version: PROTOCOL_VERSION,
        cwd,
        token: token ?? null,
        notify: notifyKinds?.() ?? [],
      })
      this.helloResult = result as unknown as HelloResult
    } catch (error) {
      const lostMeanwhile = transport !== this.transport
      if (lostMeanwhile) return // lost() has dealt with it
      // The server refused us (bad token, wrong protocol): retrying would be refused again.
      this.close(error instanceof Error ? error.message : String(error))
      return
    }
    this.attempt = 0
    this.setState('open', 'connected')
  }

  private lost(transport: Transport, reason: string): void {
    if (transport !== this.transport) return // an old socket closing late
    this.transport = undefined
    this.failPending(new RefError(`the connection was lost: ${reason}`))
    if (this.currentState === 'closed') return

    const delays = this.options.retryDelaysMs ?? DEFAULT_RETRY_DELAYS_MS
    const delay = delays[Math.min(this.attempt, delays.length - 1)]!
    this.attempt++
    this.setState('connecting', `lost (${reason}); retrying`)
    const timer = setTimeout(() => this.dial(), delay)
    this.retryTimer = timer
    void this.askToGiveUp(timer)
  }

  private async askToGiveUp(timer: ReturnType<typeof setTimeout>): Promise<void> {
    const giveUp = await this.options.giveUp?.().catch(() => false)
    if (giveUp && this.retryTimer === timer) this.close('the connection was refused for good')
  }

  private receive(text: string): void {
    let frame: ServerFrame
    try {
      frame = JSON.parse(text) as ServerFrame
    } catch (error) {
      this.reportError(new RefError(`invalid JSON from the server: ${String(error)}`))
      return
    }

    if (frame.kind === 'response') {
      this.settle(frame)
      return
    }
    switch (frame.kind) {
      case 'event':
      case 'sessions':
      case 'toast':
      case 'notify':
        for (const listener of this.frameListeners) listener(frame)
        return
      default:
        this.reportError(new RefError(`unknown frame kind "${String((frame as { kind: unknown }).kind)}"`))
    }
  }

  private settle(frame: Extract<ServerFrame, { kind: 'response' }>): void {
    const pending = this.pending.get(frame.id)
    if (!pending) {
      this.reportError(new RefError(`response for unknown request ${frame.id}`))
      return
    }
    this.pending.delete(frame.id)
    if (frame.ok) pending.resolve(frame.result)
    else pending.reject(new RefError(frame.error ?? 'request failed'))
  }

  private failPending(error: Error): void {
    const failed = [...this.pending.values()]
    this.pending.clear()
    for (const pending of failed) pending.reject(error)
  }

  private setState(state: ConnectionState, detail: string): void {
    this.currentState = state
    for (const listener of this.stateListeners) listener(state, detail)
  }

  private reportError(error: unknown): void {
    const wrapped = error instanceof Error ? error : new RefError(String(error))
    for (const listener of this.errorListeners) listener(wrapped)
  }
}
