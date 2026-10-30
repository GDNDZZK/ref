import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fadeWindow, span, track } from './anim.ts'

test('span is 0 before, 1 after, eased in between', () => {
  assert.equal(span(0.5, 1, 2), 0)
  assert.equal(span(4, 1, 2), 1)
  assert.equal(span(2, 1, 2), 0.5)
})

function approx(actual: number, expected: number) {
  assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} is not ${expected}`)
}

test('fadeWindow fades in and out around the interval', () => {
  approx(fadeWindow(0, 1, 5, 0.5), 0)
  approx(fadeWindow(3, 1, 5, 0.5), 1)
  approx(fadeWindow(5, 1, 5, 0.5), 0)
})

test('track holds the ends and interpolates linearly', () => {
  const value = track([
    { at: 1, value: 10 },
    { at: 3, value: 30, ease: 'linear' },
  ])
  assert.equal(value(0), 10)
  assert.equal(value(2), 20)
  assert.equal(value(9), 30)
})

test('track needs keys', () => {
  assert.throws(() => track([]))
})
