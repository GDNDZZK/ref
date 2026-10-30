import { describe, expect, it } from 'vitest'
import { easingNameFromQml, outBack, outElastic, plainEasings } from './easing.ts'
import type { EasingFn } from './easing.ts'
import qtFixtures from './__fixtures__/qt-easing.json'

// Samples of Qt 6.12's own easing code, see scripts/qt-easing-ref.
interface QtCurve {
  name: string
  amplitude: number
  period: number
  samples: number[]
}

const EXACT = 1e-12

function ourCurve(curve: QtCurve): EasingFn {
  if (curve.name === 'outElastic') return outElastic(curve.amplitude, curve.period)
  return plainEasings[curve.name as keyof typeof plainEasings]
}

describe('easing against Qt', () => {
  for (const curve of qtFixtures as QtCurve[]) {
    const label = curve.name === 'outElastic' ? `outElastic(${curve.amplitude}, ${curve.period})` : curve.name
    it(`${label} matches Qt at every sample`, () => {
      const ours = ourCurve(curve)
      const steps = curve.samples.length - 1
      curve.samples.forEach((expected, i) => {
        expect(ours(i / steps)).toBeCloseTo(expected, 11)
        expect(Math.abs(ours(i / steps) - expected)).toBeLessThan(EXACT)
      })
    })
  }
})

describe('easing endpoints and clamping', () => {
  it.each(Object.entries(plainEasings))('%s starts at 0 and ends at 1', (_name, fn) => {
    expect(fn(0)).toBeCloseTo(0, 12) // inOutSine gives -0, as Qt does
    expect(fn(1)).toBe(1)
  })

  it.each(Object.entries(plainEasings))('%s clamps progress outside [0, 1]', (_name, fn) => {
    expect(fn(-0.5)).toBe(fn(0))
    expect(fn(1.5)).toBe(fn(1))
  })

  it('outBack overshoots 1 and settles at 1', () => {
    const peak = Math.max(...Array.from({ length: 101 }, (_, i) => outBack()(i / 100)))
    expect(peak).toBeGreaterThan(1.09)
    expect(peak).toBeLessThan(1.11)
  })

  it('outElastic with a larger amplitude swings further', () => {
    const peak = (fn: EasingFn) => Math.max(...Array.from({ length: 1001 }, (_, i) => fn(i / 1000)))
    expect(peak(outElastic(2, 0.35))).toBeGreaterThan(peak(outElastic(1, 0.35)))
  })
})

describe('easingNameFromQml', () => {
  it('lower-cases the first letter', () => {
    expect(easingNameFromQml('OutExpo')).toBe('outExpo')
    expect(easingNameFromQml('Linear')).toBe('linear')
  })

  it('rejects curves that are not ported', () => {
    expect(() => easingNameFromQml('OutBounce')).toThrow('not ported')
  })
})
