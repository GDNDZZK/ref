import { describe, expect, it } from 'vitest'
import { info } from '../model/testMessages.ts'
import type { Todo } from '../proto/generated'
import {
  dirName,
  displayTitle,
  formatElapsed,
  matchesQuery,
  relativeTime,
  rowState,
  runningTime,
  todoProgress,
} from './sessionRow.ts'

const todo = (status: Todo['status']): Todo => ({ content: 'x', status })

describe('rowState', () => {
  it('follows the status', () => {
    expect(rowState(info({ status: { state: 'running' } }))).toBe('busy')
    expect(rowState(info({ status: { state: 'waiting_permission' } }))).toBe('permission')
    expect(rowState(info({ status: { state: 'retrying', attempt: 1, next_at: 0, error: 'x' } }))).toBe(
      'retrying',
    )
    expect(rowState(info({ status: { state: 'compacting' } }))).toBe('compacting')
  })

  it('shows the error of the last turn on an idle session only', () => {
    expect(rowState(info({ last_error: 'boom' }))).toBe('error')
    expect(rowState(info({ last_error: 'boom', status: { state: 'busy' } }))).toBe('busy')
    expect(rowState(info())).toBe('idle')
  })
})

describe('todoProgress', () => {
  it('does not count cancelled items', () => {
    const todos = [todo('completed'), todo('in_progress'), todo('pending'), todo('cancelled')]
    expect(todoProgress(todos)).toEqual({ done: 1, total: 3, complete: false })
  })

  it('is complete when everything counted is done', () => {
    expect(todoProgress([todo('completed'), todo('cancelled'), todo('completed')])).toEqual({
      done: 2,
      total: 2,
      complete: true,
    })
  })

  it('is absent without todos', () => {
    expect(todoProgress([])).toBeUndefined()
    expect(todoProgress(undefined)).toBeUndefined()
    expect(todoProgress([todo('cancelled')])).toBeUndefined()
  })
})

describe('formatElapsed', () => {
  it('uses seconds, minutes and hours', () => {
    expect(formatElapsed(45_000)).toBe('45s')
    expect(formatElapsed(185_000)).toBe('3m 05s')
    expect(formatElapsed(3_720_000)).toBe('1h 02m')
    expect(formatElapsed(-5)).toBe('0s')
  })

  it('has a running time only while busy', () => {
    expect(runningTime(info({ busy_since: 1000 }), 6000)).toBe('5s')
    expect(runningTime(info(), 6000)).toBeUndefined()
  })
})

describe('names', () => {
  it('takes the last component of a directory', () => {
    expect(dirName('/home/me/ref/')).toBe('ref')
    expect(dirName('C:\\work\\app')).toBe('app')
    expect(dirName('/')).toBe('/')
  })

  it('falls back to the name while the session has no title', () => {
    expect(displayTitle(info({ title: '', name: 'parser' }))).toBe('parser')
    expect(displayTitle(info({ title: 'Fix it', name: 'parser' }))).toBe('Fix it')
  })
})

describe('relativeTime', () => {
  const now = new Date(2026, 9, 31, 15, 0, 0).getTime()
  const at = (hours: number, minutes = 0, day = 31, year = 2026) =>
    new Date(year, 9, day, hours, minutes).getTime()

  it('counts minutes and hours within today', () => {
    expect(relativeTime(now - 30_000, now)).toEqual({ kind: 'justNow' })
    expect(relativeTime(at(14, 55), now)).toEqual({ kind: 'minutes', count: 5 })
    expect(relativeTime(at(12, 0), now)).toEqual({ kind: 'hours', count: 3 })
  })

  it('says yesterday for the previous calendar day, even when it is under 24 hours ago', () => {
    expect(relativeTime(at(23, 30, 30), now)).toEqual({ kind: 'yesterday' })
  })

  it('gives the date after that, with the year from another year', () => {
    expect(relativeTime(at(9, 0, 4), now)).toMatchObject({ kind: 'date', withYear: false })
    expect(relativeTime(at(9, 0, 4, 2025), now)).toMatchObject({ kind: 'date', withYear: true })
  })
})

describe('matchesQuery', () => {
  const session = info({ title: 'Fix the Parser', name: 'work', cwd: '/home/me/ref', id: 'ses_abc' })

  it('looks in title, name, directory and id, ignoring case', () => {
    for (const query of ['parser', 'WORK', 'me/ref', 'ses_ab'])
      expect(matchesQuery(session, query)).toBe(true)
    expect(matchesQuery(session, 'nothing')).toBe(false)
  })

  it('keeps everything for an empty query', () => {
    expect(matchesQuery(session, '  ')).toBe(true)
  })
})
