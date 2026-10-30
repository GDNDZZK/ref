import { beforeEach, describe, expect, it } from 'vitest'
import { ALERT_TITLE_PREFIX, documentAttention } from './attention.ts'

beforeEach(() => {
  document.head.innerHTML = '<link rel="icon" href="/favicon.svg" type="image/svg+xml" />'
  document.title = 'ref'
})

function iconHref(): string {
  return document.querySelector<HTMLLinkElement>('link[rel="icon"]')!.getAttribute('href')!
}

describe('documentAttention', () => {
  it('marks the title and swaps the icon, and undoes both', () => {
    const attention = documentAttention(document)
    attention.setAlerting(true)
    expect(document.title).toBe(`${ALERT_TITLE_PREFIX}ref`)
    expect(iconHref()).toBe('/favicon-alert.svg')

    attention.setAlerting(false)
    expect(document.title).toBe('ref')
    expect(iconHref()).toBe('/favicon.svg')
  })

  it('keeps the mark when the base title changes', () => {
    const attention = documentAttention(document)
    attention.setAlerting(true)
    attention.setBaseTitle('Fix bug · ref')
    expect(document.title).toBe(`${ALERT_TITLE_PREFIX}Fix bug · ref`)
    attention.setAlerting(false)
    expect(document.title).toBe('Fix bug · ref')
  })
})
