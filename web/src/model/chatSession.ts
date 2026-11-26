// One session as the window knows it: its conversation and how this window is attached to it.
//
//   events --> Transcript (model) --flush each frame--> blocks (shallowRef) --> components
//
// `blocks` is replaced as a whole, at most once per frame. The Workspace creates one per
// session the window has opened and keeps it until the session is closed, so switching away
// only detaches: the next attach replaces the history.
//
// The state is read through getters, so a template or computed that reads it is tracked.

import { effectScope, markRaw, reactive, ref, shallowReactive } from 'vue'
import type { EffectScope } from 'vue'
import { describeError } from '../errorMessage.ts'
import { useFrame } from '../motion/useFrame.ts'
import { RefError } from '../net/connection.ts'
import type { RefApi } from '../net/api.ts'
import type {
  Message,
  NamedImage,
  PermissionRequest,
  QuestionRequest,
  SessionEvent,
  SessionInfo,
  ToastLevel,
} from '../proto/generated'
import type { Block } from './blocks.ts'
import { QuestionForm } from './questionForm.ts'
import { EVICT_BUFFER, PAGE_SIZE, TAIL_KEEP_MESSAGES, Transcript } from './transcript.ts'
import type { TranscriptSlot } from './transcript.ts'
// How this window is attached: `foreground` counts as a direct connection, `watching` only
// receives events (split view tiles).
export type Attachment = 'detached' | 'watching' | 'foreground'

// What the session is doing, for the status line.
export type Activity =
  'idle' | 'thinking' | 'running' | 'waiting_permission' | 'waiting_question' | 'retrying' | 'compacting'

export interface ChatSessionDeps {
  api(): RefApi
  notify(level: ToastLevel, text: string): void
  // Undo took a prompt back: the composer of the session gets its text and pictures.
  draftRestored(sessionId: string, text: string, images: NamedImage[]): void
}

// The backend's answer to a reply for a question or permission nobody waits for any more.
const NO_PENDING_QUESTION = 'No pending question with that id.'
const NO_PENDING_PERMISSION = 'No pending permission request with that id.'

function isNoPendingQuestion(error: unknown): boolean {
  if (error instanceof RefError && error.code === 'no_pending_question') return true
  return error instanceof Error && error.message === NO_PENDING_QUESTION // remove with protocol 5
}

function isNoPendingPermission(error: unknown): boolean {
  if (error instanceof RefError && error.code === 'no_pending_permission') return true
  return error instanceof Error && error.message === NO_PENDING_PERMISSION // remove with protocol 5
}

export const RANGE_DEBOUNCE_MS = 80

export class ChatSession {
  readonly id: string

