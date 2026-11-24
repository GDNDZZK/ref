// One session's conversation as blocks.
//
//   snapshot / events --> Transcript --flush(now), once per frame--> Block[] + TranscriptSlot[]
//                           |  text growth goes through the StreamPacer first
//
// Pure TypeScript, no Vue: the store wraps `blocks` and `slots` in shallowRefs and calls `flush`
// from the frame loop. Events change the model at once; what the view shows changes only in
// `flush`, which also releases paced text, so a burst of events costs one view update.

import type {
  Delta,
  Message,
  MessageLayoutMeta,
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
import { layoutMetaFromMessage } from './layoutEstimator.ts'
import { PartView } from './partView.ts'
import { StreamPacer } from './streamPacer.ts'
import { partialPurpose } from './toolArgs.ts'

export const PAGE_SIZE = 100
export const TAIL_KEEP_MESSAGES = 100
export const EVICT_BUFFER = 150

export type TranscriptSlot =
  | {
      kind: 'materialized'
      seq: number
      id: string
      message: Message
      blocks: readonly Block[]
      stale: boolean
      lastCompaction: boolean
    }
  | {
      kind: 'placeholder'
      seq: number
      id: string
      meta: MessageLayoutMeta
    }

export interface TranscriptOptions {
  reportError(message: string): void // an event that does not fit the conversation
}

interface MessageBlocks {
  seq: number
  entry: MessageEntry
  blocks: Block[]
  stale: boolean // before the last compaction
  lastCompaction: boolean
}

type SlotModel =
  | { kind: 'materialized'; seq: number; entry: MessageBlocks }
  | { kind: 'placeholder'; seq: number; id: string; meta: MessageLayoutMeta }

const TAIL_ERROR_KEY = 'session:error'
const STREAM_SEPARATOR = '@'

type TextLikePart = Extract<Part, { type: 'text' | 'reasoning' }>

function isTextLike(part: Part | undefined): part is TextLikePart {
  return part?.type === 'text' || part?.type === 'reasoning'
}

// A pacer stream is one part of one message: "<part index>@<message id>".
function streamKey(messageId: string, index: number): string {
  return `${index}${STREAM_SEPARATOR}${messageId}`
}

function parseStreamKey(stream: string): { messageId: string; index: number } {
  const at = stream.indexOf(STREAM_SEPARATOR)
  return { index: Number(stream.slice(0, at)), messageId: stream.slice(at + 1) }
}

function fallbackMeta(seq: number): MessageLayoutMeta {
  return {
    seq,
    id: `placeholder-${seq}`,
    role: 'user',
    source_kind: 'human',
    has_error: false,
    parts: [],
  }
}

export class Transcript {
  private readonly options: TranscriptOptions
  private readonly pacer = new StreamPacer()
  private entries: MessageBlocks[] = []
  private slotModels: SlotModel[] = []
  private readonly byId = new Map<string, MessageBlocks>()
  private readonly dirty = new Set<string>() // messages whose blocks must be rebuilt
  private readonly pacedMessages = new Set<string>() // messages with text in the pacer
  private structureChanged = false
  private tailChanged = false
  private flat: readonly Block[] = []
  private publishedSlots: readonly TranscriptSlot[] = []
  private tailError: Block | undefined

  info: SessionInfo | undefined
  moreBefore = false
  closed = false
  readonly permissions = new Map<string, PermissionRequest>()
  readonly questions = new Map<string, QuestionRequest>()
  queued: Message[] = []
  // The last `Error` event; it stays until the user sends the next prompt.
  private errorText = ''

  constructor(options: TranscriptOptions) {
    this.options = options
  }

  get blocks(): readonly Block[] {
    return this.flat
  }

  get slots(): readonly TranscriptSlot[] {
    return this.publishedSlots
  }

  get totalCount(): number {
    return this.slotModels.length
  }

  get residentCount(): number {
    return this.entries.length
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

  // Returns contiguous [startSeq, endSeq) bounds containing placeholders around [start, end).
  missingRangeAround(start: number, end: number): { startSeq: number; endSeq: number } | undefined {
    const total = this.slotModels.length
    if (total === 0) return undefined
    const clampedStart = Math.max(0, Math.min(start, total - 1))
    const clampedEnd = Math.max(clampedStart + 1, Math.min(end, total))

    let firstMissing = -1
    let lastMissing = -1
    for (let seq = clampedStart; seq < clampedEnd; seq++) {
      if (this.slotModels[seq]?.kind !== 'placeholder') continue
      if (firstMissing < 0) firstMissing = seq
      lastMissing = seq
    }
    if (firstMissing < 0) return undefined

    let reqStart = firstMissing
    let reqEnd = lastMissing + 1
    // Expand contiguous placeholder block up to PAGE_SIZE so adjacent scrolling does not thrash.
    while (reqEnd - reqStart < PAGE_SIZE) {
      const canExpandLeft = reqStart > 0 && this.slotModels[reqStart - 1]?.kind === 'placeholder'
      const canExpandRight = reqEnd < total && this.slotModels[reqEnd]?.kind === 'placeholder'
      if (!canExpandLeft && !canExpandRight) break
      if (canExpandLeft) reqStart--
      if (reqEnd - reqStart < PAGE_SIZE && canExpandRight) reqEnd++
    }
    return { startSeq: reqStart, endSeq: reqEnd }
  }

  // A prompt was sent: the error of the turn before is history.
  clearError(): void {
    if (this.errorText === '') return
    this.errorText = ''
    this.tailChanged = true
  }

  // A page of older messages: shown at once, above what is there.
  prepend(messages: Message[], moreBefore: boolean, startSeq?: number): void {
    const firstMaterialized = this.entries[0]?.seq ?? this.slotModels.length
    if (firstMaterialized > 0) {
      const seq = startSeq ?? Math.max(0, firstMaterialized - messages.length)
      this.insertRange(seq, seq + messages.length, messages)
      this.moreBefore = moreBefore
      return
    }
    const addedMessages = messages.filter((message) => !this.byId.has(message.id))
    const shift = addedMessages.length
    for (const slot of this.slotModels) {
      slot.seq += shift
      if (slot.kind === 'materialized') slot.entry.seq = slot.seq
      else slot.meta = { ...slot.meta, seq: slot.seq }
    }
    const addedSlots: SlotModel[] = addedMessages.map((message, index) => ({
      kind: 'materialized',
      seq: index,
      entry: this.newEntry(message, index, false, 0),
    }))
    this.slotModels = [...addedSlots, ...this.slotModels]
    this.rebuildEntriesOrder()
    this.moreBefore = moreBefore
    this.structureChanged = true
  }

  insertRange(startSeq: number, endSeq: number, messages: Message[]): void {
    if (messages.length === 0 || startSeq < 0) return
    let changed = false
    for (let i = 0; i < messages.length; i++) {
      const seq = startSeq + i
      if (seq >= endSeq) break
      const message = messages[i]!
      if (this.byId.has(message.id)) continue
      while (this.slotModels.length <= seq) {
        const padSeq = this.slotModels.length
        const meta = fallbackMeta(padSeq)
        this.slotModels.push({ kind: 'placeholder', seq: padSeq, id: meta.id, meta })
      }
      const existingSlot = this.slotModels[seq]
      if (existingSlot?.kind === 'materialized' && existingSlot.entry.entry.message.id === message.id) {
        continue
      }
      const added = this.newEntry(message, seq, false, 0)
      this.slotModels[seq] = { kind: 'materialized', seq, entry: added }
      changed = true
    }
    if (!changed) return
    this.rebuildEntriesOrder()
    this.structureChanged = true
  }

  // Evicts materialized messages outside [keepStart, keepEnd] inclusive, converting them to placeholders.
  evictOutside(keepStart: number, keepEnd: number): string[] {
    const evicted: string[] = []
    for (let i = 0; i < this.slotModels.length; i++) {
      const slot = this.slotModels[i]!
      if (slot.kind !== 'materialized') continue
      if (slot.seq >= keepStart && slot.seq <= keepEnd) continue
      const message = slot.entry.entry.message
      const id = message.id
      slot.entry.entry.views.forEach((_view, index) => this.pacer.discard(streamKey(id, index)))
      this.byId.delete(id)
      this.dirty.delete(id)
      this.pacedMessages.delete(id)
      this.slotModels[i] = {
        kind: 'placeholder',
        seq: slot.seq,
        id,
        meta: layoutMetaFromMessage(slot.seq, message),
      }
      evicted.push(id)
    }
    if (evicted.length === 0) return evicted
    this.rebuildEntriesOrder()
    this.structureChanged = true
    return evicted
  }

  // Evicts materialized messages older than the newest `keepCount`.
  evictBeforeTail(keepCount: number): string[] {
    const total = this.slotModels.length
    const keepStart = Math.max(0, total - Math.max(0, keepCount))
    return this.evictOutside(keepStart, total - 1)
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
    this.replaceWindow(
      snapshot.messages,
      snapshot.start_seq ?? snapshot.layout_metas?.length ?? 0,
      snapshot.total_messages,
      snapshot.layout_metas ?? [],
      false,
    )
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
        this.replaceWindow(
          event.messages,
          event.start_seq ?? event.layout_metas?.length ?? 0,
          event.total_messages,
          event.layout_metas ?? [],
          true,
        )
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
  // Returns true when `blocks` or `slots` is a new array.
  flush(now: number): boolean {
    for (const chunk of this.pacer.take(now)) this.show(chunk.stream, chunk.text)
    this.markDrained()
    if (this.dirty.size === 0 && !this.structureChanged && !this.tailChanged) return false

    const wasStructured = this.structureChanged
    // Whether a message is the first decides its turn divider.
    const first = this.entries[0]
    if (wasStructured && first) this.dirty.add(first.entry.message.id)
    if (wasStructured) this.markCompactions()
    for (const id of this.dirty) this.rebuild(id)
    this.dirty.clear()
    this.structureChanged = false
    this.tailChanged = false
    return this.publish(wasStructured)
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
    if (!existing) return // older than the loaded page, evicted, or removed
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
    if (!existing) return // older than the loaded page, evicted, or removed
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
    this.slotModels = this.slotModels.filter(
      (slot) => !(slot.kind === 'materialized' && slot.entry === existing),
    )
    this.reindexSlots()
    this.rebuildEntriesOrder()
    this.byId.delete(messageId)
    this.dirty.delete(messageId)
    this.pacedMessages.delete(messageId)
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
  private replaceWindow(
    messages: Message[],
    startSeq: number,
    totalMessages: number | undefined,
    layoutMetas: readonly MessageLayoutMeta[],
    keepBlocks: boolean,
  ): void {
    const previous = keepBlocks ? this.byId : new Map<string, MessageBlocks>()
    const previousBlocks = new Map([...previous].map(([id, existing]) => [id, existing.blocks]))
    this.pacedMessages.clear()
    this.pacer.takeAll()
    this.entries = []
    this.slotModels = []
    this.byId.clear()
    this.dirty.clear()

    const metaBySeq = new Map<number, MessageLayoutMeta>()
    for (const meta of layoutMetas) metaBySeq.set(meta.seq, meta)

    const effectiveStart = Math.max(0, startSeq)
    for (let seq = 0; seq < effectiveStart; seq++) {
      const meta = metaBySeq.get(seq) ?? layoutMetas[seq] ?? fallbackMeta(seq)
      this.slotModels.push({ kind: 'placeholder', seq, id: meta.id, meta })
    }

    messages.forEach((message, index) => {
      const seq = effectiveStart + index
      const added = this.newEntry(message, seq, false, 0)
      added.blocks = previousBlocks.get(message.id) ?? []
      this.entries.push(added)
      this.slotModels.push({ kind: 'materialized', seq, entry: added })
    })

    const expectedTotal = Math.max(totalMessages ?? this.slotModels.length, this.slotModels.length)
    while (this.slotModels.length < expectedTotal) {
      const seq = this.slotModels.length
      const meta = metaBySeq.get(seq) ?? fallbackMeta(seq)
      this.slotModels.push({ kind: 'placeholder', seq, id: meta.id, meta })
    }
    this.structureChanged = true
  }

  private addMessage(message: Message, live: boolean, now: number): void {
    const seq = this.slotModels.length
    const added = this.newEntry(message, seq, live, now)
    this.entries.push(added)
    this.slotModels.push({ kind: 'materialized', seq, entry: added })
    this.structureChanged = true
  }

  // Registers the message; the caller places it in `entries` and `slotModels`.
  private newEntry(message: Message, seq: number, live: boolean, now: number): MessageBlocks {
    const entry: MessageEntry = { message: { ...message, parts: [] }, views: new Map() }
    const existing: MessageBlocks = { seq, entry, blocks: [], stale: false, lastCompaction: false }
    this.byId.set(message.id, existing)

    const paced = live && message.role === 'assistant'
    message.parts.forEach((part, index) => this.setPart(entry, index, part, paced ? now : undefined))
    this.markDirty(message.id)
    return existing
  }

  private reindexSlots(): void {
    this.slotModels.forEach((slot, index) => {
      slot.seq = index
      if (slot.kind === 'materialized') slot.entry.seq = index
      else slot.meta = { ...slot.meta, seq: index }
    })
  }

  private rebuildEntriesOrder(): void {
    const next: MessageBlocks[] = []
    for (const slot of this.slotModels) {
      if (slot.kind === 'materialized') next.push(slot.entry)
    }
    this.entries = next
  }

  // Messages before the last compaction are stale; the compaction itself carries the note.
  private markCompactions(): void {
    const lastSlotIndex = this.slotModels.findLastIndex((slot) =>
      slot.kind === 'materialized'
        ? slot.entry.entry.message.source.kind === 'compaction'
        : slot.meta.source_kind === 'compaction',
    )
    this.slotModels.forEach((slot, slotIndex) => {
      if (slot.kind !== 'materialized') return
      const existing = slot.entry
      const stale = slotIndex < lastSlotIndex
      const lastCompaction = slotIndex === lastSlotIndex
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

    const previousText = sameKind && isTextLike(previous) ? previous.text : ''
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
    const previousBlocks = new Map(existing.blocks.map((block) => [block.key, block]))

    const built = buildBlocks(entry, {
      sessionBusy: this.busy,
      hasOlderRows: this.moreBefore,
      isFirst: existing.seq === 0,
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
  private publish(forceSlots: boolean): boolean {
    const next = this.entries.flatMap((existing) => existing.blocks)
    const tail = this.tailErrorBlock()
    if (tail) next.push(tail)
    const blocksUnchanged =
      next.length === this.flat.length && next.every((block, i) => block === this.flat[i])
    if (!blocksUnchanged) this.flat = next

    const prevSlots = this.publishedSlots
    let slotsChanged = forceSlots || this.slotModels.length !== prevSlots.length
    const nextSlots: TranscriptSlot[] = this.slotModels.map((slot, index) => {
      const prev = prevSlots[index]
      if (slot.kind === 'placeholder') {
        if (
          prev?.kind === 'placeholder' &&
          prev.seq === slot.seq &&
          prev.id === slot.id &&
          prev.meta === slot.meta
        ) {
          return prev
        }
        slotsChanged = true
        return { kind: 'placeholder', seq: slot.seq, id: slot.id, meta: slot.meta }
      }
      const message = slot.entry.entry.message
      const blocks = slot.entry.blocks
      const stale = slot.entry.stale
      const lastCompaction = slot.entry.lastCompaction
      if (
        prev?.kind === 'materialized' &&
        prev.seq === slot.seq &&
        prev.id === message.id &&
        prev.message === message &&
        prev.blocks === blocks &&
        prev.stale === stale &&
        prev.lastCompaction === lastCompaction
      ) {
        return prev
      }
      slotsChanged = true
      return {
        kind: 'materialized',
        seq: slot.seq,
        id: message.id,
        message,
        blocks,
        stale,
        lastCompaction,
      }
    })
    if (slotsChanged) this.publishedSlots = nextSlots

    return !blocksUnchanged || slotsChanged
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
