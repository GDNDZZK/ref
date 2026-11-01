import { describe, expect, it } from 'vitest'
import { collapseWhitespace, oneLine } from './oneLine.ts'

describe('oneLine', () => {
  it('collapses white space runs and trims the ends', () => {
    expect(oneLine('  Fix\n\n it\tnow  ')).toBe('Fix it now')
  })

  it('drops control characters', () => {
    expect(oneLine('a\u0007b\u007fc')).toBe('abc')
  })

  it('takes U+0085 as white space and keeps a byte order mark, as the TUI does', () => {
    expect(oneLine('a\u0085b')).toBe('a b')
    expect(oneLine('\ufeffa')).toBe('\ufeffa')
  })
})

describe('collapseWhitespace', () => {
  it('turns white space runs into one space and trims the ends', () => {
    expect(collapseWhitespace('  a \n\t b  ')).toBe('a b')
  })
})
