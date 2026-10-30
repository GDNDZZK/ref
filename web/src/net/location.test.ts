import { describe, expect, it } from 'vitest'
import { withLocationState } from './location.ts'

const base = new URL('http://127.0.0.1:7480/?dir=%2Fwork')

describe('withLocationState', () => {
  it('adds the session and keeps ?dir=', () => {
    expect(withLocationState(base, { sessionId: 'ses_1' }).search).toBe('?dir=%2Fwork&session=ses_1')
  })

  it('drops what is no longer open', () => {
    const open = withLocationState(base, { sessionId: 'ses_1', split: true })
    expect(open.search).toBe('?dir=%2Fwork&session=ses_1&view=split')
    expect(withLocationState(open, { sessionId: undefined, split: false }).search).toBe('?dir=%2Fwork')
  })

  it('leaves fields it is not told about', () => {
    const open = withLocationState(base, { sessionId: 'ses_1', split: true })
    expect(withLocationState(open, { split: false }).search).toBe('?dir=%2Fwork&session=ses_1')
  })
})
