import { describe, expect, it } from 'vitest'
import { HISTORY_MAX, PromptHistory } from './history.ts'

describe('PromptHistory', () => {
  it('steps back through the entries and returns the draft at the end', () => {
    const history = new PromptHistory()
    history.push('one')
    history.push('two')
    expect(history.prev('draft')).toBe('two')
    expect(history.prev('')).toBe('one')
    expect(history.prev('')).toBeUndefined()
    expect(history.next()).toBe('two')
    expect(history.next()).toBe('draft')
    expect(history.browsing).toBe(false)
  })

  it('does nothing without entries or when not browsing', () => {
    const history = new PromptHistory()
    expect(history.prev('x')).toBeUndefined()
    expect(history.next()).toBeUndefined()
  })

  it('skips blanks and a repeat of the newest entry', () => {
    const history = new PromptHistory()
    history.push('one  \n')
    history.push('one')
    history.push('   ')
    expect(history.entries).toEqual(['one'])
  })

  it('starts again from the newest entry after reset', () => {
    const history = new PromptHistory(['a', 'b'])
    history.prev('')
    history.prev('')
    history.reset()
    expect(history.browsing).toBe(false)
    expect(history.prev('')).toBe('b')
  })

  it('keeps the newest entries only', () => {
    const history = new PromptHistory()
    for (let i = 0; i < HISTORY_MAX + 3; i++) history.push(`p${i}`)
    expect(history.entries).toHaveLength(HISTORY_MAX)
    expect(history.entries[0]).toBe('p3')
  })
})
