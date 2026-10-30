import { describe, expect, it } from 'vitest'
import { columnsOf, truncateColumns } from './columns.ts'

describe('columnsOf', () => {
  it('counts terminal columns', () => {
    expect(columnsOf('abc')).toBe(3)
    expect(columnsOf('中文')).toBe(4)
    expect(columnsOf('e\u0301')).toBe(1) // a letter and a combining accent
    expect(columnsOf('🚀')).toBe(2)
    expect(columnsOf('👨‍👩‍👧')).toBe(2) // one emoji built from several
    expect(columnsOf('')).toBe(0)
  })
})

describe('truncateColumns', () => {
  it('keeps text that fits', () => {
    expect(truncateColumns('abc', 3)).toBe('abc')
    expect(truncateColumns('中中', 4)).toBe('中中')
  })

  it('cuts with an ellipsis inside the limit', () => {
    expect(truncateColumns('abcdef', 4)).toBe('abc…')
    expect(truncateColumns('中中中', 4)).toBe('中…')
    expect(truncateColumns('中中中', 5)).toBe('中中…')
  })

  it('does not split a grapheme', () => {
    expect(truncateColumns('e\u0301e\u0301e\u0301', 2)).toBe('e\u0301…')
  })

  it('gives nothing for no room', () => {
    expect(truncateColumns('abc', 0)).toBe('')
  })
})