  private readonly state = shallowReactive({
    blocks: [] as readonly Block[],
    slots: [] as readonly TranscriptSlot[],
    info: undefined as SessionInfo | undefined,
    permissions: [] as PermissionRequest[],
    questions: [] as QuestionRequest[],
    sendingAnswers: [] as string[], // call ids whose answer is on its way
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
  private readonly messageImages = new Map<string, Set<string>>() // message id -> image ids
  private readonly forms = new Map<string, QuestionForm>() // call id -> what the user chose so far
  private rangeTimer: ReturnType<typeof setTimeout> | undefined
  private rangeSerial = 0
  private pendingRange: { startSeq: number; endSeq: number } | undefined

  // `info` is what the session list already knows, so the session can be shown before it is attached.
  constructor(id: string, deps: ChatSessionDeps, info?: SessionInfo) {
    this.id = id
    this.deps = deps
    this.state.info = info
    this.transcript = markRaw(this.newTranscript())
    this.scope.run(() => {
      useFrame(this.flushing, (_frameSeconds, now) => {
        if (this.transcript.flush(now)) {
          this.state.blocks = this.transcript.blocks
          this.state.slots = this.transcript.slots
        }
        this.flushing.value = this.transcript.needsFlush
      })
    })
  }

  get blocks(): readonly Block[] {
    return this.state.blocks
  }

  get slots(): readonly TranscriptSlot[] {
    return this.state.slots
  }

  get totalMessages(): number {
    return this.state.slots.length
  }

  get residentCount(): number {
    return this.transcript.residentCount
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

  // The oldest question that still waits for an answer from this window.
  get pendingQuestion(): QuestionRequest | undefined {
    return this.state.questions.find((request) => !this.state.sendingAnswers.includes(request.call_id))
  }

  // Whether the answer for the call is on its way (the inputs of the card are locked).
  isSendingAnswer(callId: string): boolean {
    return this.state.sendingAnswers.includes(callId)
  }

  // The choices of one question call; they live as long as the question is pending.
  questionForm(request: QuestionRequest): QuestionForm {
    let form = this.forms.get(request.call_id)
    if (!form) {
      form = reactive(new QuestionForm(request)) as QuestionForm
      this.forms.set(request.call_id, form)
    }
    return form
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
    this.messageImages.clear()
    this.recordMessageImages(snapshot.messages)
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
    this.cancelPendingRange()
    await this.deps.api().detach(this.id)
    this.state.attachment = 'detached'
    this.images.clear() // pictures are fetched again when the session is shown again
    this.messageImages.clear()
    if (this.state.loaded) {
      const evicted = this.transcript.evictBeforeTail(TAIL_KEEP_MESSAGES)
      if (evicted.length > 0) {
        this.transcript.flush(performance.now())
        this.state.blocks = this.transcript.blocks
        this.state.slots = this.transcript.slots
      }
    }
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
    if (event.type === 'message') this.recordMessageImages([event.message])
    else if (event.type === 'reset') {
      this.cancelPendingRange()
      this.messageImages.clear()
      this.recordMessageImages(event.messages)
    } else if (event.type === 'part') {
      this.recordPartImages(event.message_id, event.part)
    } else if (event.type === 'removed') {
      this.cancelPendingRange()
    }
    this.transcript.apply(event, performance.now())
    this.publish()
  }

  // The connection came back: the history may have moved on without us.
  async resync(): Promise<void> {
    if (this.state.attachment === 'detached') return
    const snapshot = await this.deps.api().attach(this.id, this.state.attachment === 'foreground')
    this.messageImages.clear()
    this.recordMessageImages(snapshot.messages)
    this.transcript.load(snapshot)
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
      const page = await this.deps.api().messagesBefore(this.id, before, PAGE_SIZE)
      if (generation !== this.state.generation) return
      this.recordMessageImages(page.messages)
      this.transcript.prepend(page.messages, page.more_before, page.start_seq)
      this.publish()
    } catch (error) {
      if (generation !== this.state.generation) return
      this.state.olderError = describeError(error)
    } finally {
      if (generation === this.state.generation) this.state.loadingOlder = false
    }
  }

  // Debounced range load for visible placeholder slots. Drops stale responses by generation and serial.
  requestRange(visibleStart: number, visibleEnd: number): void {
    const missing = this.transcript.missingRangeAround(visibleStart, visibleEnd)
    if (!missing) {
      this.clearRangeTimer()
      this.pendingRange = undefined
      return
    }
    const unchanged =
      this.rangeTimer !== undefined &&
      this.pendingRange?.startSeq === missing.startSeq &&
      this.pendingRange?.endSeq === missing.endSeq
    if (unchanged) return
    this.pendingRange = missing
    this.clearRangeTimer()
    this.rangeTimer = setTimeout(() => {
      this.rangeTimer = undefined
      const target = this.pendingRange
      this.pendingRange = undefined
      if (!target) return
      void this.loadRange(target.startSeq, target.endSeq).catch((error) => {
        this.deps.notify('error', describeError(error))
      })
    }, RANGE_DEBOUNCE_MS)
  }

  async loadRange(startSeq: number, endSeq: number): Promise<void> {
    if (startSeq >= endSeq) return
    const generation = this.state.generation
    const serial = ++this.rangeSerial
    const page = await this.deps.api().messagesRange(this.id, startSeq, endSeq, endSeq - startSeq)
    if (generation !== this.state.generation || serial !== this.rangeSerial) return
    this.recordMessageImages(page.messages)
    this.transcript.insertRange(page.start_seq, page.end_seq, page.messages)
    if (page.start_seq === 0 && !page.more_before) this.transcript.moreBefore = false
    this.publish()
  }

  // Trigger 1: scroll away -> evict outside [visibleStart - 500, visibleEnd + 500].
  evictAround(visibleStart: number, visibleEnd: number): void {
    const evicted = this.transcript.evictOutside(
      Math.max(0, visibleStart - EVICT_BUFFER),
      visibleEnd + EVICT_BUFFER,
    )
    if (evicted.length === 0) return
    this.dropEvictedImages(evicted)
    this.publish()
  }

  // Trigger 2: settle at bottom while following -> evict older than total - 1000.
  evictAtBottom(): void {
    const evicted = this.transcript.evictBeforeTail(TAIL_KEEP_MESSAGES)
    if (evicted.length === 0) return
    this.dropEvictedImages(evicted)
    this.publish()
  }

  // Takes back the message (default: the last prompt) and everything after it.
  async undo(messageId = ''): Promise<void> {
    this.cancelPendingRange()
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

  async retryNow(): Promise<boolean> {
    return this.deps.api().retryNow(this.id)
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
      if (isNoPendingPermission(error)) return
      this.transcript.apply({ type: 'permission', request }, performance.now())
      this.publish()
      throw error
    }
  }

  // `answers` has one list per question; null dismisses them. The card stays, locked, until
  // the backend reports the question resolved. A question that is gone by then (answered in
  // another window) is dropped without a word; a refused answer unlocks the card and throws.
  async answerQuestion(callId: string, answers: string[][] | null): Promise<void> {
    if (this.isSendingAnswer(callId)) return
    this.state.sendingAnswers = [...this.state.sendingAnswers, callId]
    try {
      await this.deps.api().questionReply(this.id, callId, answers)
    } catch (error) {
      this.state.sendingAnswers = this.state.sendingAnswers.filter((id) => id !== callId)
      if (!isNoPendingQuestion(error)) throw error
      this.transcript.resolveQuestion(callId)
      this.publish()
    }
  }

  dispose(): void {
    this.cancelPendingRange()
    this.images.clear()
    this.messageImages.clear()
    this.scope.stop()
  }

  private clearRangeTimer(): void {
    if (this.rangeTimer === undefined) return
    clearTimeout(this.rangeTimer)
    this.rangeTimer = undefined
  }

  private cancelPendingRange(): void {
    this.clearRangeTimer()
    this.pendingRange = undefined
    this.rangeSerial++
  }

  private recordMessageImages(messages: readonly Message[]): void {
    for (const message of messages) {
      for (const part of message.parts) this.recordPartImages(message.id, part)
    }
  }

  private recordPartImages(messageId: string, part: Message['parts'][number]): void {
    const ids: string[] = []
    if (part.type === 'image' && part.id) ids.push(part.id)
    else if (part.type === 'tool' && part.images) {
      for (const image of part.images) {
        if (image.id) ids.push(image.id)
      }
    }
    if (ids.length === 0) return
    let set = this.messageImages.get(messageId)
    if (!set) {
      set = new Set<string>()
      this.messageImages.set(messageId, set)
    }
    for (const id of ids) set.add(id)
  }

  private dropEvictedImages(evictedMessageIds: readonly string[]): void {
    for (const messageId of evictedMessageIds) {
      const ids = this.messageImages.get(messageId)
      if (ids) {
        for (const imageId of ids) this.images.delete(imageId)
        this.messageImages.delete(messageId)
      }
    }
  }

  private newTranscript(): Transcript {
    return new Transcript({ reportError: (message) => this.deps.notify('error', message) })
  }

  private showHistory(): void {
    this.cancelPendingRange()
    this.transcript.flush(performance.now())
    this.state.blocks = this.transcript.blocks
    this.state.slots = this.transcript.slots
    this.state.generation++
    this.state.loadingOlder = false // a page still on its way is dropped
    this.state.olderError = ''
    this.publish()
  }
  private forgetAnsweredQuestions(): void {
    const pending = new Set(this.state.questions.map((request) => request.call_id))
    for (const callId of this.forms.keys()) if (!pending.has(callId)) this.forms.delete(callId)
    const sending = this.state.sendingAnswers.filter((callId) => pending.has(callId))
    if (sending.length !== this.state.sendingAnswers.length) this.state.sendingAnswers = sending
  }

  private publish(): void {
    this.state.info = this.transcript.info
    this.state.moreBefore = this.transcript.moreBefore
    this.state.lastToolDelayed = this.transcript.lastToolDelayed
    this.state.permissions = [...this.transcript.permissions.values()]
    this.state.questions = [...this.transcript.questions.values()]
    this.forgetAnsweredQuestions()
    this.state.queued = this.transcript.queued
    this.flushing.value = true
  }
}
