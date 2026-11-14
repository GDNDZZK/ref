import { describe, expect, it } from 'vitest'
import { colors } from './generated/theme.ts'
import type { ColorName } from './generated/theme.ts'

function luminance(hex: string): number {
  const channel = (i: number): number => {
    const v = parseInt(hex.slice(1 + 2 * i, 3 + 2 * i), 16) / 255
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2)
}

function contrast(a: string, b: string): number {
  const la = luminance(a)
  const lb = luminance(b)
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

const texts: ColorName[] = ['accentText', 'successText', 'dangerText', 'warningText', 'infoText']
const surfaces: ColorName[] = ['background', 'surface', 'userBubble', 'codeBackground']

describe('tone text colors', () => {
  for (const theme of ['light', 'dark'] as const) {
    for (const text of texts) {
      for (const surface of surfaces) {
        it(`${text} reads at 4.5:1 on ${surface} (${theme})`, () => {
          expect(contrast(colors[theme][text], colors[theme][surface])).toBeGreaterThanOrEqual(4.5)
        })
      }
    }
  }

  it('reads at 4.5:1 on the surface of its own tone', () => {
    for (const theme of ['light', 'dark'] as const) {
      expect(contrast(colors[theme].warningText, colors[theme].warningSurface)).toBeGreaterThanOrEqual(4.5)
      expect(contrast(colors[theme].dangerText, colors[theme].errorSurface)).toBeGreaterThanOrEqual(4.5)
    }
  })
})
