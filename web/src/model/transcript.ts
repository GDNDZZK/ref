// One session's conversation as blocks.
//
//   snapshot / events --> Transcript --flush(now), once per frame--> Block[]
//                           |  text growth goes through the StreamPacer first
//
// Pure TypeScript, no Vue: the store wraps `blocks` in a shallowRef and calls `flush` from the
// frame loop. Events change the model at once; what the view shows changes only in `flush`,
// which also releases paced text, so a burst of events costs one view update.

import type {
  Delta,
  Message,
  Part,
  PermissionRequest,
  QuestionRequest,
  SessionEvent,
  SessionInfo,
  SessionSnapshot,
} from '../proto/generated'
import { reuseIfEqual } from './blocks.ts'
import type { Block } from './blocks.ts'
import { buildBlocks, messageText } from './buildBlocks.ts'
import type { MessageEntry } from './buildBlocks.ts'
import { PartView } from './partView.ts'
import { StreamPacer } from './streamPacer.ts'
import { partialPurpose } from './toolArgs.ts'

export interface TranscriptOptions {
  reportError(message: string): void // an event that does not fit the conversation
}

interface MessageBlocks {
  entry: MessageEntry
  blocks: Block[]
  stale: boolean // before the last compaction
  lastCompaction: boolean
}

const TAIL_ERROR_KEY = 'session:error'

const STREAM_SEPARATOR = '@'

// A pacer stream is one part of one message: "<part index>@<message id>".
function streamKey(messageId: string, index: number): string {
  return `${index}${STREAM_SEPARATOR}${messageId}`
}

function parseStreamKey(stream: string): { messageId: string; index: number } {
  const at = stream.indexOf(STREAM_SEPARATOR)
  return { index: Number(stream.slice(0, at)), messageId: stream.slice(at + 1) }
}

export class Transcript {
  private readonly options: TranscriptOptions
  private readonly pacer = new StreamPacer()
  private entries: MessageBlocks[] = []
  private readonly byId = new Map<string, MessageBlocks>()
  private readonly dirty = new Set<string>() // messages whose blocks must be rebuilt
  private readonly pacedMessages = new Set<string>() // messages with text in the pacer
  private structureChanged = false
  private tailChanged = false
  private flat: readonly Block[] = []
  private tailError: Block | undefined

  info: SessionInfo | undefined
  moreBefore = false
  closed = false
  readonly permissions = new Map<string, PermissionRequest>()
  readonly questions = new Map<string, QuestionRequest>()
  queued: Message[] = []
  // The last `Error` event; it stays until the user sends the next prompt.
  errorText = ''

  constructor(options: TranscriptOptions) {
    this.options = options
  }

  get blocks(): readonly Block[] {
    return this.flat
  }

  get busy(): boolean {
    return this.info !== undefined && this.info.status.state !== 'idle'
  }

  // Something is waiting for the next `flush`: keep the frame loop running.
  get needsFlush(): boolean {
    return this.dirty.size > 0 || this.structureChanged || this.tailChanged || !this.pacer.isEmpty()
  }

  get oldestMessageId(): string | undefined {
    return this.entries[0]?.entry.message.id
  }

  // Whether the newest tool call still waits for its delay to pass.
  get lastToolDelayed(): boolean {
    const reply = this.entries.findLast(({ entry }) => entry.message.role === 'assistant')
    const tool = reply?.entry.message.parts.findLast((part) => part.type === 'tool')
    return tool?.state === 'delayed'
  }

  // The text of the message as the copy button gives it.
  textOf(messageId: string): string {
    const message = this.byId.get(messageId)?.entry.message
    return message ? messageText(message) : ''
  }

  lastReplyText(): string {
    const reply = this.entries.findLast(
      ({ entry }) => entry.message.role === 'assistant' && messageText(entry.message) !== '',
    )
    return reply ? messageText(reply.entry.message) : ''
  }

  // A prompt was sent: the error of the turn before is history.
  clearError(): void {
    if (this.errorText === '') return
    this.errorText = ''
    this.tailChanged = true
  }

  // A page of older messages: shown at once, above what is there.
  prepend(messages: Message[], moreBefore: boolean): void {
    const added = messages
      .filter((message) => !this.byId.has(message.id))
      .map((message) => this.newEntry(message, false, 0))
    this.entries = [...added, ...this.entries]
    this.moreBefore = moreBefore
    this.structureChanged = true
  }

