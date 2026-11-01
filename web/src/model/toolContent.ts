// What a tool card shows, read out of the call: the one-line detail, the icon and the structured
// body of each tool. The formats are the ones crates/tools write; anything that does not parse
// falls back to plain output.

import type { IconKey } from '../icons/generated/icons.ts'
import type { JsonValue, Todo, TodoStatus } from '../proto/generated'
import { truncateColumns } from './columns.ts'
import { oneLine } from './oneLine.ts'

const DETAIL_ARG_MAX_COLUMNS = 40
const DETAIL_ARG_MAX_COUNT = 3

const SEND_MESSAGE = 'send_message'
const CREATE_SESSION = 'create_session'
const READ_SESSION = 'read_session'
const CLOSE_SESSION = 'close_session'
const DIRECTORY_PREFIX = 'Directory '
const NO_FILES_FOUND = 'No files found'
const KNOWN_TODO_STATUSES: TodoStatus[] = ['in_progress', 'completed', 'cancelled']

const TOOL_ICONS: Record<string, IconKey> = {
  read: 'file-text-16',
  edit: 'pencil-16',
  write: 'file-plus-16',
  delete: 'trash-16',
  bash: 'terminal-16',
  grep: 'search-16',
  find: 'folder-search-16',
  webfetch: 'globe-16',
  todo: 'todo-16',
  [CREATE_SESSION]: 'chat-plus-16',
  [SEND_MESSAGE]: 'chat-send-16',
  message_status: 'chat-send-16',
  list_sessions: 'chat-list-16',
  [CLOSE_SESSION]: 'chat-x-16',
  [READ_SESSION]: 'chat-16',
}

export function toolIcon(name: string): IconKey {
  if (Object.hasOwn(TOOL_ICONS, name)) return TOOL_ICONS[name]!
  return name.includes('_') ? 'mcp-16' : 'tool-16'
}

function asObject(value: JsonValue | null | undefined): Record<string, JsonValue> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? value : undefined
}

export function stringArg(input: JsonValue | null | undefined, key: string): string | undefined {
  const value = asObject(input)?.[key]
  return typeof value === 'string' ? value : undefined
}

function firstLine(text: string): string {
  return text.split('\n')[0] ?? ''
}

export function parseTodos(input: JsonValue | null | undefined): Todo[] {
  const items = asObject(input)?.todos
  if (!Array.isArray(items)) return []
  return items.flatMap((item) => {
    const content = stringArg(item, 'content')?.trim()
    if (!content) return []
    const status = stringArg(item, 'status')
    return [{ content, status: KNOWN_TODO_STATUSES.find((candidate) => candidate === status) ?? 'pending' }]
  })
}

// The one line after the title: command, path, pattern, url, recipient, progress.
export function toolDetail(name: string, input: JsonValue | null | undefined): string {
  const detail = detailOf(name, input, (key) => stringArg(input, key))
  return detail === undefined ? genericDetail(input) : oneLine(detail)
}

function detailOf(
  name: string,
  input: JsonValue | null | undefined,
  arg: (key: string) => string | undefined,
): string | undefined {
  switch (name) {
    case 'bash': {
      const command = arg('command')
      return command === undefined ? undefined : `$ ${firstLine(command)}`
    }
    case 'read':
    case 'write':
    case 'delete':
      return arg('path')
    case 'edit': {
      const edits = asObject(input)?.edits
      const count = Array.isArray(edits) ? edits.length : 0
      const path = arg('path')
      return path !== undefined && count > 1 ? `${path} (${count} edits)` : path
    }
    case 'grep': {
      const pattern = arg('pattern')
      if (pattern === undefined) return undefined
      const path = arg('path')
      return path === undefined ? `"${pattern}"` : `"${pattern}" in ${path}`
    }
    case 'find':
      return arg('pattern') ?? arg('path')
    case 'webfetch': {
      const url = arg('url')
      const format = arg('format')
      return url !== undefined && format !== undefined && format !== 'markdown' ? `${url} (${format})` : url
    }
    case SEND_MESSAGE:
      return arg('to') === undefined ? undefined : `→ ${arg('to')}`
    case CREATE_SESSION:
      return arg('name') === undefined ? undefined : `${arg('name')} (${arg('agent') ?? ''})`
    case READ_SESSION:
    case CLOSE_SESSION:
    case 'context_window':
      return arg('session')
    case 'todo': {
      const todos = parseTodos(input)
      if (todos.length === 0) return undefined
      return `${todos.filter((todo) => todo.status === 'completed').length}/${todos.length} done`
    }
    default:
      return undefined
  }
}

// `key=value` of the first few plain arguments.
function genericDetail(input: JsonValue | null | undefined): string {
  const object = asObject(input)
  if (!object) return ''
  const parts: string[] = []
  for (const [key, value] of Object.entries(object)) {
    if (typeof value !== 'string' && typeof value !== 'number' && typeof value !== 'boolean') continue
    parts.push(`${key}=${truncateColumns(oneLine(String(value)), DETAIL_ARG_MAX_COLUMNS)}`)
    if (parts.length >= DETAIL_ARG_MAX_COUNT) break
  }
  return parts.join(' ')
}

