// What the conversation view draws. One row = one block, not one message: a message's parts
// become reasoning, markdown, code and tool blocks, so each animates and folds on its own.
//
// Blocks are immutable records. A change replaces the block's object and leaves every
// other block's object alone, so the view re-renders only what changed.

import type { JsonValue, PermissionRequest, ToolState } from '../proto/generated'

export type UserSource = 'human' | 'peer' | 'spawn' | 'reminder' | 'compaction' | 'shell'

interface BlockBase {
  key: string // stable across updates: `<message id>:<part index>[:<segment>]`
  messageId: string
  streaming: boolean // text or tool call still being produced
}

export interface UserBlock extends BlockBase {
  kind: 'user'
  text: string
  source: UserSource
  senderName: string | undefined // other sessions' messages
  opensTurn: boolean // draws the divider above it
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
}

export interface ErrorBlock extends BlockBase {
  kind: 'error'
  text: string
}

export type Block = UserBlock | ReasoningBlock | MarkdownBlock | CodeBlock | ToolBlock | ErrorBlock

export function isFinalToolState(state: ToolState): boolean {
  return state === 'done' || state === 'error' || state === 'denied' || state === 'cancelled'
}

// Keeps `previous` when `next` says the same, so the view sees no change.
export function reuseIfEqual<T extends Block>(previous: T | undefined, next: T): T {
  if (!previous || previous.kind !== next.kind) return next
  const keys = Object.keys(next) as (keyof T)[]
  return keys.every((key) => previous[key] === next[key]) ? previous : next
}
