// What a sidebar row or a tile header says about a session, from its SessionInfo.

import { formatElapsed } from '../model/format.ts'
import { oneLine } from '../model/oneLine.ts'
import type { SessionInfo, Todo } from '../proto/generated'

export type RowState = 'idle' | 'busy' | 'compacting' | 'retrying' | 'permission' | 'question' | 'error'

// The dot and the mark of a row. A session that is idle but whose last turn ended in an error
// shows the error until the next turn starts (the backend clears `last_error`).
export function rowState(info: SessionInfo): RowState {
  switch (info.status.state) {
    case 'busy':
    case 'running':
      return 'busy'
    case 'compacting':
      return 'compacting'
    case 'retrying':
      return 'retrying'
    case 'waiting_permission':
      return 'permission'
    case 'waiting_question':
      return 'question'
    case 'idle':
      return info.last_error ? 'error' : 'idle'
  }
}

// Rows that need the user's attention are drawn apart from the rest.
export function needsAttention(state: RowState): boolean {
  return state === 'permission' || state === 'question' || state === 'error'
}

export interface TodoProgress {
  done: number
  total: number
  complete: boolean // all done
}

// Cancelled items count for nothing, as in the TUI. Undefined when there is nothing to do.
export function todoProgress(todos: readonly Todo[] | undefined): TodoProgress | undefined {
  const counted = (todos ?? []).filter((todo) => todo.status !== 'cancelled')
  if (counted.length === 0) return undefined
  const done = counted.filter((todo) => todo.status === 'completed').length
  return { done, total: counted.length, complete: done === counted.length }
}

// The turn's running time, or undefined while the session is idle.
export function runningTime(info: SessionInfo, now: number): string | undefined {
  return info.busy_since == null ? undefined : formatElapsed(now - info.busy_since)
}

// The last component of a directory: "/home/me/src/ref/" -> "ref".
export function dirName(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part !== '')
  return parts[parts.length - 1] ?? path
}

// A live session is labelled by its name, as in the TUI; the title is secondary.
export function liveLabel(info: SessionInfo): string {
  return info.name.trim() || oneLine(info.title) || dirName(info.cwd)
}

// A session from history is labelled by its one-line title, then its name, then its directory.
export function historyLabel(info: SessionInfo): string {
  return oneLine(info.title) || info.name.trim() || dirName(info.cwd)
}

const MS_PER_SECOND = 1000
const SECONDS_PER_MINUTE = 60
const SECONDS_PER_HOUR = 3600
const MS_PER_MINUTE = SECONDS_PER_MINUTE * MS_PER_SECOND
const MS_PER_HOUR = SECONDS_PER_HOUR * MS_PER_SECOND

export type RelativeTime =
  | { kind: 'justNow' }
  | { kind: 'minutes'; count: number }
  | { kind: 'hours'; count: number }
  | { kind: 'yesterday' }
  | { kind: 'date'; date: Date; withYear: boolean }

function startOfDay(time: number): number {
  const date = new Date(time)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

// "just now", "5m ago", "3h ago" (today), "Yesterday", then the date (with the year from another year).
export function relativeTime(then: number, now: number): RelativeTime {
  const age = now - then
  if (age < MS_PER_MINUTE) return { kind: 'justNow' }

  const today = startOfDay(now)
  if (then >= today) {
    return age < MS_PER_HOUR
      ? { kind: 'minutes', count: Math.floor(age / MS_PER_MINUTE) }
      : { kind: 'hours', count: Math.floor(age / MS_PER_HOUR) }
  }
  const yesterday = startOfDay(today - 1)
  if (then >= yesterday) return { kind: 'yesterday' }

  const date = new Date(then)
  return { kind: 'date', date, withYear: date.getFullYear() !== new Date(now).getFullYear() }
}

// Case-insensitive search over title, name, directory and id; empty `query` keeps everything.
export function matchesQuery(info: SessionInfo, query: string): boolean {
  const wanted = query.trim().toLowerCase()
  if (wanted === '') return true
  return [info.title, info.name, info.cwd, info.id].some((field) => field.toLowerCase().includes(wanted))
}