// Only web addresses are opened from a tool card: the URL comes from the model.
export function isWebUrl(target: string): boolean {
  try {
    const { protocol } = new URL(target)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}

// The session a cross-session tool talks to (or creates).
export function peerName(name: string, input: JsonValue | null | undefined): string | undefined {
  if (name === SEND_MESSAGE) return stringArg(input, 'to')
  if (name === CREATE_SESSION) return stringArg(input, 'name')
  if ([READ_SESSION, CLOSE_SESSION, 'message_status'].includes(name)) return stringArg(input, 'session')
  return undefined
}

// ---- meta -----------------------------------------------------------------------------------

export function metaString(meta: JsonValue | null | undefined, key: string): string | undefined {
  return stringArg(meta, key)
}

export function metaNumber(meta: JsonValue | null | undefined, key: string): number | undefined {
  const value = asObject(meta)?.[key]
  return typeof value === 'number' ? value : undefined
}

export function metaFlag(meta: JsonValue | null | undefined, key: string): boolean {
  return asObject(meta)?.[key] === true
}

// ---- diff -----------------------------------------------------------------------------------

export type DiffKind = 'added' | 'removed' | 'hunk' | 'file' | 'context'

export interface DiffRow {
  kind: DiffKind
  sign: string // '+', '-' or ''
  text: string
}

export function parseDiff(diff: string): DiffRow[] {
  return diff
    .trimEnd()
    .split('\n')
    .map((line): DiffRow => {
      if (line.startsWith('+++') || line.startsWith('---')) return { kind: 'file', sign: '', text: line }
      if (line.startsWith('@@')) return { kind: 'hunk', sign: '', text: line }
      if (line.startsWith('+')) return { kind: 'added', sign: '+', text: line.slice(1) }
      if (line.startsWith('-')) return { kind: 'removed', sign: '-', text: line.slice(1) }
      return { kind: 'context', sign: '', text: line.startsWith(' ') ? line.slice(1) : line }
    })
}

// ---- read and grep: `[path#tag]` followed by numbered lines ------------------------------------

export interface NumberedLine {
  number: number
  text: string
  match: boolean // grep: the line matched, not only context
}

export type SourceRow = NumberedLine | 'gap'

export interface SourceFile {
  path: string
  tag: string
  rows: SourceRow[]
}

const HEADER = /^\[(.+)#([0-9A-Fa-f]{4,})\]$/
const NUMBERED = /^([ *]?)(\d+):(.*)$/

function parseSourceFile(block: string): SourceFile | undefined {
  const [header = '', ...lines] = block.split('\n')
  const found = HEADER.exec(header)
  if (!found) return undefined
  const rows = lines.flatMap((line): SourceRow[] => {
    if (line === '...') return ['gap']
    const numbered = NUMBERED.exec(line)
    if (!numbered) return []
    return [{ number: Number(numbered[2]), text: numbered[3] ?? '', match: numbered[1] === '*' }]
  })
  return { path: found[1] ?? '', tag: found[2] ?? '', rows }
}

// The part of the output after the last blank line that is a bracketed note.
function splitNotes(blocks: string[]): { content: string[]; notes: string[] } {
  const content: string[] = []
  const notes: string[] = []
  for (const block of blocks) {
    if (HEADER.test(firstLine(block)) || block.startsWith(DIRECTORY_PREFIX)) content.push(block)
    else if (block.startsWith('[')) notes.push(...block.split('\n'))
    else content.push(block)
  }
  return { content, notes }
}

export interface SourceListing {
  files: SourceFile[]
  notes: string[]
}

// read: one file; grep: one per file with matches. Undefined when the output is not in this format.
export function parseSourceListing(output: string): SourceListing | undefined {
  const { content, notes } = splitNotes(output.split('\n\n'))
  const files = content.flatMap((block) => parseSourceFile(block) ?? [])
  return files.length > 0 && files.length === content.length ? { files, notes } : undefined
}

// ---- find: one path per line, directories end in `/` ---------------------------------------------

export interface FoundPath {
  directory: string // with the trailing slash, or ''
  name: string
  isDir: boolean
}

export function parseFound(output: string): { paths: FoundPath[]; notes: string[] } {
  const [list = '', ...rest] = output.split('\n\n')
  const notes = rest.flatMap((block) => block.split('\n'))
  if (list.startsWith(NO_FILES_FOUND)) return { paths: [], notes: [] }
  const paths = list.split('\n').map((line): FoundPath => {
    const isDir = line.endsWith('/')
    const trimmed = isDir ? line.slice(0, -1) : line
    const slash = trimmed.lastIndexOf('/') + 1
    return { directory: trimmed.slice(0, slash), name: trimmed.slice(slash) + (isDir ? '/' : ''), isDir }
  })
  return { paths, notes }
}

// ---- long text --------------------------------------------------------------------------------

export interface LimitedText {
  text: string
  hidden: number // lines cut off
}

// The first `max` lines of `text`, all of it when `max` is undefined.
export function limitLines(text: string, max: number | undefined): LimitedText {
  const lines = text.trimEnd().split('\n')
  if (max === undefined || lines.length <= max) return { text: lines.join('\n'), hidden: 0 }
  return { text: lines.slice(0, max).join('\n'), hidden: lines.length - max }
}

// The last `max` lines, for live output.
export function tailLines(text: string, max: number): LimitedText {
  const lines = text.trimEnd().split('\n')
  if (lines.length <= max) return { text: lines.join('\n'), hidden: 0 }
  return { text: lines.slice(-max).join('\n'), hidden: lines.length - max }
}