  load(snapshot: SessionSnapshot): void {
    this.info = snapshot.info
    this.moreBefore = snapshot.more_before
    this.queued = snapshot.queued
    this.closed = false
    this.errorText = ''
    this.tailChanged = true
    this.permissions.clear()
    for (const request of snapshot.pending_permissions) this.permissions.set(request.call_id, request)
    this.questions.clear()
    for (const request of snapshot.pending_questions) this.questions.set(request.call_id, request)
    this.replaceMessages(snapshot.messages)
  }

  apply(event: SessionEvent, now: number): void {
    switch (event.type) {
      case 'info':
        this.setInfo(event.info)
        break
      case 'message':
        this.upsertMessage(event.message, now)
        break
      case 'part':
        this.replacePart(event.message_id, event.index, event.part, now)
        break
      case 'delta':
        this.applyDelta(event.message_id, event.index, event.delta, now)
        break
      case 'removed':
        this.removeMessage(event.message_id)
        break
      case 'reset':
        this.moreBefore = event.more_before
        this.replaceMessages(event.messages, true)
        break
      case 'permission':
        this.permissions.set(event.request.call_id, event.request)
        this.markDirty(event.request.message_id)
        break
      case 'permission_resolved':
        this.resolvePermission(event.call_id)
        break
      case 'question':
        this.questions.set(event.request.call_id, event.request)
        this.markDirty(event.request.message_id)
        break
      case 'question_resolved':
        this.resolveQuestion(event.call_id)
        break
      case 'queue':
        this.queued = event.messages
        break
      case 'closed':
        this.closed = true
        break
      case 'error':
        this.errorText = event.text
        this.tailChanged = true
        break
    }
  }

  // Shows everything the pacer holds, e.g. when the user stops the turn.
  settle(): void {
    for (const chunk of this.pacer.takeAll()) this.show(chunk.stream, chunk.text)
    this.pacedMessages.forEach((id) => this.markDirty(id))
    this.pacedMessages.clear()
  }

  // Releases what the pacer says is due and rebuilds the blocks that changed.
  // Returns true when `blocks` is a new array.
  flush(now: number): boolean {
    for (const chunk of this.pacer.take(now)) this.show(chunk.stream, chunk.text)
    this.markDrained()
    if (this.dirty.size === 0 && !this.structureChanged && !this.tailChanged) return false

    // Whether a message is the first decides its turn divider.
    const first = this.entries[0]
    if (this.structureChanged && first) this.dirty.add(first.entry.message.id)
    if (this.structureChanged) this.markCompactions()
    for (const id of this.dirty) this.rebuild(id)
    this.dirty.clear()
    this.structureChanged = false
    this.tailChanged = false
    return this.publish()
  }

  private setInfo(info: SessionInfo): void {
    const wasBusy = this.busy
    this.info = info
    if (wasBusy === this.busy) return
    // Whether the last reply is still streaming depends on it.
    for (const { entry } of this.entries) {
      if (entry.message.role === 'assistant') this.markDirty(entry.message.id)
    }
  }

  private upsertMessage(message: Message, now: number): void {
    const existing = this.byId.get(message.id)
    if (!existing) {
      this.addMessage(message, true, now)
      return
    }

    const { entry } = existing
    const previous = entry.message
    entry.message = { ...message, parts: previous.parts }
    const paceFrom = message.role === 'assistant' ? now : undefined
    message.parts.forEach((part, index) => this.setPart(entry, index, part, paceFrom))
    for (let index = message.parts.length; index < previous.parts.length; index++) this.dropView(entry, index)
    entry.message.parts.length = message.parts.length
    this.markDirty(message.id)
  }

  private replacePart(messageId: string, index: number, part: Part, now: number): void {
    const existing = this.byId.get(messageId)
    if (!existing) return // older than the loaded page, or removed
    if (index > existing.entry.message.parts.length) {
      this.options.reportError(
        `part ${index} of message ${messageId} arrived before part ${existing.entry.message.parts.length}`,
      )
      return
    }
    const paceFrom = existing.entry.message.role === 'assistant' ? now : undefined
    this.setPart(existing.entry, index, part, paceFrom)
    this.markDirty(messageId)
  }

