import { describe, expect, it } from 'vitest'
import { StreamPacer } from './streamPacer.ts'
import type { PacedText } from './streamPacer.ts'

const START = 0

function lengthOf(chunks: PacedText[]): number {
  return chunks.reduce((sum, chunk) => sum + chunk.text.length, 0)
}

function render(chunks: PacedText[]): string[] {
  return chunks.map((chunk) => `${chunk.stream}:${chunk.text}`)
}

const text = (value: string): PacedText => ({ stream: 'text', text: value })
const reasoning = (value: string): PacedText => ({ stream: 'reasoning', text: value })

describe('StreamPacer', () => {
  // With no gap seen yet, the pacer assumes the longest one.
  it('spreads the first arrival over MaxLag', () => {
    const pacer = new StreamPacer()
    pacer.push(text('a'.repeat(100)), START)

    expect(lengthOf(pacer.take(START))).toBe(0)
    const halfway = lengthOf(pacer.take(START + 100))
    expect(halfway).toBeGreaterThanOrEqual(45)
    expect(halfway).toBeLessThanOrEqual(55)
    expect(lengthOf(pacer.take(START + 200))).toBe(100 - halfway)
    expect(pacer.isEmpty()).toBe(true)
  })

  // 120 characters every 100 ms: once the gap is known, each frame shows about the same
  // amount, and each burst runs out about when the next one comes.
  it('shows bursts at a steady rate', () => {
    const pacer = new StreamPacer()
    const bursts = 10
    const gap = 100
    const frame = 10
    const perFrame: number[] = []
    for (let burst = 0; burst < bursts; burst++) {
      const arrival = START + burst * gap
      pacer.push(text('a'.repeat(120)), arrival)
      for (let at = frame; at <= gap; at += frame) perFrame.push(lengthOf(pacer.take(arrival + at)))
    }

    // From the third burst on, the gap estimate has settled at 100 ms: 12 per 10 ms frame.
    for (const shown of perFrame.slice((2 * gap) / frame)) {
      expect(shown).toBeGreaterThanOrEqual(11)
      expect(shown).toBeLessThanOrEqual(13)
    }
  })

  // A server that sends a token every 16 ms trails by about a frame, not by MaxLag.
  it('shows token streams promptly', () => {
    const pacer = new StreamPacer()
    let shown = 0
    let sent = 0
    for (let token = 0; token < 50; token++) {
      const arrival = START + token * 16
      pacer.push(text('word '), arrival)
      sent += 5
      shown += lengthOf(pacer.take(arrival + 8))
    }
    expect(sent - shown).toBeLessThanOrEqual(10)
  })

  it('keeps streams in order and merges adjacent chunks of one stream', () => {
    const pacer = new StreamPacer()
    pacer.push(reasoning('thin'), START)
    pacer.push(reasoning('k'), START)
    pacer.push(text('say'), START)

    expect(render(pacer.take(START + 1000))).toEqual(['reasoning:think', 'text:say'])
  })

  // "a😀" is three UTF-16 code units; the emoji is a surrogate pair that must come out whole.
  it('never splits a grapheme', () => {
    const pacer = new StreamPacer()
    const input = 'a\u{1F600}'
    pacer.push(text(input), START)

    let shown = ''
    for (let now = START; !pacer.isEmpty(); now += 1) {
      for (const chunk of pacer.take(now)) {
        const lastUnit = chunk.text.charCodeAt(chunk.text.length - 1)
        const splitsPair = lastUnit >= 0xd800 && lastUnit <= 0xdbff
        expect(splitsPair).toBe(false)
        shown += chunk.text
      }
    }
    expect(shown).toBe(input)
  })

  // After a pause the next burst still starts from nothing shown, not all at once.
  it('does not bank idle time', () => {
    const pacer = new StreamPacer()
    pacer.push(text('abcd'), START)
    expect(lengthOf(pacer.take(START + 1000))).toBe(4)

    pacer.push(text('a'.repeat(100)), START + 5000)
    expect(lengthOf(pacer.take(START + 5000 + 1))).toBeLessThan(10)
  })

  it('takeAll empties it', () => {
    const pacer = new StreamPacer()
    pacer.push(text('ab'), START)
    pacer.push(reasoning('cd'), START)

    expect(render(pacer.takeAll())).toEqual(['text:ab', 'reasoning:cd'])
    expect(pacer.isEmpty()).toBe(true)
    expect(lengthOf(pacer.take(START + 1000))).toBe(0)
  })

  it('reports and discards what waits for one stream', () => {
    const pacer = new StreamPacer()
    pacer.push(text('ab'), START)
    pacer.push(reasoning('cd'), START)
    expect(pacer.hasPending('text')).toBe(true)

    pacer.discard('text')

    expect(pacer.hasPending('text')).toBe(false)
    expect(render(pacer.takeAll())).toEqual(['reasoning:cd'])
  })
})
