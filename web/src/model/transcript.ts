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
  SessionEvent,
  SessionInfo,
  SessionSnapshot,
} from '../proto/generated'
import { reuseIfEqual } from './blocks.ts'
import type { Block } from './blocks.ts'
import { buildBlocks } from './buildBlocks.ts'
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
}

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
  private flat: readonly Block[] = []

  info: SessionInfo | undefined
  moreBefore = false
  closed = false
  readonly permissions = new Map<string, PermissionRequest>()
  queued: Message[] = []

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
    return this.dirty.size > 0 || this.structureChanged || !this.pacer.isEmpty()
  }

  load(snapshot: SessionSnapshot): void {
    this.info = snapshot.info
    this.moreBefore = snapshot.more_before
    this.queued = snapshot.queued
    this.closed = false
    this.permissions.clear()
    for (const request of snapshot.pending_permissions) this.permissions.set(request.call_id, request)
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
        this.replaceMessages(event.messages)
        break
      case 'permission':
        this.permissions.set(event.request.call_id, event.request)
        this.markDirty(event.request.message_id)
        break
      case 'permission_resolved':
        this.resolvePermission(event.call_id)
        break
      case 'queue':
        this.queued = event.messages
        break
      case 'closed':
        this.closed = true
        break
      case 'error':
        break // a notice for the user, not part of the conversation; the store shows it
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
    if (this.dirty.size === 0 && !this.structureChanged) return false

    // Whether a message is the first decides its turn divider.
    const first = this.entries[0]
    if (this.structureChanged && first) this.dirty.add(first.entry.message.id)
    for (const id of this.dirty) this.rebuild(id)
    this.dirty.clear()
    this.structureChanged = false
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

  private resolvePermission(callId: string): void {
    const request = this.permissions.get(callId)
    if (!request) return
    this.permissions.delete(callId)
    this.markDirty(request.message_id)
  }

  // History loaded or replaced as a whole: shown at once, not paced.
  private replaceMessages(messages: Message[]): void {
    this.pacedMessages.clear()
    this.pacer.takeAll()
    this.entries = []
    this.byId.clear()
    this.dirty.clear()
    for (const message of messages) this.addMessage(message, false, 0)
    this.structureChanged = true
  }

  private addMessage(message: Message, live: boolean, now: number): void {
    const entry: MessageEntry = { message: { ...message, parts: [] }, views: new Map() }
    const existing: MessageBlocks = { entry, blocks: [] }
    this.entries.push(existing)
    this.byId.set(message.id, existing)

    const paced = live && message.role === 'assistant'
    message.parts.forEach((part, index) => this.setPart(entry, index, part, paced ? now : undefined))
    this.markDirty(message.id)
    this.structureChanged = true
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
      isPacing: (index) => this.pacer.hasPending(streamKey(messageId, index)),
      permissions: this.permissions,
    })
    existing.blocks = built.map((block) => reuseIfEqual(previousBlocks.get(block.key), block))
  }

  // The flat list, or the old array when nothing in it changed.
  private publish(): boolean {
    const next = this.entries.flatMap((existing) => existing.blocks)
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
