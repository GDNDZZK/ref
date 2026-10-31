// What the conversation view draws. One row = one block, not one message: a message's parts
// become reasoning, markdown, code and tool blocks, so each animates and folds on its own.
//
// Blocks are immutable records. A change replaces the block's object and leaves every
// other block's object alone, so the view re-renders only what changed.

import type { JsonValue, PermissionRequest, QuestionRequest, ToolState } from '../proto/generated'

export type UserSource = 'human' | 'peer' | 'spawn' | 'reminder' | 'compaction' | 'shell'

// An image the messages carry as a stub; `getImage` fetches the bytes.
export interface ImageRef {
  id: string | null
  name: string | null
  mime: string
  size: number | null
}

interface BlockBase {
  key: string // stable across updates: `<message id>:<part index>[:<segment>]`
  messageId: string
  streaming: boolean // text or tool call still being produced
  stale: boolean // before the last compaction: no longer sent to the model
}

export interface UserBlock extends BlockBase {
  kind: 'user'
  text: string
  source: UserSource
  senderName: string | undefined // other sessions' messages
  opensTurn: boolean // draws the divider above it
  images: ImageRef[]
}

export interface ReasoningBlock extends BlockBase {
  kind: 'reasoning'
  text: string
}

export interface MarkdownBlock extends BlockBase {
  kind: 'markdown'
  text: string
}

export interface CodeBlock extends BlockBase {
  kind: 'code'
  language: string // empty without an info string
  text: string
}

export interface ToolBlock extends BlockBase {
  kind: 'tool'
  callId: string
  name: string
  purpose: string
  state: ToolState
  input: JsonValue
  rawInput: string // the arguments as streamed, while `input` is not parsed yet
  output: string | null
  meta: JsonValue | null
  time: number
  timeEnd: number | null
  runAt: number | null // Delayed: when execution starts
  permission: PermissionRequest | undefined // set while the call waits for the user
  question: QuestionRequest | undefined // set while a `question` call waits for the answers
  images: ImageRef[]
}

export interface ErrorBlock extends BlockBase {
  kind: 'error'
  text: string
}

// The footer under a message.
export interface MetaBlock extends BlockBase {
  kind: 'meta'
  role: 'user' | 'assistant'
  source: UserSource
  time: number
  timeEnd: number | null // null while the reply is still being written
  agent: string | null
  model: string | null
  variant: string | null
  tokensIn: number // input + cache read + cache write
  tokensOut: number
  interrupted: boolean
  failed: boolean
  canUndo: boolean
  copyable: boolean // the message has text to copy
}

// The message that summarizes the history before it.
export interface CompactionBlock extends BlockBase {
  kind: 'compaction'
  text: string
  time: number
  last: boolean // the last compaction of the session
}

export type Block =
  | UserBlock
  | ReasoningBlock
  | MarkdownBlock
  | CodeBlock
  | ToolBlock
  | ErrorBlock
  | MetaBlock
  | CompactionBlock

export function isFinalToolState(state: ToolState): boolean {
  return state === 'done' || state === 'error' || state === 'denied' || state === 'cancelled'
}

// Same value, looking into arrays and plain objects (image lists are rebuilt on every build).
function sameValue(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false
  if (Array.isArray(a) !== Array.isArray(b)) return false
  const keys = Object.keys(a)
  if (keys.length !== Object.keys(b).length) return false
  return keys.every((key) =>
    sameValue((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]),
  )
}

// Keeps `previous` when `next` says the same, so the view sees no change.
export function reuseIfEqual<T extends Block>(previous: T | undefined, next: T): T {
  if (!previous || previous.kind !== next.kind) return next
  const keys = Object.keys(next) as (keyof T)[]
  return keys.every((key) => sameValue(previous[key], next[key])) ? previous : next
}