  private applyDelta(messageId: string, index: number, delta: Delta, now: number): void {
    const existing = this.byId.get(messageId)
    if (!existing) return // older than the loaded page, or removed
    const { entry } = existing
    const parts = entry.message.parts

    if (index === parts.length) this.setPart(entry, index, emptyPartFor(delta, index, now), now)
    const part = parts[index]
    if (!part) {
      this.options.reportError(`delta for part ${index} of message ${messageId}, which has ${parts.length}`)
      return
    }

    if (delta.type === 'tool_input' && part.type === 'tool') {
      part.raw_input += delta.text
      if (part.purpose === '') part.purpose = partialPurpose(part.raw_input) ?? ''
    } else if (delta.type === part.type && (part.type === 'text' || part.type === 'reasoning')) {
      part.text += delta.text
      this.pacer.push({ stream: streamKey(messageId, index), text: delta.text }, now)
      this.pacedMessages.add(messageId)
    } else {
      this.options.reportError(
        `${delta.type} delta for a ${part.type} part (message ${messageId}, part ${index})`,
      )
      return
    }
    this.markDirty(messageId)
  }

  private removeMessage(messageId: string): void {
    const existing = this.byId.get(messageId)
    if (!existing) return
    existing.entry.views.forEach((_view, index) => this.pacer.discard(streamKey(messageId, index)))
    this.entries = this.entries.filter((candidate) => candidate !== existing)
    this.byId.delete(messageId)
    this.dirty.delete(messageId)
    this.structureChanged = true
  }

  // Removes the request and returns it, if it was pending.
  resolvePermission(callId: string): PermissionRequest | undefined {
    const request = this.permissions.get(callId)
    if (!request) return undefined
    this.permissions.delete(callId)
    this.markDirty(request.message_id)
    return request
  }

  resolveQuestion(callId: string): QuestionRequest | undefined {
    const request = this.questions.get(callId)
    if (!request) return undefined
    this.questions.delete(callId)
    this.markDirty(request.message_id)
    return request
  }

  // History loaded or replaced as a whole: shown at once, not paced.
  // `keepBlocks`: the messages that stay keep their blocks, so only what differs changes.
  private replaceMessages(messages: Message[], keepBlocks = false): void {
    const previous = keepBlocks ? this.byId : new Map<string, MessageBlocks>()
    const previousBlocks = new Map([...previous].map(([id, existing]) => [id, existing.blocks]))
    this.pacedMessages.clear()
    this.pacer.takeAll()
    this.entries = []
    this.byId.clear()
    this.dirty.clear()
    for (const message of messages) {
      const added = this.newEntry(message, false, 0)
      added.blocks = previousBlocks.get(message.id) ?? []
      this.entries.push(added)
    }
    this.structureChanged = true
  }

  private addMessage(message: Message, live: boolean, now: number): void {
    this.entries.push(this.newEntry(message, live, now))
    this.structureChanged = true
  }

  // Registers the message; the caller places it in `entries`.
  private newEntry(message: Message, live: boolean, now: number): MessageBlocks {
    const entry: MessageEntry = { message: { ...message, parts: [] }, views: new Map() }
    const existing: MessageBlocks = { entry, blocks: [], stale: false, lastCompaction: false }
    this.byId.set(message.id, existing)

    const paced = live && message.role === 'assistant'
    message.parts.forEach((part, index) => this.setPart(entry, index, part, paced ? now : undefined))
    this.markDirty(message.id)
    return existing
  }

  // Messages before the last compaction are stale; the compaction itself carries the note.
  private markCompactions(): void {
    const last = this.entries.findLastIndex(({ entry }) => entry.message.source.kind === 'compaction')
    this.entries.forEach((existing, index) => {
      const stale = index < last
      const lastCompaction = index === last
      if (existing.stale === stale && existing.lastCompaction === lastCompaction) return
      existing.stale = stale
      existing.lastCompaction = lastCompaction
      this.dirty.add(existing.entry.message.id)
    })
  }

