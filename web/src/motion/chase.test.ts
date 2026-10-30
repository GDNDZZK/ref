import { describe, expect, it } from 'vitest'
import { motion } from '../theme/generated/motion.ts'
import { arrived, chase, scrollArrived, scrollStep } from './chase.ts'

const FRAME_60 = 1 / 60

describe('chase', () => {
  it('covers 1 - e^(-dt / followLag) of the distance per frame', () => {
    const covered = 1 - Math.exp(-(FRAME_60 * 1000) / motion.followLag)
    expect(chase(0, 100, FRAME_60)).toBeCloseTo(100 * covered, 9)
  })

  it('reaches the same place at 60 and 120 Hz', () => {
    let at60 = 0
    let at120 = 0
    for (let i = 0; i < 30; i++) at60 = chase(at60, 100, 1 / 60)
    for (let i = 0; i < 60; i++) at120 = chase(at120, 100, 1 / 120)
    expect(at60).toBeCloseTo(at120, 6)
  })

  it('snaps to the target within chaseSnap', () => {
    expect(arrived(10, 10 + motion.chaseSnap - 0.01)).toBe(true)
    expect(arrived(10, 10 + motion.chaseSnap)).toBe(false)
    expect(chase(10, 10.3, FRAME_60)).toBe(10.3)
  })

  it('trails a target moving at constant speed by speed x followLag', () => {
    const pixelsPerFrame = 1
    let position = 0
    for (let frame = 1; frame <= 300; frame++) position = chase(position, frame * pixelsPerFrame, FRAME_60)
    const speed = pixelsPerFrame / FRAME_60 // px/s
    const expectedLag = (speed * motion.followLag) / 1000
    expect(300 * pixelsPerFrame - position).toBeCloseTo(expectedLag, 0)
  })
})

describe('scrollStep', () => {
  it('moves in whole pixels', () => {
    expect(Number.isInteger(scrollStep(100.4, 900, FRAME_60))).toBe(true)
  })

  it('advances at least one pixel per frame so rounding cannot stall it', () => {
    // 4 px to go at 120 Hz: the plain chase moves < 1 px and would round away
    expect(scrollStep(996, 1000, 1 / 120)).toBe(997)
    expect(scrollStep(1004, 1000, 1 / 120)).toBe(1003)
  })

  it('does not overshoot a fractional target', () => {
    expect(scrollStep(999, 999.5, FRAME_60)).toBe(999.5)
  })

  it('arrives within a pixel', () => {
    expect(scrollArrived(999.2, 1000)).toBe(true)
    expect(scrollArrived(998, 1000)).toBe(false)
  })
})
