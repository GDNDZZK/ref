import { describe, expect, it } from 'vitest'
import { relativeTimeText } from './relativeTimeText.ts'

const t = (key: string, named: Record<string, unknown> = {}) => `${key}:${JSON.stringify(named)}`

describe('relativeTimeText', () => {
  it('translates the recent kinds with their count', () => {
    expect(relativeTimeText({ kind: 'minutes', count: 5 }, t, 'en')).toBe('time.minutesAgo:{"n":5}')
    expect(relativeTimeText({ kind: 'yesterday' }, t, 'en')).toBe('time.yesterday:{}')
  })

  it('writes dates in the interface language, with the year only when asked', () => {
    const date = new Date(2025, 2, 4)
    expect(relativeTimeText({ kind: 'date', date, withYear: false }, t, 'en')).toBe('Mar 4')
    expect(relativeTimeText({ kind: 'date', date, withYear: true }, t, 'en')).toBe('Mar 4, 2025')
  })
})
