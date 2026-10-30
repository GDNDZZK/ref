// One session as the window knows it: its conversation and how this window is attached to it.
//
//   events --> Transcript (model) --flush each frame--> blocks (shallowRef) --> components
//
// `blocks` is replaced as a whole, at most once per frame. The Workspace creates one per
// session the window has opened and keeps it until the session is closed, so switching away
// only detaches: the next attach replaces the history.
//
// The state is read through getters, so a template or computed that reads it is tracked.

import { effectScope, markRaw, ref, shallowReactive } from 'vue'
import type { EffectScope } from 'vue'
import { useFrame } from '../motion/useFrame.ts'
import type { RefApi } from '../net/api.ts'
import type { Message, PermissionRequest, SessionEvent, SessionInfo } from '../proto/generated'
import type { Block } from './blocks.ts'
import { Transcript } from './transcript.ts'

// How this window is attached: `foreground` counts as a direct connection, `watching` only
// receives events (split view tiles).
export type Attachment = 'detached' | 'watching' | 'foreground'

export interface ChatSessionDeps {
  api(): RefApi
  notify(level: 'info' | 'warn' | 'error', text: string): void
}

export class ChatSession {
  readonly id: string

  private readonly state = shallowReactive({
    blocks: [] as readonly Block[],
    info: undefined as SessionInfo | undefined,
    permissions: [] as PermissionRequest[],
    queued: [] as readonly Message[], // waiting for the next safe point, in delivery order
    loaded: false,
    generation: 0, // counts the times the history was loaded
    attachment: 'detached' as Attachment,
  })
  private readonly deps: ChatSessionDeps
  private readonly scope: EffectScope = effectScope()
  private transcript: Transcript
  private readonly flushing = ref(false) // something waits for the next frame

  // `info` is what the session list already knows, so the session can be shown before it is attached.
  constructor(id: string, deps: ChatSessionDeps, info?: SessionInfo) {
    this.id = id
    this.deps = deps
    this.state.info = info
    this.transcript = markRaw(this.newTranscript())
    this.scope.run(() => {
      useFrame(this.flushing, (_frameSeconds, now) => {
        if (this.transcript.flush(now)) this.state.blocks = this.transcript.blocks
        this.flushing.value = this.transcript.needsFlush
      })
    })
  }

  get blocks(): readonly Block[] {
    return this.state.blocks
  }

  get info(): SessionInfo | undefined {
    return this.state.info
  }

  get permissions(): readonly PermissionRequest[] {
    return this.state.permissions
  }

  get queued(): readonly Message[] {
    return this.state.queued
  }

  get loaded(): boolean {
    return this.state.loaded
  }

  get generation(): number {
    return this.state.generation
  }

  get attachment(): Attachment {
    return this.state.attachment
  }

  get busy(): boolean {
    return this.state.info !== undefined && this.state.info.status.state !== 'idle'
  }

  // Attaches (or changes how it is attached) and shows the history the backend sends.
  async attach(foreground: boolean): Promise<void> {
    const snapshot = await this.deps.api().attach(this.id, foreground)
    this.transcript = markRaw(this.newTranscript())
    this.transcript.load(snapshot)
    this.state.attachment = foreground ? 'foreground' : 'watching'
    this.state.loaded = true
    this.showHistory()
  }

  // Moves between foreground and watching. While the window already receives the session's events
  // (split view subscribes to all of them) the history stays as it is; otherwise it is loaded again.
  async setForeground(foreground: boolean): Promise<void> {
    const receivingEvents = this.state.loaded && this.state.attachment !== 'detached'
    if (!receivingEvents) return this.attach(foreground)
    if (foreground) await this.deps.api().attach(this.id, true)
    else await this.deps.api().detach(this.id)
    this.state.attachment = foreground ? 'foreground' : 'watching'
  }

  async detach(): Promise<void> {
    await this.deps.api().detach(this.id)
    this.state.attachment = 'detached'
  }

  // The session list changed: keeps the status, name and the like fresh, also before the first attach.
  setInfo(info: SessionInfo): void {
    if (!this.state.loaded) {
      this.state.info = info
      return
    }
    this.applyEvent({ type: 'info', info })
  }

  applyEvent(event: SessionEvent): void {
    if (!this.state.loaded) return // before the snapshot: the snapshot already has it
    this.transcript.apply(event, performance.now())
    this.publish()
  }

  // The connection came back: the history may have moved on without us.
  async resync(): Promise<void> {
    if (this.state.attachment === 'detached') return
    this.transcript.load(await this.deps.api().attach(this.id, this.state.attachment === 'foreground'))
    this.showHistory()
  }

  async send(text: string): Promise<void> {
    await this.deps.api().prompt(this.id, text)
  }

  async stop(): Promise<void> {
    await this.deps.api().abort(this.id)
    this.transcript.settle() // what is on screen should match what the session has
    this.publish()
  }

  async answerPermission(callId: string, allow: boolean, always: boolean): Promise<void> {
    await this.deps.api().permissionReply(this.id, callId, allow, always)
  }

  dispose(): void {
    this.scope.stop()
  }

  private newTranscript(): Transcript {
    return new Transcript({ reportError: (message) => this.deps.notify('error', message) })
  }

  private showHistory(): void {
    this.transcript.flush(performance.now())
    this.state.blocks = this.transcript.blocks
    this.state.generation++
    this.publish()
  }

  private publish(): void {
    this.state.info = this.transcript.info
    this.state.permissions = [...this.transcript.permissions.values()]
    this.state.queued = this.transcript.queued
    this.flushing.value = true
  }
}
