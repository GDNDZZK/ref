import { describe, expect, it } from 'vitest'
import { mentionAt } from './mention.ts'

describe('mentionAt', () => {
  it('finds the mention before the cursor', () => {
    expect(mentionAt('look at @src/ma', 15)).toEqual({ start: 8, query: 'src/ma' })
    expect(mentionAt('@', 1)).toEqual({ start: 0, query: '' })
  })

  it('ends at whitespace', () => {
    expect(mentionAt('look at @src/ma x', 17)).toBeUndefined()
  })

  it('needs the @ to start a word', () => {
    expect(mentionAt('mail me@host', 12)).toBeUndefined()
  })

  it('only looks before the cursor', () => {
    expect(mentionAt('see @src and more', 8)).toEqual({ start: 4, query: 'src' })
  })
})
