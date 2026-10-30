import { describe, expect, it } from 'vitest'
import { partialPurpose } from './toolArgs.ts'

describe('partialPurpose', () => {
  it('returns a complete purpose', () => {
    expect(partialPurpose('{"purpose": "Run the tests", "command": "cargo test"}')).toBe('Run the tests')
  })

  it('returns the part that has streamed in so far', () => {
    expect(partialPurpose('{"purpose": "Run the te')).toBe('Run the te')
  })

  it('turns \\n and \\t into spaces and unescapes other characters', () => {
    expect(partialPurpose('{"purpose": "a\\nb\\"c"}')).toBe('a b"c')
  })

  it('is undefined until the value starts', () => {
    expect(partialPurpose('{"command": "ls"')).toBeUndefined()
    expect(partialPurpose('{"purpose"')).toBeUndefined()
    expect(partialPurpose('{"purpose": ')).toBeUndefined()
    expect(partialPurpose('{"purpose": "')).toBeUndefined()
  })
})