  // Sets part `index` to `part`. `paceFrom`: the time to pace new text from, or undefined to
  // show it at once.
  private setPart(entry: MessageEntry, index: number, part: Part, paceFrom: number | undefined): void {
    const { message } = entry
    const previous = message.parts[index]
    message.parts[index] = part

    if (part.type !== 'text' && part.type !== 'reasoning') {
      this.dropView(entry, index)
      return
    }
    const sameKind = previous?.type === part.type
    if (!sameKind) this.dropView(entry, index)
    let view = entry.views.get(index)
    if (!view) {
      view = new PartView(part.type === 'text')
      entry.views.set(index, view)
    }

    const previousText =
      sameKind && (previous?.type === 'text' || previous?.type === 'reasoning') ? previous.text : ''
    const stream = streamKey(message.id, index)
    const extendsPrevious = part.text.startsWith(previousText)
    if (paceFrom !== undefined && extendsPrevious) {
      const added = part.text.slice(previousText.length)
      if (added === '') return
      this.pacer.push({ stream, text: added }, paceFrom)
      this.pacedMessages.add(message.id)
      return
    }
    this.pacer.discard(stream)
    view.setText(part.text)
  }

  private dropView(entry: MessageEntry, index: number): void {
    if (!entry.views.delete(index)) return
    this.pacer.discard(streamKey(entry.message.id, index))
  }

  private show(stream: string, text: string): void {
    const { messageId, index } = parseStreamKey(stream)
    const entry = this.byId.get(messageId)?.entry
    entry?.views.get(index)?.append(text)
    this.markDirty(messageId)
  }

  // A message whose last paced text was just shown stops streaming and may show its error.
  private markDrained(): void {
    for (const id of this.pacedMessages) {
      const entry = this.byId.get(id)?.entry
      const stillPaced =
        entry && entry.message.parts.some((_, index) => this.pacer.hasPending(streamKey(id, index)))
      if (stillPaced) continue
      this.pacedMessages.delete(id)
      this.markDirty(id)
    }
  }

  private markDirty(messageId: string): void {
    if (this.byId.has(messageId)) this.dirty.add(messageId)
  }

  private rebuild(messageId: string): void {
    const existing = this.byId.get(messageId)
    if (!existing) return
    const { entry } = existing
    const position = this.entries.indexOf(existing)
    const previousBlocks = new Map(existing.blocks.map((block) => [block.key, block]))

    const built = buildBlocks(entry, {
      sessionBusy: this.busy,
      hasOlderRows: this.moreBefore,
      isFirst: position === 0,
      stale: existing.stale,
      lastCompaction: existing.lastCompaction,
      isPacing: (index) => this.pacer.hasPending(streamKey(messageId, index)),
      permissions: this.permissions,
      questions: this.questions,
    })
    existing.blocks = built.map((block) => reuseIfEqual(previousBlocks.get(block.key), block))
  }

  // The session's error as a block at the end, unless the last message already shows it.
  private tailErrorBlock(): Block | undefined {
    const text = this.errorText
    const lastError = this.entries.at(-1)?.entry.message.error
    if (text === '' || text === lastError) {
      this.tailError = undefined
      return undefined
    }
    const block: Block = {
      kind: 'error',
      key: TAIL_ERROR_KEY,
      messageId: '',
      streaming: false,
      stale: false,
      text,
    }
    this.tailError = reuseIfEqual(this.tailError?.kind === 'error' ? this.tailError : undefined, block)
    return this.tailError
  }

  // The flat list, or the old array when nothing in it changed.
  private publish(): boolean {
    const next = this.entries.flatMap((existing) => existing.blocks)
    const tail = this.tailErrorBlock()
    if (tail) next.push(tail)
    const unchanged = next.length === this.flat.length && next.every((block, i) => block === this.flat[i])
    if (unchanged) return false
    this.flat = next
    return true
  }
}

function emptyPartFor(delta: Delta, index: number, now: number): Part {
  if (delta.type === 'text') return { type: 'text', text: '', time: now }
  if (delta.type === 'reasoning') return { type: 'reasoning', text: '', time: now, time_end: null }
  return {
    type: 'tool',
    call_id: `pending-${index}`,
    name: '',
    purpose: '',
    input: null,
    raw_input: '',
    state: 'streaming',
    output: null,
    time: now,
    time_end: null,
  }
}
