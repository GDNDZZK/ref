import { describe, expect, it } from 'vitest'
import { HISTORY_MAX, HistoryBrowser, PromptHistory } from './history.ts'

function browserOver(...items: string[]) {
  const history = new PromptHistory(items)
  return { history, browser: new HistoryBrowser(history) }
}

describe('PromptHistory', () => {
  it('skips blanks and a repeat of the newest entry', () => {
    const history = new PromptHistory()
    expect(history.push('one  \n')).toBe(true)
    expect(history.push('one')).toBe(false)
    expect(history.push('   ')).toBe(false)
    expect(history.entries).toEqual(['one'])
  })

  it('keeps the newest entries only', () => {
    const history = new PromptHistory()
    for (let i = 0; i < HISTORY_MAX + 3; i++) history.push(`p${i}`)
    expect(history.entries).toHaveLength(HISTORY_MAX)
    expect(history.entries[0]).toBe('p3')
  })
})

describe('HistoryBrowser', () => {
  it('steps back through the entries and returns the draft at the end', () => {
    const { browser } = browserOver('one', 'two')
    expect(browser.prev('draft')).toBe('two')
    expect(browser.prev('')).toBe('one')
    expect(browser.prev('')).toBeUndefined()
    expect(browser.next()).toBe('two')
    expect(browser.next()).toBe('draft')
    expect(browser.browsing).toBe(false)
  })

  it('does nothing without entries or when not browsing', () => {
    const { browser } = browserOver()
    expect(browser.prev('x')).toBeUndefined()
    expect(browser.next()).toBeUndefined()
  })

  it('starts again from the newest entry after reset', () => {
    const { browser } = browserOver('a', 'b')
    browser.prev('')
    browser.prev('')
    browser.reset()
    expect(browser.browsing).toBe(false)
    expect(browser.prev('')).toBe('b')
  })

  it('walks independently of other browsers over the same history', () => {
    const { history, browser } = browserOver('a', 'b')
    const other = new HistoryBrowser(history)
    expect(browser.prev('x')).toBe('b')
    expect(other.prev('y')).toBe('b')
    expect(browser.prev('')).toBe('a')
    expect(other.next()).toBe('y')
  })
})
