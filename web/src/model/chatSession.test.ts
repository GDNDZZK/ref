import { describe, expect, it, vi } from 'vitest'
import type { RefApi } from '../net/api.ts'
import { ChatSession } from './chatSession.ts'
import { info, snapshot, userMessage } from './testMessages.ts'

function setup(sessionInfo = info({ id: 'ses_1' })) {
  const api = {
    attach: vi.fn().mockResolvedValue(snapshot([userMessage('u1', 'hello')], { info: sessionInfo })),
    detach: vi.fn().mockResolvedValue(undefined),
  }
  const notify = vi.fn()
  const session = new ChatSession('ses_1', { api: () => api as unknown as RefApi, notify }, sessionInfo)
  return { api, notify, session }
}

describe('ChatSession', () => {
  it('knows what the session list says before it is attached', () => {
    const { session } = setup()
    expect(session.info?.name).toBe('test')
    expect(session.attachment).toBe('detached')
    expect(session.loaded).toBe(false)
  })

  it('shows the history after attaching and says how it is attached', async () => {
    const { api, session } = setup()
    await session.attach(true)
    expect(api.attach).toHaveBeenCalledWith('ses_1', true)
    expect(session.attachment).toBe('foreground')
    expect(session.blocks.map((block) => block.kind)).toEqual(['user'])
    expect(session.generation).toBe(1)
  })

  it('attaches as a watcher for split view tiles', async () => {
    const { session } = setup()
    await session.attach(false)
    expect(session.attachment).toBe('watching')
  })

  it('keeps the history when it is detached', async () => {
    const { api, session } = setup()
    await session.attach(true)
    await session.detach()
    expect(api.detach).toHaveBeenCalledWith('ses_1')
    expect(session.attachment).toBe('detached')
    expect(session.blocks).toHaveLength(1)
  })

  it('ignores events that come before the snapshot', () => {
    const { session } = setup()
    session.applyEvent({ type: 'message', message: userMessage('u2', 'early') })
    expect(session.blocks).toEqual([])
  })

  it('takes the new status from the session list, loaded or not', async () => {
    const { session } = setup()
    const busy = info({ id: 'ses_1', status: { state: 'busy' } })
    session.setInfo(busy)
    expect(session.busy).toBe(true)
    await session.attach(true)
    session.setInfo(info({ id: 'ses_1' }))
    expect(session.busy).toBe(false)
  })

  it('names the session in an error it shows from the background', async () => {
    const { notify, session } = setup()
    await session.attach(false)
    session.applyEvent({ type: 'error', text: 'rate limited' })
    expect(notify).toHaveBeenCalledWith('error', 'test: rate limited')
    await session.attach(true)
    session.applyEvent({ type: 'error', text: 'again' })
    expect(notify).toHaveBeenLastCalledWith('error', 'again')
  })
})
