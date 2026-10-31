// Message -> blocks.
//
//   user message          -> one user block
//   assistant message     -> per part: text -> markdown / code blocks (cut at code fences)
//                                      reasoning -> reasoning block, tool -> tool block
//                            then the message's error, if it ended in one
//   compaction message    -> one compaction block
//   every other message   -> ends with a meta block

import type { Message, PermissionRequest, QuestionRequest } from '../proto/generated'
import { isFinalToolState } from './blocks.ts'
import type { Block, CompactionBlock, ImageRef, MetaBlock, UserBlock, UserSource } from './blocks.ts'
import type { PartView } from './partView.ts'
import { partialPurpose } from './toolArgs.ts'

export interface MessageEntry {
  message: Message
  views: Map<number, PartView> // text and reasoning parts, by part index
}

export interface BuildContext {
  sessionBusy: boolean
  hasOlderRows: boolean // messages exist above the first loaded one
  isFirst: boolean // the first loaded message
  stale: boolean // before the last compaction
  lastCompaction: boolean // this is the last compaction message
  isPacing(partIndex: number): boolean // the pacer still holds text of that part
  permissions: ReadonlyMap<string, PermissionRequest>
  questions: ReadonlyMap<string, QuestionRequest>
}

const INTERRUPTED = 'aborted' // the error text of a turn the user interrupted; not shown as an error
const TURN_OPENING_SOURCES: UserSource[] = ['human', 'peer', 'spawn']

export function messageText(message: Message): string {
  return message.parts.flatMap((part) => (part.type === 'text' ? [part.text] : [])).join('\n\n')
}

function imageRef(image: {
  name?: string | null
  mime: string
  id?: string | null
  size?: number | null
}): ImageRef {
  return { id: image.id ?? null, name: image.name ?? null, mime: image.mime, size: image.size ?? null }
}

function userBlock(message: Message, context: BuildContext): UserBlock {
  const text = messageText(message)
  const source = message.source.kind
  const senderName =
    message.source.kind === 'peer' || message.source.kind === 'spawn' ? message.source.from_name : undefined
  const startsTurn = TURN_OPENING_SOURCES.includes(source) && !(context.isFirst && !context.hasOlderRows)
  return {
    kind: 'user',
    key: `${message.id}:user`,
    messageId: message.id,
    streaming: false,
    stale: context.stale,
    text,
    source,
    senderName,
    opensTurn: startsTurn,
    images: message.parts.flatMap((part) => (part.type === 'image' ? [imageRef(part)] : [])),
  }
}

function compactionBlock(message: Message, context: BuildContext): CompactionBlock {
  return {
    kind: 'compaction',
    key: `${message.id}:compaction`,
    messageId: message.id,
    streaming: false,
    stale: context.stale,
    text: messageText(message),
    time: message.time,
    last: context.lastCompaction,
  }
}

function metaBlock(message: Message, context: BuildContext, open: boolean): MetaBlock {
  const { usage } = message
  return {
    kind: 'meta',
    key: `${message.id}:meta`,
    messageId: message.id,
    streaming: open,
    stale: context.stale,
    role: message.role,
    source: message.source.kind,
    time: message.time,
    timeEnd: message.time_end,
    agent: message.agent,
    model: message.model?.model ?? null,
    variant: message.model?.variant ?? null,
    tokensIn: usage ? usage.input + usage.cache_read + usage.cache_write : 0,
    tokensOut: usage?.output ?? 0,
    interrupted: isInterrupted(message),
    failed: message.error !== null && !isInterrupted(message),
    canUndo: message.role === 'user' && message.source.kind === 'human',
    copyable: messageText(message) !== '',
  }
}

function isInterrupted(message: Message): boolean {
  return message.error?.toLowerCase() === INTERRUPTED
}

function assistantBlocks(entry: MessageEntry, context: BuildContext): Block[] {
  const { message } = entry
  const open = message.time_end === null && message.error === null && context.sessionBusy
  const lastIndex = message.parts.length - 1
  const blocks: Block[] = []

  message.parts.forEach((part, index) => {
    const streaming = context.isPacing(index) || (open && index === lastIndex)
    const base = { messageId: message.id, stale: context.stale }

    if (part.type === 'tool') {
      blocks.push({
        ...base,
        kind: 'tool',
        key: `${message.id}:${index}`,
        streaming: !isFinalToolState(part.state),
        callId: part.call_id,
        name: part.name,
        purpose: part.purpose || partialPurpose(part.raw_input) || '',
        state: part.state,
        input: part.input,
        rawInput: part.raw_input,
        output: part.output,
        meta: part.meta ?? null,
        time: part.time,
        timeEnd: part.time_end,
        runAt: part.run_at ?? null,
        permission: context.permissions.get(part.call_id),
        question: context.questions.get(part.call_id),
        images: (part.images ?? []).map(imageRef),
      })
      return
    }

    const view = entry.views.get(index)
    if (!view) return // an image: not shown yet
    if (!streaming) view.finish()

    if (part.type === 'reasoning') {
      if (view.shown !== '')
        blocks.push({
          ...base,
          kind: 'reasoning',
          key: `${message.id}:${index}`,
          streaming,
          text: view.shown,
        })
      return
    }

    view.segments.forEach((segment, s) => {
      const segmentBase = {
        ...base,
        key: `${message.id}:${index}:${s}`,
        streaming: streaming && s === view.segments.length - 1,
        text: segment.text,
      }
      blocks.push(
        segment.kind === 'code'
          ? { ...segmentBase, kind: 'code', language: segment.language }
          : { ...segmentBase, kind: 'markdown' },
      )
    })
  })

  const pacing = message.parts.some((_, index) => context.isPacing(index))
  const failed = message.error !== null && !isInterrupted(message)
  if (failed && !pacing) {
    blocks.push({
      kind: 'error',
      key: `${message.id}:error`,
      messageId: message.id,
      streaming: false,
      stale: context.stale,
      text: message.error!,
    })
  }
  blocks.push(metaBlock(message, context, open || pacing))
  return blocks
}

export function buildBlocks(entry: MessageEntry, context: BuildContext): Block[] {
  const { message } = entry
  if (message.role === 'assistant') return assistantBlocks(entry, context)
  if (message.source.kind === 'compaction') return [compactionBlock(message, context)]
  return [userBlock(message, context), metaBlock(message, context, false)]
}
