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
import type { Message, NamedImage, PermissionRequest, SessionEvent, SessionInfo } from '../proto/generated'
import type { Block } from './blocks.ts'
import { Transcript } from './transcript.ts'

// How this window is attached: `foreground` counts as a direct connection, `watching` only
// receives events (split view tiles).
export type Attachment = 'detached' | 'watching' | 'foreground'

// What the session is doing, for the status line.
export type Activity = 'idle' | 'thinking' | 'running' | 'waiting_permission' | 'retrying' | 'compacting'

export interface ChatSessionDeps {
  api(): RefApi
  notify(level: 'info' | 'warn' | 'error', text: string): void
  // Undo took a prompt back: the composer of the session gets its text and pictures.
  draftRestored(sessionId: string, text: string, images: NamedImage[]): void
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
    moreBefore: false,
    loadingOlder: false,
    olderError: '',
    lastToolDelayed: false,
    thinkingExpanded: false,
    detailsExpanded: false,
  })
  private readonly deps: ChatSessionDeps
  private readonly scope: EffectScope = effectScope()
  private transcript: Transcript
  private readonly flushing = ref(false) // something waits for the next frame
  private readonly images = new Map<string, Promise<string>>() // image id -> data URL

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

  get activity(): Activity {
    const status = this.state.info?.status
    if (!status) return 'idle'
    if (status.state === 'busy') return 'thinking'
    return status.state
  }

  // When the current run started (ms), if one is running.
  get busySince(): number | undefined {
    return this.state.info?.busy_since ?? undefined
  }

  get retry(): { attempt: number; at: number; error: string } | undefined {
    const status = this.state.info?.status
    return status?.state === 'retrying'
      ? { attempt: status.attempt, at: status.next_at, error: status.error }
      : undefined
  }

  get context(): { used: number; max: number; recommend: number; auto: number } | undefined {
    return this.state.info?.context
  }

  get moreBefore(): boolean {
    return this.state.moreBefore
  }

  get loadingOlder(): boolean {
    return this.state.loadingOlder
  }

  get olderError(): string {
    return this.state.olderError
  }

  get lastToolDelayed(): boolean {
    return this.state.lastToolDelayed
  }

  get pendingPermission(): PermissionRequest | undefined {
    return this.state.permissions[0]
  }

  // The defaults of the folds. A block flips against them when the user toggles it.
  get thinkingExpanded(): boolean {
    return this.state.thinkingExpanded
  }

  get detailsExpanded(): boolean {
    return this.state.detailsExpanded
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
    this.images.clear() // pictures are fetched again when the session is shown again
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
    this.transcript.clearError()
    this.publish()
    await this.deps.api().prompt(this.id, text)
  }

  // Loads the page of messages before the first one shown. A page that arrives after the
  // history was loaded again belongs to the old one and is dropped.
  async loadOlder(): Promise<void> {
    const before = this.transcript.oldestMessageId
    if (this.state.loadingOlder || !this.state.moreBefore || before === undefined) return
    const generation = this.state.generation
    this.state.loadingOlder = true
    this.state.olderError = ''
    try {
      const page = await this.deps.api().messagesBefore(this.id, before)
      if (generation !== this.state.generation) return
      this.transcript.prepend(page.messages, page.more_before)
      this.publish()
    } catch (error) {
      if (generation !== this.state.generation) return
      this.state.olderError = error instanceof Error ? error.message : String(error)
    } finally {
      if (generation === this.state.generation) this.state.loadingOlder = false
    }
  }

  // Takes back the message (default: the last prompt) and everything after it.
  async undo(messageId = ''): Promise<void> {
    const taken = await this.deps.api().undo(this.id, messageId === '' ? undefined : messageId)
    this.deps.draftRestored(this.id, taken.text, taken.images)
  }

  undoLast(): Promise<void> {
    return this.undo()
  }

  async copyMessage(messageId: string): Promise<void> {
    await navigator.clipboard.writeText(this.transcript.textOf(messageId))
  }

  // Returns false when there is no reply to copy.
  async copyLastReply(): Promise<boolean> {
    const text = this.transcript.lastReplyText()
    if (text === '') return false
    await navigator.clipboard.writeText(text)
    return true
  }

  // The picture a message carries as a stub, as a `data:` URL. Asked once per picture.
  imageUrl(imageId: string): Promise<string> {
    const cached = this.images.get(imageId)
    if (cached) return cached
    const loading = this.deps
      .api()
      .getImage(this.id, imageId)
      .then((image) => `data:${image.mime};base64,${image.data}`)
    this.images.set(imageId, loading)
    loading.catch(() => this.images.delete(imageId)) // a failed load can be tried again
    return loading
  }

  toggleReasoning(): void {
    this.state.thinkingExpanded = !this.state.thinkingExpanded
  }

  toggleDetails(): void {
    this.state.detailsExpanded = !this.state.detailsExpanded
  }

  async stop(): Promise<void> {
    await this.deps.api().abort(this.id)
    this.transcript.settle() // what is on screen should match what the session has
    this.publish()
  }

  // The request leaves at once, so a second key press cannot answer it twice; it comes back
  // when the answer was not sent.
  async answerPermission(callId: string, allow: boolean, always: boolean): Promise<void> {
    const request = this.transcript.resolvePermission(callId)
    if (!request) return
    this.publish()
    try {
      await this.deps.api().permissionReply(this.id, callId, allow, always)
    } catch (error) {
      this.transcript.apply({ type: 'permission', request }, performance.now())
      this.publish()
      throw error
    }
  }

  dispose(): void {
    this.images.clear()
    this.scope.stop()
  }

  private newTranscript(): Transcript {
    return new Transcript({ reportError: (message) => this.deps.notify('error', message) })
  }

  private showHistory(): void {
    this.transcript.flush(performance.now())
    this.state.blocks = this.transcript.blocks
    this.state.generation++
    this.state.loadingOlder = false // a page still on its way is dropped
    this.state.olderError = ''
    this.publish()
  }

  private publish(): void {
    this.state.info = this.transcript.info
    this.state.moreBefore = this.transcript.moreBefore
    this.state.lastToolDelayed = this.transcript.lastToolDelayed
    this.state.permissions = [...this.transcript.permissions.values()]
    this.state.queued = this.transcript.queued
    this.flushing.value = true
  }
}
